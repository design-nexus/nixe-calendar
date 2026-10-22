//! Quick create: type "Lunch with Sam tomorrow at 1" and press Enter. The line
//! under the entry shows how it was read; other readings are offered as chips.

use crate::actions;
use crate::draw::{self, Slot};
use crate::editor;
use crate::widgets;
use crate::window;
use chrono::{Duration, Local, NaiveTime};
use gtk::prelude::*;
use nixe_calendar_core::{Repeat, Span, whens};
use nixe_eds::Draft;
use nixe_when::Parse;
use std::cell::RefCell;
use std::rc::Rc;

/// What a reading turns into.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Plan {
    title: String,
    span: Span,
    repeat: Option<String>,
}

fn plan(text: &str, parse: &Parse, slot: Option<Slot>) -> Plan {
    let now = Local::now().naive_local();
    let fallback_day = match slot {
        Some(Slot::At(at)) => at.date(),
        Some(Slot::Day(day)) => day,
        None => now.date(),
    };
    let day = parse.date.unwrap_or(fallback_day);
    let span = match (parse.start, slot) {
        (Some(start), _) => {
            let start = day.and_time(start);
            let end = parse.ends().filter(|e| *e > start).unwrap_or(start + Duration::hours(1));
            Span::Timed { start, end }
        }
        (None, Some(Slot::At(at))) if parse.date.is_none() => {
            Span::Timed { start: at, end: at + parse.duration.unwrap_or(Duration::hours(1)) }
        }
        _ => Span::AllDay { first: day, last: day },
    };
    let title =
        if parse.title.trim().is_empty() && !parse.found() { text.trim().to_string() } else { parse.title.trim().to_string() };
    Plan { title: if title.is_empty() { "Untitled".into() } else { title }, span, repeat: parse.repeat.clone() }
}

fn describe(plan: &Plan) -> String {
    let mut text = draw::when_text(plan.span);
    if let Some(rule) = &plan.repeat {
        let first = match plan.span {
            Span::AllDay { first, .. } => first,
            Span::Timed { start, .. } => start.date(),
        };
        text.push_str(" · ");
        text.push_str(&Repeat::parse(rule).map(|r| r.describe(first)).unwrap_or_else(|| "Repeats".into()));
    }
    text
}

/// Opens quick create pointing at `rect` on `parent`. `slot` is where the user
/// double-clicked, if they did.
pub fn show(parent: &gtk::Widget, rect: gtk::gdk::Rectangle, slot: Option<Slot>) {
    let writable: Vec<(String, String)> =
        window::calendars().into_iter().filter(|c| !c.read_only).map(|c| (c.uid, c.name)).collect();
    if writable.is_empty() {
        window::toast("There's no calendar to add events to. Add an account in Settings → Online Accounts.");
        return;
    }
    let popover = gtk::Popover::new();
    let card = widgets::vbox(8);
    card.add_css_class("quick-create");
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Lunch with Sam tomorrow at 1"));
    entry.set_width_chars(34);
    card.append(&entry);
    let preview = widgets::label("", "dim");
    preview.set_xalign(0.0);
    preview.set_wrap(true);
    preview.set_max_width_chars(40);
    card.append(&preview);
    let chips = gtk::FlowBox::new();
    chips.set_selection_mode(gtk::SelectionMode::None);
    chips.set_max_children_per_line(3);
    chips.set_visible(false);
    card.append(&chips);
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let calendar = widgets::dropdown(&writable, &window::default_calendar());
    if calendar.selected() == gtk::INVALID_LIST_POSITION {
        calendar.set_selected(0);
    }
    calendar.set_hexpand(true);
    bottom.append(&calendar);
    let more = widgets::icon_button("document-edit-symbolic", "More options");
    bottom.append(&more);
    let add = widgets::icon_button("list-add-symbolic", "Add (Enter)");
    add.add_css_class("suggested-action");
    bottom.append(&add);
    card.append(&bottom);

    // The reading in use: the parse, or an alternative the user picked.
    let chosen: Rc<RefCell<Option<Plan>>> = Rc::new(RefCell::new(None));
    let reparse = {
        let (preview, chips, chosen) = (preview.clone(), chips.clone(), chosen.clone());
        move |text: &str| {
            let now = match slot {
                Some(Slot::At(at)) => at,
                Some(Slot::Day(day)) => day.and_time(NaiveTime::MIN),
                None => Local::now().naive_local(),
            };
            let parse = nixe_when::parse(text, now);
            let main = plan(text, &parse, slot);
            preview.set_text(&if text.trim().is_empty() {
                String::new()
            } else {
                format!("{} · {}", main.title, describe(&main))
            });
            *chosen.borrow_mut() = Some(main.clone());
            while let Some(child) = chips.first_child() {
                chips.remove(&child);
            }
            let others: Vec<Plan> = parse.alternatives.iter().map(|alt| plan(text, alt, slot)).filter(|p| *p != main).collect();
            chips.set_visible(!others.is_empty());
            if !others.is_empty() {
                let note = widgets::label("Or did you mean:", "dim");
                chips.append(&note);
            }
            for other in others {
                let chip = gtk::Button::with_label(&describe(&other));
                chip.add_css_class("chip");
                let (preview, chosen) = (preview.clone(), chosen.clone());
                chip.connect_clicked(move |_| {
                    preview.set_text(&format!("{} · {}", other.title, describe(&other)));
                    *chosen.borrow_mut() = Some(other.clone());
                });
                chips.append(&chip);
            }
        }
    };
    let r = reparse.clone();
    entry.connect_changed(move |e| r(&e.text()));
    reparse("");

    let save = {
        let (popover, chosen, calendar, entry) = (popover.clone(), chosen.clone(), calendar.clone(), entry.clone());
        move || {
            if entry.text().trim().is_empty() {
                return;
            }
            let Some(plan) = chosen.borrow().clone() else { return };
            let Some((uid, _)) = writable.get(calendar.selected() as usize).cloned() else { return };
            let (start, end) = whens(plan.span);
            let draft = Draft {
                summary: Some(plan.title.clone()),
                start: Some(Some(start)),
                end: Some(Some(end)),
                repeat: plan.repeat.clone().map(Some),
                ..Draft::default()
            };
            window::remember_calendar(&uid);
            popover.popdown();
            let title = plan.title.clone();
            actions::create(uid, draft, move |result| match result {
                Ok(_) => window::toast(&format!("Added “{title}”.")),
                Err(err) => window::toast(&err),
            });
        }
    };
    let s = save.clone();
    entry.connect_activate(move |_| s());
    add.connect_clicked(move |_| save());
    {
        let (popover, chosen, entry) = (popover.clone(), chosen.clone(), entry.clone());
        more.connect_clicked(move |_| {
            let plan = chosen.borrow().clone();
            popover.popdown();
            let start = match plan {
                Some(p) if !entry.text().trim().is_empty() => {
                    editor::Start { title: p.title, span: Some(p.span), repeat: p.repeat, calendar: None }
                }
                _ => slot.map(editor::start_at).unwrap_or_default(),
            };
            editor::open(None, Some(start));
        });
    }

    popover.set_child(Some(&card));
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&rect));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
    entry.grab_focus();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime};

    fn at(day: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn slot_fills_in_what_the_text_leaves_out() {
        let slot = Some(Slot::At(at(22, 14)));
        let p = nixe_when::parse("Review", at(22, 14));
        assert_eq!(plan("Review", &p, slot).span, Span::Timed { start: at(22, 14), end: at(22, 15) });
        let p = nixe_when::parse("Lunch at 1pm for 90 min", at(22, 9));
        let got = plan("Lunch at 1pm for 90 min", &p, Some(Slot::At(at(22, 9))));
        assert_eq!(got.title, "Lunch");
        assert_eq!(got.span, Span::Timed { start: at(22, 13), end: at(22, 13) + Duration::minutes(90) });
        let day = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        let p = nixe_when::parse("Hike", day.and_time(NaiveTime::MIN));
        assert_eq!(plan("Hike", &p, Some(Slot::Day(day))).span, Span::AllDay { first: day, last: day });
    }

    #[test]
    fn repeats_carry_through() {
        let p = nixe_when::parse("Standup every weekday at 9:30", at(21, 10));
        let got = plan("Standup every weekday at 9:30", &p, None);
        assert_eq!(got.title, "Standup");
        assert_eq!(got.repeat.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
    }
}
