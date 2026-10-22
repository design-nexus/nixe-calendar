//! The settings card's groups.

use crate::prefs;
use crate::theme;
use crate::widgets::{self, Page};
use crate::window;
use gtk::prelude::*;

pub fn build(page: &Page) {
    window_group(page);
    week(page);
    working(page);
    sets(page);
    accounts(page);
    keyboard(page);
}

fn window_group(page: &Page) {
    let group = page.group("Calendar window");
    group.add(&nixe_ui::widgets::start_row());
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
