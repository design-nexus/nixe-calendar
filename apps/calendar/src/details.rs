//! The popover for one event: when, where, who, and what can be done with it.

use crate::actions;
use crate::draw;
use crate::editor;
use crate::widgets;
use crate::window;
use gtk::prelude::*;
use nixe_calendar_core::{Event, Repeat, Response};

const GUESTS_SHOWN: usize = 6;

pub fn show(parent: &gtk::Widget, rect: gtk::gdk::Rectangle, event: Event) {
    let calendars = window::calendars();
    let calendar = calendars.iter().find(|c| c.uid == event.calendar);
    let read_only = calendar.is_none_or(|c| c.read_only);

    let popover = gtk::Popover::new();
    popover.add_css_class("event-popover");
    let card = widgets::vbox(8);
    card.add_css_class("event-details");

    let title = widgets::label(&event.title, "event-title");
    title.set_wrap(true);
    title.set_xalign(0.0);
    title.set_max_width_chars(36);
    card.append(&title);

    let line = |text: &str, icon: &str| {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let image = gtk::Image::from_icon_name(icon);
        image.add_css_class("dim");
        image.set_valign(gtk::Align::Start);
        row.append(&image);
        let label = widgets::label(text, "");
        label.set_wrap(true);
        label.set_xalign(0.0);
        label.set_max_width_chars(36);
        label.set_selectable(true);
        row.append(&label);
        row
    };
    card.append(&line(&draw::when_text(event.span), "x-office-calendar-symbolic"));
    if let Some(rule) = &event.rule {
        let (first, _) = event.days();
        let text = Repeat::parse(rule).map(|r| r.describe(first)).unwrap_or_else(|| "Repeats".into());
        card.append(&line(&text, "media-playlist-repeat-symbolic"));
    } else if event.repeats {
        card.append(&line("Changed occurrence of a repeating event", "media-playlist-repeat-symbolic"));
    }
    if event.cancelled {
        card.append(&line("Cancelled", "dialog-warning-symbolic"));
    }
    if !event.location.is_empty() {
        card.append(&line(&event.location, "mark-location-symbolic"));
    }
    if !event.alarms.is_empty() {
        let text = event.alarms.iter().map(|m| editor::alert_label(*m)).collect::<Vec<_>>().join(", ");
        card.append(&line(&text, "alarm-symbolic"));
    }
    if let Some(organizer) = &event.organizer
        && !event.attendees.is_empty()
    {
        card.append(&line(&format!("Organized by {}", organizer.label()), "system-users-symbolic"));
        let mut guests: Vec<String> = event
            .attendees
            .iter()
            .filter(|a| !a.email.eq_ignore_ascii_case(&organizer.email))
            .map(|a| {
                let answer = match a.response {
                    Response::Accepted => " ✓",
                    Response::Declined => " ✗",
                    Response::Tentative => " ?",
                    _ => "",
                };
                format!("{}{answer}", a.label())
            })
            .collect();
        let more = guests.len().saturating_sub(GUESTS_SHOWN);
        guests.truncate(GUESTS_SHOWN);
        if more > 0 {
            guests.push(format!("and {more} more"));
        }
        if !guests.is_empty() {
            let label = widgets::label(&guests.join(", "), "dim");
            label.set_wrap(true);
            label.set_xalign(0.0);
            label.set_max_width_chars(40);
            card.append(&label);
        }
    }
    if let Some(calendar) = calendar {
        card.append(&line(&calendar.name, "view-list-symbolic"));
    }
    if !event.description.trim().is_empty() {
        let notes = widgets::label(event.description.trim(), "event-notes");
        notes.set_wrap(true);
        notes.set_xalign(0.0);
        notes.set_max_width_chars(40);
        notes.set_lines(8);
        notes.set_ellipsize(gtk::pango::EllipsizeMode::End);
        notes.set_selectable(true);
        card.append(&notes);
    }
    if let Some(url) = event.url.clone().filter(|u| u.starts_with("https://") || u.starts_with("http://")) {
        let link = gtk::LinkButton::with_label(&url, &url);
        link.set_halign(gtk::Align::Start);
        if let Some(label) = link.child().and_downcast::<gtk::Label>() {
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_max_width_chars(36);
        }
        card.append(&link);
    }

    // Answer an invitation.
    let me = window::me();
    if let Some(mine) = event.invitation(&me)
        && !read_only
    {
        let current = match mine.response {
            Response::Accepted => "yes",
            Response::Tentative => "maybe",
            Response::Declined => "no",
            _ => "",
        };
        let e = event.clone();
        let p = popover.clone();
        let answers = widgets::segmented(
            &widgets::opts(&[("yes", "Going"), ("maybe", "Maybe"), ("no", "Not going")]),
            current,
            move |answer| {
                let response = match answer.as_str() {
                    "yes" => Response::Accepted,
                    "maybe" => Response::Tentative,
                    _ => Response::Declined,
                };
                actions::respond(&e, response);
                p.popdown();
            },
        );
        card.append(&answers);
    }

    let tools = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    tools.set_halign(gtk::Align::End);
    tools.set_margin_top(4);
    let export = widgets::icon_button("document-save-as-symbolic", "Export as .ics");
    {
        let (e, p) = (event.clone(), popover.clone());
        export.connect_clicked(move |_| {
            p.popdown();
            actions::export(&e);
        });
    }
    tools.append(&export);
    if !read_only {
        let edit = widgets::icon_button("document-edit-symbolic", "Edit (E)");
        let (e, p) = (event.clone(), popover.clone());
        edit.connect_clicked(move |_| {
            p.popdown();
            editor::open(Some(&e), None);
        });
        tools.append(&edit);
        let delete = widgets::icon_button("user-trash-symbolic", "Delete (Delete)");
        delete.add_css_class("destructive");
        let (e, p) = (event.clone(), popover.clone());
        delete.connect_clicked(move |_| {
            p.popdown();
            actions::delete(&e);
        });
        tools.append(&delete);
    }
    card.append(&tools);

    // E and Delete act on the event while the popover is open.
    let keys = gtk::EventControllerKey::new();
    let (e, p) = (event.clone(), popover.clone());
    keys.connect_key_pressed(move |_, key, _, _| {
        match key {
            gtk::gdk::Key::e if !read_only => {
                p.popdown();
                editor::open(Some(&e), None);
            }
            gtk::gdk::Key::Delete if !read_only => {
                p.popdown();
                actions::delete(&e);
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    popover.add_controller(keys);

    popover.set_child(Some(&card));
    popover.set_parent(parent);
    popover.set_pointing_to(Some(&rect));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}
