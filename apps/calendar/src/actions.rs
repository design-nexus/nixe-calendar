//! What the user does to events: create, save, delete (with Undo), answer
//! invitations, import and export. Every change runs on the EDS worker.

use crate::widgets;
use crate::window;
use crate::worker;
use gtk::prelude::*;
use nyx_calendar_core::{Event, Response, When, whens};
use nyx_eds::{Draft, Scope};

/// Adds an event to a calendar.
pub fn create(calendar: String, draft: Draft, after: impl FnOnce(Result<String, String>) + 'static) {
    worker::ask(move || worker::with_calendar(&calendar, |cal| cal.create(&draft)), after);
}

/// Saves changes to an event. For a repeating event the user picks which
/// occurrences first.
pub fn save(event: &Event, draft: Draft) {
    let event = event.clone();
    if event.repeats {
        let following = splits(&event);
        ask_scope("Change a repeating event", "Change", following, move |scope| save_scoped(event.clone(), draft.clone(), scope));
    } else {
        save_scoped(event, draft, Scope::All);
    }
}

fn save_scoped(event: Event, mut draft: Draft, scope: Scope) {
    let title = event.title.clone();
    let (old_start, old_end) = whens(event.span);
    worker::ask(
        move || {
            worker::with_calendar(&event.calendar, |cal| {
                let rid = if scope == Scope::All { None } else { event.rid.as_deref() };
                if scope == Scope::All && event.rid.is_some() {
                    // Moving every occurrence: shift the series by how far this one moved.
                    let master = cal.get(&event.uid, None)?;
                    if let (Some(Some(new)), Some(first)) = (draft.start, master.start) {
                        draft.start = Some(Some(shifted(first, old_start, new)));
                    }
                    if let (Some(Some(new)), Some(last)) = (draft.end, master.end) {
                        draft.end = Some(Some(shifted(last, old_end, new)));
                    }
                }
                cal.modify(&event.uid, rid, scope, &draft)
            })
        },
        move |result| match result {
            Ok(()) => window::toast(&format!("Saved “{title}”.")),
            Err(err) => window::toast(&err),
        },
    );
}

/// Where a series value lands when one occurrence moved from `old` to `new`.
fn shifted(series: When, old: When, new: When) -> When {
    match (series, old, new) {
        (When::Time(s), When::Time(o), When::Time(n)) => When::Time(s + (n - o)),
        (series @ When::Date { .. }, old @ When::Date { .. }, new @ When::Date { .. }) => {
            let date = |w: When| match w {
                When::Date { year, month, day } => chrono::NaiveDate::from_ymd_opt(year, month, day),
                When::Time(_) => None,
            };
            match (date(series), date(old), date(new)) {
                (Some(s), Some(o), Some(n)) => nyx_calendar_core::date_when(s + (n - o)),
                _ => new,
            }
        }
        _ => new,
    }
}

/// Moves an event (its whole series) to another calendar, with Undo.
pub fn move_to(event: &Event, target: String) {
    let (from, uid, title) = (event.calendar.clone(), event.uid.clone(), event.title.clone());
    let name = window::calendars().iter().find(|c| c.uid == target).map(|c| c.name.clone()).unwrap_or_default();
    let (back_from, back_to, back_uid) = (target.clone(), from.clone(), uid.clone());
    worker::ask(
        move || transfer(&from, &target, &uid),
        move |result| match result {
            Ok(()) => window::toast_undo(&format!("Moved “{title}” to {name}."), move || {
                let (from, to, uid) = (back_from.clone(), back_to.clone(), back_uid.clone());
                worker::ask(
                    move || transfer(&from, &to, &uid),
                    |result| {
                        if let Err(err) = result {
                            window::toast(&err);
                        }
                    },
                );
            }),
            Err(err) => window::toast(&err),
        },
    );
}

/// Copies a series into another calendar, then removes the original (or the
/// copy again, when the original can't go).
fn transfer(from: &str, to: &str, uid: &str) -> Result<(), String> {
    let ics = worker::with_calendar(from, |cal| cal.export(uid))?;
    worker::with_calendar(to, |cal| cal.import_all(&ics))?;
    if let Err(err) = worker::with_calendar(from, |cal| cal.remove(uid, None, Scope::All)) {
        let _ = worker::with_calendar(to, |cal| cal.remove(uid, None, Scope::All));
        return Err(err);
    }
    Ok(())
}

/// Deletes an event (asking which occurrences for a repeating one), with Undo.
pub fn delete(event: &Event) {
    let event = event.clone();
    if event.repeats {
        let following = splits(&event);
        ask_scope("Delete a repeating event", "Delete", following, move |scope| delete_scoped(event.clone(), scope));
    } else {
        delete_scoped(event, Scope::All);
    }
}

fn delete_scoped(event: Event, scope: Scope) {
    let (calendar, title) = (event.calendar.clone(), event.title.clone());
    worker::ask(
        move || {
            worker::with_calendar(&event.calendar, |cal| {
                // The whole series as it was, to put back on Undo.
                let backup = cal.export(&event.uid)?;
                let rid = if scope == Scope::All { None } else { event.rid.as_deref() };
                cal.remove(&event.uid, rid, scope)?;
                Ok(backup)
            })
        },
        move |result| match result {
            Ok(backup) => window::toast_undo(&format!("Deleted “{title}”."), move || {
                let calendar = calendar.clone();
                let backup = backup.clone();
                worker::ask(
                    move || worker::with_calendar(&calendar, |cal| cal.import_all(&backup)),
                    |result| {
                        if let Err(err) = result {
                            window::toast(&err);
                        }
                    },
                );
            }),
            Err(err) => window::toast(&err),
        },
    );
}

/// Answers an invitation for the whole series.
pub fn respond(event: &Event, response: Response) {
    let event = event.clone();
    let me = window::me();
    worker::ask(
        move || worker::with_calendar(&event.calendar, |cal| cal.respond(&event.uid, None, &me, response)),
        move |result| match result {
            Ok(true) => window::toast(match response {
                Response::Accepted => "You're going.",
                Response::Tentative => "You might go.",
                Response::Declined => "You declined.",
                _ => "Answer saved.",
            }),
            Ok(false) => window::toast("You aren't on this invitation's guest list."),
            Err(err) => window::toast(&err),
        },
    );
}

/// Saves an event (with its changed occurrences and time zones) as an .ics file.
pub fn export(event: &Event) {
    let Some(parent) = window::window() else { return };
    let dialog =
        gtk::FileDialog::builder().title("Export event").initial_name(format!("{}.ics", safe_name(&event.title))).build();
    let event = event.clone();
    dialog.save(Some(&parent), gtk::gio::Cancellable::NONE, move |file| {
        let Some(path) = file.ok().and_then(|f| f.path()) else { return };
        worker::ask(
            move || worker::with_calendar(&event.calendar, |cal| cal.export(&event.uid)),
            move |result| match result.and_then(|ics| std::fs::write(&path, ics).map_err(|e| e.to_string())) {
                Ok(()) => window::toast("Exported."),
                Err(err) => window::toast(&err),
            },
        );
    });
}

fn safe_name(title: &str) -> String {
    let name: String = title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let name = name.trim();
    if name.is_empty() { "event".into() } else { name.chars().take(60).collect() }
}

/// Picks an .ics file and imports it.
pub fn import_dialog() {
    let Some(parent) = window::window() else { return };
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Calendar files"));
    filter.add_mime_type("text/calendar");
    filter.add_suffix("ics");
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder().title("Import events").filters(&filters).build();
    dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, |file| {
        if let Some(path) = file.ok().and_then(|f| f.path()) {
            import_file(&path);
        }
    });
}

/// Imports an .ics file into a calendar the user picks.
pub fn import_file(path: &std::path::Path) {
    let ics = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => return window::toast(&format!("Couldn't read {}: {err}", path.display())),
    };
    let writable: Vec<(String, String)> =
        window::calendars().into_iter().filter(|c| !c.read_only).map(|c| (c.uid, c.name)).collect();
    if writable.is_empty() {
        return window::toast("There's no calendar to add events to.");
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let (dialog, card) = widgets::dialog("Import events", 460);
    let note = widgets::label(&format!("Add the events in {name} to:"), "dim");
    note.set_xalign(0.0);
    note.set_wrap(true);
    card.append(&note);
    let choice = widgets::dropdown(&writable, &window::default_calendar());
    card.append(&choice);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Not now");
    let import = gtk::Button::with_label("Import");
    import.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&import);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let d = dialog.clone();
    import.connect_clicked(move |_| {
        let Some((calendar, _)) = writable.get(choice.selected() as usize).cloned() else { return };
        let ics = ics.clone();
        d.close();
        worker::ask(
            move || worker::with_calendar(&calendar, |cal| cal.import_all(&ics)),
            |result| match result {
                Ok(uids) if uids.len() == 1 => window::toast("Imported 1 event."),
                Ok(uids) => window::toast(&format!("Imported {} events.", uids.len())),
                Err(err) => window::toast(&err),
            },
        );
    });
    dialog.present();
}

/// Whether the event's calendar can change "this and following" occurrences.
fn splits(event: &Event) -> bool {
    !window::calendars().iter().any(|c| c.uid == event.calendar && c.no_following)
}

/// "This event / This and following / All events" for a repeating event
/// (without the middle one where the calendar can't do it).
fn ask_scope(title: &str, verb: &str, following: bool, then: impl Fn(Scope) + 'static) {
    let (dialog, card) = widgets::dialog(title, 480);
    let question = if following {
        format!("{verb} only this occurrence, this one and the ones after it, or every occurrence?")
    } else {
        format!("{verb} only this occurrence, or every occurrence?")
    };
    let note = widgets::label(&question, "dim");
    note.set_xalign(0.0);
    note.set_wrap(true);
    card.append(&note);
    let buttons = gtk::FlowBox::new();
    buttons.set_selection_mode(gtk::SelectionMode::None);
    buttons.set_halign(gtk::Align::End);
    buttons.set_max_children_per_line(4);
    let then = std::rc::Rc::new(then);
    let cancel = gtk::Button::with_label("Not now");
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    buttons.append(&cancel);
    for (label, scope) in
        [("This event", Scope::This), ("This and following", Scope::ThisAndFollowing), ("All events", Scope::All)]
    {
        if scope == Scope::ThisAndFollowing && !following {
            continue;
        }
        let button = gtk::Button::with_label(label);
        if scope == Scope::This {
            button.add_css_class("suggested-action");
        }
        let (d, then) = (dialog.clone(), then.clone());
        button.connect_clicked(move |_| {
            d.close();
            then(scope);
        });
        buttons.append(&button);
    }
    card.append(&buttons);
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_a_series() {
        assert_eq!(shifted(When::Time(1000), When::Time(5000), When::Time(5600)), When::Time(1600));
        let d = |day| When::Date { year: 2026, month: 10, day };
        assert_eq!(shifted(d(1), d(15), d(17)), d(3));
        assert_eq!(safe_name("Lunch / Sam?"), "Lunch _ Sam_");
        assert_eq!(safe_name("///"), "___");
        assert_eq!(safe_name("  "), "event");
    }
}
