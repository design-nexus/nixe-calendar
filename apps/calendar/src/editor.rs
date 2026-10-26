//! The event editor: a dialog for a new event or changes to one.

use crate::actions;
use crate::draw;
use crate::widgets;
use crate::window;
use crate::worker;
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
        if let Ok(dt) =
            glib::DateTime::from_local(date.get().year(), date.get().month() as i32, date.get().day() as i32, 0, 0, 0.0)
        {
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

/// "30 minutes", "1 hour", "1 h 30 min".
pub fn duration_text(minutes: i64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} minutes"),
        (1, 0) => "1 hour".into(),
        (h, 0) => format!("{h} hours"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// An event's time zone, when it isn't this computer's; `None` means local time.
type Zone = Rc<Cell<Option<chrono_tz::Tz>>>;

/// A local time as the clock reads in `zone`.
fn to_zone(local: NaiveDateTime, zone: Option<chrono_tz::Tz>) -> NaiveDateTime {
    use chrono::TimeZone;
    match zone {
        Some(tz) => Local.from_local_datetime(&local).earliest().map(|t| t.with_timezone(&tz).naive_local()).unwrap_or(local),
        None => local,
    }
}

/// A clock time in `zone` as local time.
fn from_zone(wall: NaiveDateTime, zone: Option<chrono_tz::Tz>) -> NaiveDateTime {
    use chrono::TimeZone;
    match zone {
        Some(tz) => tz.from_local_datetime(&wall).earliest().map(|t| t.with_timezone(&Local).naive_local()).unwrap_or(wall),
        None => wall,
    }
}

/// The IANA zone of an event's start, from its iCalendar text: a TZID that
/// names one, or ends in one (EDS writes `/freeassociation.sourceforge.net/…/Europe/Paris`).
fn zone_in_ics(ics: &str) -> Option<chrono_tz::Tz> {
    let line = ics.lines().find(|l| l.starts_with("DTSTART;"))?;
    let tzid = line.split([';', ':']).find_map(|p| p.strip_prefix("TZID="))?.trim_matches('"');
    let parts: Vec<&str> = tzid.split('/').filter(|p| !p.is_empty()).collect();
    (0..parts.len()).find_map(|i| parts[i..].join("/").parse().ok())
}

fn labeled(text: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("editor-row");
    let label = widgets::label(text, "editor-label");
    label.set_xalign(0.0);
    label.set_width_chars(9);
    row.append(&label);
    widget.set_hexpand(true);
    widget.as_ref().update_relation(&[gtk::accessible::Relation::LabelledBy(&[label.upcast_ref()])]);
    row.append(widget);
    row
}

/// Opens the editor for `event`, or for a new event when `event` is `None`.
pub fn open(event: Option<&Event>, start: Option<Start>) {
    let event = event.cloned();
    let start = start.unwrap_or_default();
    let now = Local::now().naive_local();
    let default_start =
        NaiveDateTime::new(now.date(), NaiveTime::from_hms_opt((now.hour() + 1).min(23), 0, 0).unwrap_or_default());
    let span = event
        .as_ref()
        .map(|e| e.span)
        .or(start.span)
        .unwrap_or(Span::Timed { start: default_start, end: default_start + new_length() });
    let (mut from_date, mut to_date, from_time, to_time, all_day) = match span {
        Span::AllDay { first, last } => (
            first,
            last,
            NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default(),
            NaiveTime::from_hms_opt(10, 0, 0).unwrap_or_default(),
            true,
        ),
        Span::Timed { start, end } => (start.date(), end.date(), start.time(), end.time(), false),
    };
    if to_date < from_date {
        to_date = from_date;
    }
    if from_date > to_date {
        from_date = to_date;
    }

    let writable: Vec<(String, String)> =
        window::calendars().into_iter().filter(|c| !c.read_only).map(|c| (c.uid, c.name)).collect();
    if writable.is_empty() {
        window::toast("There's no calendar to add events to.");
        return;
    }

    let (dialog, card) = widgets::dialog(if event.is_some() { "Edit event" } else { "New event" }, 560);
    card.add_css_class("event-editor");

    let title = gtk::Entry::new();
    title.set_placeholder_text(Some("Title"));
    title.update_property(&[gtk::accessible::Property::Label("Title")]);
    title.add_css_class("editor-title");
    title.set_text(event.as_ref().map(|e| e.title.as_str()).unwrap_or(&start.title));
    card.append(&title);

    let current_calendar =
        event.as_ref().map(|e| e.calendar.clone()).or(start.calendar.clone()).unwrap_or_else(window::default_calendar);
    let calendar = widgets::dropdown(&writable, &current_calendar);
    if calendar.selected() == gtk::INVALID_LIST_POSITION {
        calendar.set_selected(0);
    }
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

    // The times above are as the clock reads in this zone: this computer's, or
    // the event's own (read from it below), or one the user picks.
    let zone: Zone = Rc::new(Cell::new(None));
    let local_zone = nixe_eds::local_zone();
    let zone_names: Vec<&str> = chrono_tz::TZ_VARIANTS.iter().map(|z| z.name()).collect();
    let zone_choice = gtk::DropDown::new(
        Some(gtk::StringList::new(&zone_names)),
        Some(gtk::PropertyExpression::new(gtk::StringObject::static_type(), None::<gtk::Expression>, "string")),
    );
    zone_choice.set_enable_search(true);
    zone_choice.set_tooltip_text(Some("The time zone the times are in (type to search)"));
    let position_of = move |name: &str| chrono_tz::TZ_VARIANTS.iter().position(|z| z.name() == name).map(|p| p as u32);
    zone_choice.set_selected(position_of(&local_zone).unwrap_or(gtk::INVALID_LIST_POSITION));
    {
        let (zone, local_zone) = (zone.clone(), local_zone.clone());
        zone_choice.connect_selected_notify(move |d| {
            let picked = chrono_tz::TZ_VARIANTS.get(d.selected() as usize).copied();
            zone.set(picked.filter(|tz| tz.name() != local_zone));
        });
    }
    let zone_row = labeled("Time zone", &zone_choice);
    card.append(&zone_row);
    if let Some(e) = event.as_ref().filter(|e| matches!(e.span, Span::Timed { .. })) {
        // Shows an event kept in another zone in that zone's time.
        let (calendar, uid, rid) = (e.calendar.clone(), e.uid.clone(), e.rid.clone());
        let (zc, sd, ed, st, et) =
            (zone_choice.clone(), start_date.date.clone(), end_date.clone(), start_time.clone(), end_time.clone());
        let start_button = start_date.button.clone();
        let (span, local_zone) = (e.span, local_zone.clone());
        worker::ask(
            move || worker::with_calendar(&calendar, |c| c.ics(&uid, rid.as_deref())).ok().and_then(|ics| zone_in_ics(&ics)),
            move |found| {
                let (Some(tz), Span::Timed { start, end }) = (found, span) else { return };
                if tz.name() == local_zone {
                    return;
                }
                if let Some(p) = position_of(tz.name()) {
                    zc.set_selected(p);
                }
                let (s, e) = (to_zone(start, Some(tz)), to_zone(end, Some(tz)));
                sd.set(s.date());
                start_button.set_label(&s.date().format("%a, %b %-d, %Y").to_string());
                ed.set(e.date());
                st.set_text(&draw::time_text(s.time()));
                et.set_text(&draw::time_text(e.time()));
            },
        );
    }
    {
        let (a, b, z) = (start_time.clone(), end_time.clone(), zone_row.clone());
        let show = move |all_day: bool| {
            a.set_visible(!all_day);
            b.set_visible(!all_day);
            z.set_visible(!all_day);
        };
        show(all_day);
        all_day_switch.connect_active_notify(move |s| show(s.is_active()));
    }

    // The scheduling assistant: what this time clashes with, and free times.
    let clash = widgets::label("", "editor-clash");
    clash.set_xalign(0.0);
    clash.set_wrap(true);
    clash.set_visible(false);
    card.append(&clash);
    let find = gtk::MenuButton::new();
    find.set_icon_name("appointment-soon-symbolic");
    find.set_tooltip_text(Some("Find a time: free times this long, within working hours"));
    find.update_property(&[gtk::accessible::Property::Label("Find a time")]);
    find.set_has_frame(false);
    starts.append(&find);
    let ignore = event.as_ref().map(|e| (e.uid.clone(), e.rid.clone()));
    // The times entered, in local time.
    let timed = {
        let (sd, ed, st, et, sw) =
            (start_date.date.clone(), end_date.date.clone(), start_time.clone(), end_time.clone(), all_day_switch.clone());
        let zone = zone.clone();
        Rc::new(move || -> Option<(NaiveDateTime, NaiveDateTime)> {
            if sw.is_active() {
                return None;
            }
            let (a, b) = (parse_time(&st.text())?, parse_time(&et.text())?);
            let (s, e) = (sd.get().and_time(a), ed.get().max(sd.get()).and_time(b));
            let (s, e) = (from_zone(s, zone.get()), from_zone(e, zone.get()));
            (e > s).then_some((s, e))
        })
    };
    let check = {
        let (timed, clash, ignore) = (timed.clone(), clash.clone(), ignore.clone());
        let asked = Rc::new(Cell::new(0u64));
        Rc::new(move || {
            let n = asked.get() + 1;
            asked.set(n);
            let Some((s, e)) = timed() else {
                clash.set_visible(false);
                return;
            };
            let (clash, ignore, asked) = (clash.clone(), ignore.clone(), asked.clone());
            worker::ask(
                move || worker::events_between(s.date(), e.date() + Duration::days(1)),
                move |events| {
                    if asked.get() != n {
                        return;
                    }
                    let skip = ignore.as_ref().map(|(u, r)| (u.as_str(), r.as_deref()));
                    let hits = nixe_calendar_core::plan::clashes(&events, s, e, skip);
                    let names: Vec<String> = hits.iter().take(3).map(|e| format!("“{}”", e.title)).collect();
                    clash.set_visible(!hits.is_empty());
                    clash.set_text(&match hits.len() {
                        0 => String::new(),
                        n if n > 3 => {
                            format!("Overlaps {} and {} more. Find a time suggests free ones.", names.join(", "), n - 3)
                        }
                        _ => format!("Overlaps {}. Find a time suggests free ones.", names.join(" and ")),
                    });
                },
            );
        })
    };
    for entry in [&start_time, &end_time] {
        let c = check.clone();
        entry.connect_changed(move |_| c());
    }
    {
        let c = check.clone();
        all_day_switch.connect_active_notify(move |_| c());
        let c = check.clone();
        zone_choice.connect_selected_notify(move |_| c());
        for button in [&start_date.button, &end_date.button] {
            let c = check.clone();
            button.connect_label_notify(move |_| c());
        }
    }
    check();
    {
        let (timed, ignore, zone) = (timed.clone(), ignore.clone(), zone.clone());
        let (sd, ed, st, et) = (start_date.date.clone(), end_date.clone(), start_time.clone(), end_time.clone());
        let start_button = start_date.button.clone();
        // People to find a time with: their busy times come from the chosen
        // calendar's server, when it shares them.
        let people: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let (calendar_choice, calendars) = (calendar.clone(), writable.clone());
        find.set_create_popup_func(move |button| {
            let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
            list.add_css_class("menu-list");
            let popover = gtk::Popover::new();
            popover.set_child(Some(&list));
            button.set_popover(Some(&popover));
            let minutes = timed().map(|(s, e)| (e - s).num_minutes()).unwrap_or(60).clamp(5, 12 * 60);
            let with = gtk::Entry::new();
            with.set_placeholder_text(Some("With: sam@example.com, …"));
            with.update_property(&[gtk::accessible::Property::Label("Find a time with these people")]);
            with.set_tooltip_text(Some("Their busy times too (Enter), when the calendar's server shares them"));
            with.set_text(&people.borrow());
            {
                let (people, button) = (people.clone(), button.clone());
                with.connect_activate(move |e| {
                    *people.borrow_mut() = e.text().trim().to_string();
                    button.popdown();
                    button.popup();
                });
            }
            list.append(&with);
            list.append(&widgets::label(&format!("Free for {}", duration_text(minutes)), "dim"));
            let now = Local::now().naive_local();
            let wanted: Vec<String> = people
                .borrow()
                .split([',', ' ', ';'])
                .map(|p| p.trim().trim_start_matches("mailto:").to_string())
                .filter(|p| p.contains('@'))
                .collect();
            let server = calendars.get(calendar_choice.selected() as usize).map(|(uid, _)| uid.clone()).unwrap_or_default();
            let after = sd.get().and_time(NaiveTime::MIN).max(now);
            let hours = crate::prefs::get().working_hours;
            let ignore = ignore.clone();
            let (sd, ed, st, et, start_button, popover, zone) =
                (sd.clone(), ed.clone(), st.clone(), et.clone(), start_button.clone(), popover.clone(), zone.clone());
            let until = after + Duration::days(14);
            worker::ask(
                move || {
                    let events = worker::events_between(after.date(), until.date());
                    let theirs = if wanted.is_empty() {
                        Ok(Vec::new())
                    } else {
                        let stamp = |t: NaiveDateTime| nixe_calendar_core::timestamp(t);
                        worker::with_calendar(&server, |c| c.free_busy(stamp(after), stamp(until), &wanted))
                    };
                    (events, theirs, wanted)
                },
                move |(events, theirs, wanted)| {
                    let skip = ignore.as_ref().map(|(u, r)| (u.as_str(), r.as_deref()));
                    let mut busy = nixe_calendar_core::plan::busy(&events, skip);
                    match theirs {
                        Ok(found) => {
                            let local = |secs: i64| {
                                chrono::DateTime::from_timestamp(secs, 0).map(|t| t.with_timezone(&Local).naive_local())
                            };
                            for (_, times) in &found {
                                busy.extend(times.iter().filter_map(|(a, b)| Some((local(*a)?, local(*b)?))));
                            }
                            let missing: Vec<&str> = wanted
                                .iter()
                                .filter(|w| !found.iter().any(|(who, _)| who.eq_ignore_ascii_case(w)))
                                .map(String::as_str)
                                .collect();
                            if !missing.is_empty() {
                                let note = widgets::label(&format!("No free/busy times for {}.", missing.join(", ")), "dim");
                                note.set_wrap(true);
                                note.set_max_width_chars(40);
                                list.append(&note);
                            }
                        }
                        Err(err) => {
                            let note = widgets::label(&format!("Couldn't get their busy times: {err}"), "dim");
                            note.set_wrap(true);
                            note.set_max_width_chars(40);
                            list.append(&note);
                        }
                    }
                    busy.sort();
                    let times = nixe_calendar_core::plan::suggest(&busy, after, minutes, &hours, 8, 14);
                    if times.is_empty() {
                        list.append(&widgets::label("Nothing free in working hours in the next two weeks.", "dim"));
                    }
                    for t in times {
                        let end = t + Duration::minutes(minutes);
                        let label =
                            format!("{} · {}–{}", t.format("%a %b %-d"), draw::time_text(t.time()), draw::time_text(end.time()));
                        let item = gtk::Button::with_label(&label);
                        item.add_css_class("flat");
                        item.add_css_class("menu-item");
                        let (sd, ed, st, et, sb, p) =
                            (sd.clone(), ed.clone(), st.clone(), et.clone(), start_button.clone(), popover.clone());
                        let zone = zone.clone();
                        item.connect_clicked(move |_| {
                            let (t, end) = (to_zone(t, zone.get()), to_zone(end, zone.get()));
                            sd.set(t.date());
                            sb.set_label(&t.date().format("%a, %b %-d, %Y").to_string());
                            ed.set(end.date());
                            st.set_text(&draw::time_text(t.time()));
                            et.set_text(&draw::time_text(end.time()));
                            p.popdown();
                        });
                        list.append(&item);
                    }
                },
            );
        });
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
                let label = parsed
                    .as_ref()
                    .map(|r| Repeat { ends: Ends::Never, ..r.clone() }.describe(from_date))
                    .unwrap_or_else(|| "Custom".into());
                repeats.push(("custom".into(), label, base));
                current_repeat = "custom".into();
            }
        }
    }
    let repeat_choice =
        widgets::dropdown(&repeats.iter().map(|(id, l, _)| (id.clone(), l.clone())).collect::<Vec<_>>(), &current_repeat);
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
        let (spin, until, line, repeat) =
            (count_spin.clone(), until_date.button.clone(), ends_line.clone(), repeat_choice.clone());
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
    // A new timed event starts with Settings' alert.
    let original_alert = match &event {
        Some(e) => e.alarms.first().copied(),
        None if !all_day => crate::prefs::get().default_alert,
        None => None,
    };
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
            let (s, mut e) = (from_zone(first.and_time(a), zone.get()), from_zone(last.and_time(b), zone.get()));
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
                draft.zone = zone.get().map(|tz| tz.name().to_string());
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
                    draft.zone = zone.get().map(|tz| tz.name().to_string());
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
                // Moving runs first on the worker, so the changes are saved where the event went.
                let mut original = original.clone();
                if let Some((target, _)) = writable.get(calendar.selected() as usize).filter(|(uid, _)| *uid != original.calendar)
                {
                    actions::move_to(&original, target.clone());
                    original.calendar = target.clone();
                }
                if draft != Draft::default() {
                    actions::save(&original, draft);
                }
            }
        }
        d.close();
    });
    dialog.present();
    focus.grab_focus();
}

/// How long a new event lasts (Settings → New events).
pub fn new_length() -> Duration {
    Duration::minutes(crate::prefs::get().event_minutes.clamp(5, 24 * 60))
}

/// A new event's start for a slot the user double-clicked.
pub fn start_at(slot: draw::Slot) -> Start {
    match slot {
        draw::Slot::At(at) => Start { span: Some(Span::Timed { start: at, end: at + new_length() }), ..Start::default() },
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

#[cfg(test)]
mod zone_tests {
    use super::*;

    #[test]
    fn reads_event_zones() {
        let ics = "BEGIN:VEVENT\nUID:a\nDTSTART;TZID=/freeassociation.sourceforge.net/Tzfile/Asia/Tokyo:20261030T090000\nEND:VEVENT";
        assert_eq!(zone_in_ics(ics), Some(chrono_tz::Asia::Tokyo));
        assert_eq!(zone_in_ics("DTSTART;VALUE=DATE-TIME;TZID=\"Europe/Paris\":20261030T090000"), Some(chrono_tz::Europe::Paris));
        assert_eq!(zone_in_ics("DTSTART:20261030T090000Z"), None);
        assert_eq!(zone_in_ics("DTSTART;TZID=Pacific Standard Time:20261030T090000"), None);
    }

    #[test]
    fn zone_times_round_trip() {
        let wall = NaiveDate::from_ymd_opt(2026, 10, 30).unwrap().and_hms_opt(9, 0, 0).unwrap();
        let tz = Some(chrono_tz::Asia::Tokyo);
        assert_eq!(to_zone(from_zone(wall, tz), tz), wall);
        assert_eq!(from_zone(wall, None), wall);
    }
}
