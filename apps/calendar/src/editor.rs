//! The event editor: a dialog for a new event or changes to one.

use crate::actions;
use crate::draw;
use crate::widgets;
use crate::window;
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use gtk::prelude::*;
use nixe_calendar_core::{Ends, Event, Repeat, Span, whens};
use nixe_eds::Draft;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What a new event starts with.
#[derive(Clone, Debug, Default)]
pub struct Start {
    pub title: String,
    pub span: Option<Span>,
    pub calendar: Option<String>,
    pub repeat: Option<String>,
}

/// Alert choices: minutes before the start (negative: after it).
const TIMED_ALERTS: &[i64] = &[0, 5, 10, 15, 30, 60, 120, 1440];
const DAY_ALERTS: &[i64] = &[-540, 900, 10_620];

pub fn alert_label(minutes: i64) -> String {
    match minutes {
        0 => "At the start".into(),
        -540 => "On the day at 9 AM".into(),
        900 => "The day before at 9 AM".into(),
        10_620 => "A week before at 9 AM".into(),
        m if m < 0 => format!("{} min after the start", -m),
        m if m % 1440 == 0 => plural(m / 1440, "day"),
        m if m % 60 == 0 => plural(m / 60, "hour"),
        m => format!("{m} min before"),
    }
}

fn plural(n: i64, unit: &str) -> String {
    if n == 1 { format!("1 {unit} before") } else { format!("{n} {unit}s before") }
}

/// A date shown on a button that opens a calendar.
struct DateField {
    button: gtk::MenuButton,
    date: Rc<Cell<NaiveDate>>,
}

impl DateField {
    fn new(date: NaiveDate, on_change: impl Fn(NaiveDate) + 'static) -> Self {
        let date = Rc::new(Cell::new(date));
        let button = gtk::MenuButton::new();
        button.add_css_class("date-field");
        button.set_label(&date.get().format("%a, %b %-d, %Y").to_string());
        let calendar = gtk::Calendar::new();
        if let Ok(dt) = glib::DateTime::from_local(date.get().year(), date.get().month() as i32, date.get().day() as i32, 0, 0, 0.0) {
            calendar.select_day(&dt);
        }
        let popover = gtk::Popover::new();
        popover.set_child(Some(&calendar));
        button.set_popover(Some(&popover));
        let (d, b, p) = (date.clone(), button.clone(), popover.clone());
        calendar.connect_day_selected(move |cal| {
            let picked = cal.date();
            if let Some(day) = NaiveDate::from_ymd_opt(picked.year(), picked.month() as u32, picked.day_of_month() as u32) {
                d.set(day);
                b.set_label(&day.format("%a, %b %-d, %Y").to_string());
                on_change(day);
            }
            p.popdown();
        });
        Self { button, date }
    }

    fn set(&self, day: NaiveDate) {
        self.date.set(day);
        self.button.set_label(&day.format("%a, %b %-d, %Y").to_string());
    }
}

/// Reads "2:30 PM", "14:30", "noon", "9".
fn parse_time(text: &str) -> Option<NaiveTime> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    nixe_when::parse(&format!("at {t}"), Local::now().naive_local()).start
}

fn time_entry(time: NaiveTime) -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.add_css_class("time-field");
    entry.set_text(&draw::time_text(time));
    entry.set_width_chars(9);
    entry.connect_changed(|e| {
        if parse_time(&e.text()).is_some() {
            e.remove_css_class("error");
        } else {
            e.add_css_class("error");
        }
    });
    entry
}

fn labeled(text: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("editor-row");
    let label = widgets::label(text, "editor-label");
    label.set_xalign(0.0);
    label.set_width_chars(9);
    row.append(&label);
    widget.set_hexpand(true);
    row.append(widget);
    row
}

/// Opens the editor for `event`, or for a new event when `event` is `None`.
pub fn open(event: Option<&Event>, start: Option<Start>) {
    let event = event.cloned();
    let start = start.unwrap_or_default();
    let now = Local::now().naive_local();
    let default_start = NaiveDateTime::new(now.date(), NaiveTime::from_hms_opt((now.hour() + 1).min(23), 0, 0).unwrap_or_default());
    let span = event.as_ref().map(|e| e.span).or(start.span).unwrap_or(Span::Timed { start: default_start, end: default_start + Duration::hours(1) });
    let (mut from_date, mut to_date, from_time, to_time, all_day) = match span {
        Span::AllDay { first, last } => (first, last, NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default(), NaiveTime::from_hms_opt(10, 0, 0).unwrap_or_default(), true),
        Span::Timed { start, end } => (start.date(), end.date(), start.time(), end.time(), false),
    };
    if to_date < from_date {
        to_date = from_date;
    }
    if from_date > to_date {
        from_date = to_date;
    }

    let writable: Vec<(String, String)> = window::calendars().into_iter().filter(|c| !c.read_only).map(|c| (c.uid, c.name)).collect();
    if writable.is_empty() {
        window::toast("There's no calendar to add events to.");
        return;
    }

    let (dialog, card) = widgets::dialog(if event.is_some() { "Edit event" } else { "New event" }, 560);
    card.add_css_class("event-editor");

    let title = gtk::Entry::new();
    title.set_placeholder_text(Some("Title"));
    title.add_css_class("editor-title");
    title.set_text(event.as_ref().map(|e| e.title.as_str()).unwrap_or(&start.title));
    card.append(&title);

    let current_calendar = event.as_ref().map(|e| e.calendar.clone()).or(start.calendar.clone()).unwrap_or_else(window::default_calendar);
    let calendar = widgets::dropdown(&writable, &current_calendar);
    if calendar.selected() == gtk::INVALID_LIST_POSITION {
        calendar.set_selected(0);
    }
    // Moving an event between calendars isn't offered yet.
    calendar.set_sensitive(event.is_none());
    card.append(&labeled("Calendar", &calendar));

    let all_day_switch = gtk::Switch::new();
    all_day_switch.set_active(all_day);
    all_day_switch.set_halign(gtk::Align::Start);
    all_day_switch.set_hexpand(false);
    card.append(&labeled("All day", &all_day_switch));

    let start_time = time_entry(from_time);
    let end_time = time_entry(to_time);
    let end_date = Rc::new(DateField::new(to_date, |_| {}));
    let start_date = {
        let end_date = end_date.clone();
        let length = to_date - from_date;
        DateField::new(from_date, move |day| {
            // Keep the length when the start moves.
            end_date.set(day + length);
        })
    };
    let starts = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    starts.append(&start_date.button);
    starts.append(&start_time);
    card.append(&labeled("Starts", &starts));
    let ends = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ends.append(&end_date.button);
    ends.append(&end_time);
    card.append(&labeled("Ends", &ends));
    {
        let (a, b) = (start_time.clone(), end_time.clone());
        let show = move |all_day: bool| {
            a.set_visible(!all_day);
            b.set_visible(!all_day);
        };
        show(all_day);
        all_day_switch.connect_active_notify(move |s| show(s.is_active()));
    }

    // Repeat: Never, the presets for the start date, and the rule as it is when
    // it's one the presets can't show.
    let original_rule = event.as_ref().and_then(|e| e.rule.clone()).or(start.repeat.clone());
    let parsed = original_rule.as_deref().and_then(Repeat::parse);
    let mut repeats: Vec<(String, String, Option<Repeat>)> = vec![("never".into(), "Never".into(), None)];
    for (i, preset) in Repeat::presets(from_date).into_iter().enumerate() {
        repeats.push((format!("p{i}"), preset.describe(from_date), Some(preset)));
    }
    let mut current_repeat = "never".to_string();
    if original_rule.is_some() {
        let mut base = parsed.clone();
        if let Some(b) = base.as_mut() {
            b.ends = Ends::Never;
        }
        match repeats.iter().find(|(_, _, r)| r.is_some() && *r == base) {
            Some((id, _, _)) => current_repeat = id.clone(),
            None => {
                let label = parsed.as_ref().map(|r| Repeat { ends: Ends::Never, ..r.clone() }.describe(from_date)).unwrap_or_else(|| "Custom".into());
                repeats.push(("custom".into(), label, base));
                current_repeat = "custom".into();
            }
        }
    }
    let repeat_choice = widgets::dropdown(&repeats.iter().map(|(id, l, _)| (id.clone(), l.clone())).collect::<Vec<_>>(), &current_repeat);
    card.append(&labeled("Repeat", &repeat_choice));

    let ends_options = widgets::opts(&[("never", "Forever"), ("count", "This many times"), ("until", "Until a date")]);
    let (ends_id, count, until) = match parsed.as_ref().map(|r| r.ends) {
        Some(Ends::After(n)) => ("count", n, from_date + Duration::days(90)),
        Some(Ends::On(d)) => ("until", 10, d),
        _ => ("never", 10, from_date + Duration::days(90)),
    };
    let ends_choice = widgets::dropdown(&ends_options, ends_id);
    let count_spin = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
    count_spin.set_value(count as f64);
    let until_date = DateField::new(until, |_| {});
    let ends_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    ends_row.append(&ends_choice);
    ends_row.append(&count_spin);
    ends_row.append(&until_date.button);
    let ends_line = labeled("Until", &ends_row);
    card.append(&ends_line);
    {
        let (spin, until, line, repeat) = (count_spin.clone(), until_date.button.clone(), ends_line.clone(), repeat_choice.clone());
        let update = Rc::new(move |ends: u32, repeat_index: u32| {
            line.set_visible(repeat_index != 0);
            spin.set_visible(ends == 1);
            until.set_visible(ends == 2);
        });
        update(ends_choice.selected(), repeat_choice.selected());
        let u = update.clone();
        let r = repeat.clone();
        ends_choice.connect_selected_notify(move |d| u(d.selected(), r.selected()));
        let e = ends_choice.clone();
        repeat.connect_selected_notify(move |d| update(e.selected(), d.selected()));
    }

    // Alerts: one choice, or none.
    let alert_options = |all_day: bool| -> Vec<(String, String)> {
        let mut list = vec![("none".to_string(), "None".to_string())];
        for m in if all_day { DAY_ALERTS } else { TIMED_ALERTS } {
            list.push((m.to_string(), alert_label(*m)));
        }
        list
    };
    let original_alert = event.as_ref().and_then(|e| e.alarms.first().copied());
    let alert_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let alert_choice = Rc::new(RefCell::new(widgets::dropdown(
        &alert_options(all_day),
        &original_alert.map(|m| m.to_string()).unwrap_or_else(|| "none".into()),
    )));
    alert_box.append(&*alert_choice.borrow());
    card.append(&labeled("Alert", &alert_box));
    {
        let (alert_box, alert_choice) = (alert_box.clone(), alert_choice.clone());
        all_day_switch.connect_active_notify(move |s| {
            let fresh = widgets::dropdown(&alert_options(s.is_active()), "none");
            alert_box.remove(&*alert_choice.borrow());
            alert_box.append(&fresh);
            *alert_choice.borrow_mut() = fresh;
        });
    }

    let location = gtk::Entry::new();
    location.set_placeholder_text(Some("Place or address"));
    location.set_text(event.as_ref().map(|e| e.location.as_str()).unwrap_or(""));
    card.append(&labeled("Location", &location));
    let link = gtk::Entry::new();
    link.set_placeholder_text(Some("https://…"));
    link.set_text(event.as_ref().and_then(|e| e.url.as_deref()).unwrap_or(""));
    card.append(&labeled("Link", &link));
    let notes = gtk::TextView::new();
    notes.add_css_class("editor-notes");
    notes.set_wrap_mode(gtk::WrapMode::WordChar);
    notes.buffer().set_text(event.as_ref().map(|e| e.description.as_str()).unwrap_or(""));
    let notes_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .min_content_height(90)
        .child(&notes)
        .build();
    card.append(&labeled("Notes", &notes_scroll));

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(8);
    let cancel = gtk::Button::with_label("Not now");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&save);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());

    let start_date = Rc::new(start_date);
    let until_date = Rc::new(until_date);
    let d = dialog.clone();
    let focus = title.clone();
    save.connect_clicked(move |_| {
        let all_day = all_day_switch.is_active();
        let (first, last) = (start_date.date.get(), end_date.date.get().max(start_date.date.get()));
        let span = if all_day {
            Span::AllDay { first, last }
        } else {
            let (Some(a), Some(b)) = (parse_time(&start_time.text()), parse_time(&end_time.text())) else {
                window::toast("Enter times like 2:30 PM or 14:30.");
                return;
            };
            let (s, mut e) = (first.and_time(a), last.and_time(b));
            if e < s {
                e = s + Duration::hours(1);
            }
            Span::Timed { start: s, end: e }
        };
        let repeat = repeats.get(repeat_choice.selected() as usize).and_then(|(_, _, r)| r.clone()).map(|mut r| {
            r.ends = match ends_choice.selected() {
                1 => Ends::After(count_spin.value() as u32),
                2 => Ends::On(until_date.date.get()),
                _ => Ends::Never,
            };
            r
        });
        let rule = repeat.map(|r| r.to_rule(all_day));
        let alert = {
            let choice = alert_choice.borrow();
            let list = alert_options(all_day);
            list.get(choice.selected() as usize).and_then(|(id, _)| id.parse::<i64>().ok())
        };
        let notes_text = {
            let b = notes.buffer();
            b.text(&b.start_iter(), &b.end_iter(), false).to_string()
        };
        let (new_start, new_end) = whens(span);
        let title_text = title.text().trim().to_string();
        let title_text = if title_text.is_empty() { "Untitled".to_string() } else { title_text };
        let link_text = link.text().trim().to_string();
        let mut draft = Draft::default();
        match &event {
            None => {
                draft.summary = Some(title_text);
                draft.start = Some(Some(new_start));
                draft.end = Some(Some(new_end));
                draft.repeat = rule.map(Some);
                draft.alarms = alert.map(|m| vec![m]);
                draft.location = Some(location.text().to_string()).filter(|l| !l.is_empty());
                draft.description = Some(notes_text).filter(|n| !n.trim().is_empty());
                draft.url = Some(Some(link_text)).filter(|u| u.as_deref().is_some_and(|u| !u.is_empty()));
                let Some((calendar, _)) = writable.get(calendar.selected() as usize).cloned() else { return };
                window::remember_calendar(&calendar);
                let label = draft.summary.clone().unwrap_or_default();
                actions::create(calendar, draft, move |result| match result {
                    Ok(_) => window::toast(&format!("Added “{label}”.")),
                    Err(err) => window::toast(&err),
                });
            }
            Some(original) => {
                if title_text != original.title {
                    draft.summary = Some(title_text);
                }
                if span != original.span {
                    draft.start = Some(Some(new_start));
                    draft.end = Some(Some(new_end));
                }
                if rule != original.rule && !(rule.is_none() && original.rule.is_none()) {
                    draft.repeat = Some(rule);
                }
                if alert != original.alarms.first().copied() {
                    draft.alarms = Some(alert.into_iter().collect());
                }
                if location.text() != original.location {
                    draft.location = Some(location.text().to_string());
                }
                if notes_text != original.description {
                    draft.description = Some(notes_text);
                }
                if Some(link_text.as_str()) != original.url.as_deref().or(Some("")) {
                    draft.url = Some(Some(link_text).filter(|u| !u.is_empty()));
                }
                if draft == Draft::default() {
                    d.close();
                    return;
                }
                actions::save(original, draft);
            }
        }
        d.close();
    });
    dialog.present();
    focus.grab_focus();
}

/// A new event's start for a slot the user double-clicked.
pub fn start_at(slot: draw::Slot) -> Start {
    match slot {
        draw::Slot::At(at) => Start { span: Some(Span::Timed { start: at, end: at + Duration::hours(1) }), ..Start::default() },
        draw::Slot::Day(day) => Start { span: Some(Span::AllDay { first: day, last: day }), ..Start::default() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_labels() {
        assert_eq!(alert_label(0), "At the start");
        assert_eq!(alert_label(10), "10 min before");
        assert_eq!(alert_label(60), "1 hour before");
        assert_eq!(alert_label(120), "2 hours before");
        assert_eq!(alert_label(1440), "1 day before");
        assert_eq!(alert_label(-540), "On the day at 9 AM");
    }
}
