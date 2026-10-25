//! Right-click menus: events, empty time in the views, calendars and sets in
//! the sidebar. Each item does what the event popover, its keys or the
//! sidebar already do.

use crate::draw::Slot;
use crate::editor::{self, Start};
use crate::widgets::{self, MenuEntry, danger, item, keyed, separator, sub};
use crate::{actions, details, prefs, quick, window, worker};
use chrono::Local;
use nixe_calendar_core::{CalendarInfo, Event};
use nixe_eds::Response;

fn point(x: f64, y: f64) -> gtk::gdk::Rectangle {
    gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)
}

/// An event in any view.
pub fn event(widget: &gtk::Widget, x: f64, y: f64, event: Event) {
    // A reminder in the overlay belongs to Reminders.
    if event.calendar.starts_with("task:") {
        let (w, e) = (widget.clone(), event.clone());
        widgets::popup_menu(widget, x, y, vec![item("Open in Reminders", move || details::show(&w, point(x, y), e.clone()))]);
        return;
    }
    let calendars = window::calendars();
    let calendar = calendars.iter().find(|c| c.uid == event.calendar);
    let read_only = calendar.is_none_or(|c| c.read_only);
    let mut out: Vec<MenuEntry> = Vec::new();
    let (w, e) = (widget.clone(), event.clone());
    out.push(item("Open", move || details::show(&w, point(x, y), e.clone())));
    if !read_only {
        let e = event.clone();
        out.push(keyed("Edit…", "E", move || editor::open(Some(&e), None)));
        let e = event.clone();
        out.push(item("Duplicate…", move || {
            let start = Start { title: e.title.clone(), span: Some(e.span), calendar: Some(e.calendar.clone()), repeat: None };
            editor::open(None, Some(start));
        }));
    }
    if let Some(link) = nixe_calendar_core::plan::meeting_link(&event) {
        out.push(keyed("Join the video call", "J", move || window::open_link(&link)));
    }
    if let Some(link) = nixe_calendar_core::plan::map_link(&event.location) {
        out.push(item("Show on a map", move || window::open_link(&link)));
    }
    if !read_only && event.invitation(&window::me()).is_some() {
        let answers = [("Going", Response::Accepted), ("Maybe", Response::Tentative), ("Not going", Response::Declined)];
        out.push(sub(
            "Answer",
            answers
                .into_iter()
                .map(|(label, response)| {
                    let e = event.clone();
                    item(label, move || actions::respond(&e, response))
                })
                .collect(),
        ));
    }
    out.push(separator());
    let e = event.clone();
    out.push(item("Export as .ics…", move || actions::export(&e)));
    if let Some(cal) = calendar {
        let uid = cal.uid.clone();
        out.push(item(&format!("Hide {}", cal.name), move || worker::set_selected(uid.clone(), false)));
    }
    if !read_only {
        out.push(separator());
        let e = event.clone();
        out.push(danger("Delete", Some("Delete"), move || actions::delete(&e)));
    }
    widgets::popup_menu(widget, x, y, out);
}

/// Empty time (or a day) in a view.
pub fn empty(widget: &gtk::Widget, x: f64, y: f64, slot: Slot) {
    let day = match slot {
        Slot::At(at) => at.date(),
        Slot::Day(day) => day,
    };
    let w = widget.clone();
    let mut out = vec![
        item("New event here…", move || editor::open(None, Some(editor::start_at(slot)))),
        item("Quick add here…", move || quick::show(&w, point(x, y), Some(slot))),
    ];
    out.push(item(&format!("Show {}", day.format("%A, %B %-d")), move || {
        window::set_view(window::ViewKind::Day);
        window::go_to(day);
    }));
    if day != Local::now().date_naive() {
        out.push(separator());
        out.push(keyed("Go to today", "T", window::go_to_today));
    }
    widgets::popup_menu(widget, x, y, out);
}

/// Colors offered for a calendar (EDS saves them; providers that keep colors sync them).
const COLORS: &[(&str, &str)] = &[
    ("Red", "#e5484d"),
    ("Orange", "#f76b15"),
    ("Yellow", "#ffc53d"),
    ("Green", "#30a46c"),
    ("Teal", "#12a594"),
    ("Blue", "#3e63dd"),
    ("Purple", "#8e4ec6"),
    ("Pink", "#d6409f"),
    ("Gray", "#8b8d98"),
];

/// A calendar in the sidebar.
pub fn attach_calendar(button: &gtk::Button, cal: &CalendarInfo) {
    let cal = cal.clone();
    widgets::on_right_click(button, move |w, x, y| {
        let mut out: Vec<MenuEntry> = Vec::new();
        let (uid, selected) = (cal.uid.clone(), cal.selected);
        out.push(item(if selected { "Hide" } else { "Show" }, move || worker::set_selected(uid.clone(), !selected)));
        let only = cal.uid.clone();
        out.push(item("Show only this calendar", move || {
            window::show_set(Some(&prefs::CalendarSet { name: String::new(), calendars: vec![only.clone()] }));
        }));
        if !cal.read_only {
            let uid = cal.uid.clone();
            out.push(keyed("New event in this calendar…", "N", move || {
                editor::open(None, Some(Start { calendar: Some(uid.clone()), ..Start::default() }));
            }));
        }
        out.push(sub(
            "Color",
            COLORS
                .iter()
                .map(|(name, hex)| {
                    let (uid, hex) = (cal.uid.clone(), hex.to_string());
                    item(name, move || {
                        worker::set_color(uid.clone(), hex.clone(), |r| {
                            if let Err(e) = r {
                                window::toast(&e);
                            }
                        })
                    })
                })
                .collect(),
        ));
        out.push(separator());
        let (uid, name) = (cal.uid.clone(), cal.name.clone());
        out.push(item("Turn off", move || turn_off(&uid, &name)));
        widgets::popup_menu(w, x, y, out);
    });
}

/// Switches a calendar off (not loaded or synced, in every app), with Undo.
pub fn turn_off(uid: &str, name: &str) {
    let (uid, name) = (uid.to_string(), name.to_string());
    worker::set_enabled(uid.clone(), false, move |r| match r {
        Ok(()) => window::toast_undo(&format!("{name} is off. Turn it back on in Settings → Calendars."), move || {
            worker::set_enabled(uid.clone(), true, |_| {})
        }),
        Err(e) => window::toast(&e),
    });
}

/// A saved set in the sidebar.
pub fn attach_set(button: &gtk::Button, set: &prefs::CalendarSet) {
    let set = set.clone();
    widgets::on_right_click(button, move |w, x, y| {
        let (show, name) = (set.clone(), set.name.clone());
        widgets::popup_menu(
            w,
            x,
            y,
            vec![
                item("Show", move || window::show_set(Some(&show))),
                separator(),
                danger("Delete set", None, move || {
                    prefs::update(|p| p.sets.retain(|s| s.name != name));
                    window::refresh_nav();
                    window::toast(&format!("Deleted the set {name}."));
                }),
            ],
        );
    });
}
