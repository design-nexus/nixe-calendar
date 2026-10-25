//! The settings card's groups.

use crate::prefs;
use crate::theme;
use crate::widgets::{self, Page};
use crate::window;
use crate::{editor, worker};
use gtk::prelude::*;

pub fn build(page: &Page) {
    window_group(page);
    week(page);
    new_events(page);
    working(page);
    calendars(page);
    sets(page);
    accounts(page);
    keyboard(page);
}

fn window_group(page: &Page) {
    let group = page.group("Calendar window");
    group.add(&widgets::segmented_row(
        "Open on",
        "The view Calendar opens on.",
        widgets::opts(&[("day", "Day"), ("week", "Week"), ("month", "Month"), ("year", "Year"), ("agenda", "Agenda")]),
        &prefs::get().open_view,
        |view| prefs::update(|p| p.open_view = view),
    ));
    let prefs = prefs::get();
    let options: Vec<(String, String)> = theme::all().iter().map(|t| (t.id.clone(), t.name.clone())).collect();
    let (theme_row, theme_dd) =
        widgets::choice_row("Theme", "A bundled theme, or one of your own.", options, &prefs.theme, |id| {
            prefs::update(|p| {
                p.theme = id;
                p.mode = prefs::ThemeMode::Theme;
            });
            theme::apply();
        });
    theme_dd.set_sensitive(prefs.mode == prefs::ThemeMode::Theme || !theme::omarchy_available());
    if theme::omarchy_available() {
        let dd = theme_dd.clone();
        let (row, _) = widgets::switch_row(
            "Follow Omarchy theme",
            "Match the desktop's colors and update when the Omarchy theme changes.",
            prefs.mode == prefs::ThemeMode::Omarchy,
            move |on| {
                prefs::update(|p| p.mode = if on { prefs::ThemeMode::Omarchy } else { prefs::ThemeMode::Theme });
                dd.set_sensitive(!on);
                theme::apply();
            },
        );
        group.add(&row);
    }
    group.add(&theme_row);
    let (row, _) = widgets::switch_row("Glow", "Soft accent glow around focused and selected elements.", prefs.glow, |on| {
        prefs::update(|p| p.glow = on);
        theme::apply();
    });
    group.add(&row);
    let (row, _) = widgets::switch_row("Reduce motion", "Turn off transitions in this window.", prefs.reduce_motion, |on| {
        prefs::update(|p| p.reduce_motion = on);
        theme::apply();
    });
    group.add(&row);
}

fn week(page: &Page) {
    let group = page.group("Week");
    let prefs = prefs::get();
    group.add(&widgets::segmented_row(
        "First day of the week",
        "The day each week starts on.",
        widgets::opts(&[("sunday", "Sunday"), ("monday", "Monday")]),
        &prefs.week_start,
        |day| {
            prefs::update(|p| p.week_start = day);
            window::go_to_today();
        },
    ));
    let hours: Vec<(String, String)> = (0..=12).map(|h| (h.to_string(), format!("{h}:00"))).collect();
    let (row, _) =
        widgets::choice_row("Scroll to", "The hour the week opens on.", hours, &prefs.day_starts.to_string(), |hour| {
            prefs::update(|p| p.day_starts = hour.parse().unwrap_or(8))
        });
    group.add(&row);
    let (row, _) = widgets::switch_row(
        "Show declined invitations",
        "Off: invitations you said no to leave the views.",
        prefs.show_declined,
        |on| {
            prefs::update(|p| p.show_declined = on);
            window::redraw();
        },
    );
    group.add(&row);
}

fn new_events(page: &Page) {
    let group = page.group("New events");
    let prefs = prefs::get();
    let mut where_to = vec![(String::new(), "The last one used".to_string())];
    where_to.extend(
        window::calendars()
            .into_iter()
            .filter(|c| !c.read_only)
            .map(|c| (c.uid.clone(), format!("{} · {}", c.name, c.group_label()))),
    );
    let (row, _) = widgets::choice_row(
        "Add to",
        "The calendar new events go to; you can pick another in the editor.",
        where_to,
        &prefs.new_events_calendar,
        |uid| prefs::update(|p| p.new_events_calendar = uid),
    );
    group.add(&row);
    let lengths: Vec<(String, String)> =
        [15, 30, 45, 60, 90, 120].iter().map(|m| (m.to_string(), editor::duration_text(*m))).collect();
    let (row, _) = widgets::choice_row(
        "Length",
        "How long a new event lasts unless you say (\u{201c}lunch at 1 for 90 minutes\u{201d}).",
        lengths,
        &prefs.event_minutes.to_string(),
        |m| prefs::update(|p| p.event_minutes = m.parse().unwrap_or(60)),
    );
    group.add(&row);
    let mut alerts = vec![("none".to_string(), "None".to_string())];
    alerts.extend([0i64, 5, 10, 15, 30, 60, 1440].iter().map(|m| (m.to_string(), editor::alert_label(*m))));
    let (row, _) = widgets::choice_row(
        "Alert",
        "The alert a new timed event starts with.",
        alerts,
        &prefs.default_alert.map(|m| m.to_string()).unwrap_or_else(|| "none".into()),
        |m| prefs::update(|p| p.default_alert = m.parse().ok()),
    );
    group.add(&row);
}

/// Calendars switched on or off, by account (off: not loaded or synced, in
/// every app; switch on again here).
fn calendars(page: &Page) {
    let group = page.group("Calendars");
    group.note("Turn calendars off to stop loading and syncing them, here and in other calendar apps.");
    let list = group.list.clone();
    fill_calendars(&list);
}

fn fill_calendars(list: &gtk::Box) {
    let list = list.clone();
    worker::all_calendars(move |all| {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let mut groups: Vec<(String, Vec<(nixe_eds::Source, bool)>)> = Vec::new();
        for (source, on) in all {
            let label = source.group.clone().unwrap_or_else(|| "On this computer".into());
            match groups.iter_mut().find(|(g, _)| *g == label) {
                Some((_, items)) => items.push((source, on)),
                None => groups.push((label, vec![(source, on)])),
            }
        }
        for (label, items) in groups {
            let uids: Vec<String> = items.iter().map(|(s, _)| s.uid.clone()).collect();
            let any_on = items.iter().any(|(_, on)| *on);
            let l = list.clone();
            let (row, _) = widgets::switch_row(&label, &format!("{} calendars", items.len()), any_on, move |on| {
                let (count, done) = (uids.len(), std::rc::Rc::new(std::cell::Cell::new(0)));
                for uid in &uids {
                    let (l, done) = (l.clone(), done.clone());
                    worker::set_enabled(uid.clone(), on, move |r| {
                        if let Err(e) = r {
                            window::toast(&e);
                        }
                        done.set(done.get() + 1);
                        if done.get() == count {
                            fill_calendars(&l);
                        }
                    });
                }
            });
            row.add_css_class("account-row");
            list.append(&row);
            for (source, on) in items {
                let (uid, l) = (source.uid.clone(), list.clone());
                let (row, _) = widgets::switch_row(source.label(), "", on, move |on| {
                    let l = l.clone();
                    worker::set_enabled(uid.clone(), on, move |r| {
                        if let Err(e) = r {
                            window::toast(&e);
                        }
                        fill_calendars(&l);
                    });
                });
                row.add_css_class("nested-row");
                list.append(&row);
            }
        }
    });
}

fn working(page: &Page) {
    let group = page.group("Working hours");
    group.note("Find a time and Assistant's suggested times stay within these.");
    let hours = prefs::get().working_hours;
    let days = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
        let toggle = gtk::ToggleButton::with_label(name);
        toggle.add_css_class("segment");
        toggle.set_active(hours.days[i]);
        toggle.connect_toggled(move |t| prefs::update(|p| p.working_hours.days[i] = t.is_active()));
        days.append(&toggle);
    }
    group.add(&widgets::row("Days", "", Some(days.upcast_ref())));
    let times: Vec<(String, String)> =
        (0..=48).map(|h| ((h * 30).to_string(), format!("{:02}:{:02}", h / 2, h % 2 * 30))).collect();
    let (row, _) = widgets::choice_row("Start", "", times.clone(), &hours.start.to_string(), |v| {
        prefs::update(|p| p.working_hours.start = v.parse().unwrap_or(9 * 60))
    });
    group.add(&row);
    let (row, _) = widgets::choice_row("End", "", times, &hours.end.to_string(), |v| {
        prefs::update(|p| p.working_hours.end = v.parse().unwrap_or(17 * 60))
    });
    group.add(&row);
    let (row, _) = widgets::switch_row(
        "Show reminders",
        "Reminders with a due date, from your task lists, in the day and week views.",
        prefs::get().show_reminders,
        |on| {
            prefs::update(|p| p.show_reminders = on);
            window::refresh_reminders();
        },
    );
    group.add(&row);
}

fn sets(page: &Page) {
    let group = page.group("Calendar sets");
    group.note("Show a group of calendars together from the sidebar. Save the calendars shown now with + beside SETS.");
    let sets = prefs::get().sets;
    if sets.is_empty() {
        group.add(&widgets::row("No sets yet", "", None));
    }
    let calendars = window::calendars();
    for set in sets {
        let names: Vec<String> =
            set.calendars.iter().filter_map(|u| calendars.iter().find(|c| c.uid == *u).map(|c| c.name.clone())).collect();
        let remove = widgets::icon_button("user-trash-symbolic", &format!("Remove the set “{}” (the calendars stay)", set.name));
        let name = set.name.clone();
        let line = widgets::row(&set.name, &names.join(", "), Some(remove.upcast_ref()));
        let l = line.clone();
        remove.connect_clicked(move |_| {
            prefs::update(|p| p.sets.retain(|s| s.name != name));
            l.set_visible(false);
            window::refresh_nav();
        });
        group.add(&line);
    }
}

fn accounts(page: &Page) {
    let group = page.group("Accounts");
    group.note("Calendars come from the accounts in Settings → Online Accounts, and from this computer.");
    let open = widgets::icon_button("go-next-symbolic", "Open Settings → Online Accounts");
    open.connect_clicked(|_| window::open_online_accounts());
    group.add(&widgets::row(
        "Online accounts",
        "Add, sign in to or remove Google, Nextcloud and other calendar accounts.",
        Some(open.upcast_ref()),
    ));
}

fn keyboard(page: &Page) {
    let group = page.group("Keyboard");
    for (keys, what) in window::SHORTCUTS {
        group.add(&widgets::row(what, "", Some(widgets::key_caps(keys).upcast_ref())));
    }
}
