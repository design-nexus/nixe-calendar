//! Settings as a card over the window.

use crate::settings_page;
use crate::widgets;
use crate::window;
use gtk::prelude::*;
use nyx_ui::settings_card::{self, About, Spec, Standard};

pub use nyx_ui::settings_card::fit;

const GROUPS: &[(&str, &str, &str)] = &[
    ("Week", "x-office-calendar-symbolic", "Starting view, first day, scroll position"),
    ("New events", "list-add-symbolic", "Calendar, length, alert"),
    ("Working hours", "appointment-soon-symbolic", "Days and hours, reminders in the views"),
    ("Calendars", "view-calendar-symbolic", "Turn calendars and accounts on or off"),
    ("Calendar sets", "view-list-symbolic", "Groups of calendars"),
    ("Accounts", "system-users-symbolic", "Online accounts in Settings"),
];

const SPEC: Spec = Spec {
    groups: GROUPS,
    search_text: None,
    standard: Some(Standard {
        app: "calendar",
        about: About {
            name: "Calendar",
            icon: "io.github.design_nexus.nyx.Calendar",
            version: env!("CARGO_PKG_VERSION"),
            description: "Your online and local calendars in one place.",
            licence: "MIT",
            website: "https://github.com/design-nexus/nyx-calendar",
            issues: "https://github.com/design-nexus/nyx-calendar/issues",
            credits: &[],
        },
        shortcuts: window::SHORTCUTS,
        reduce_motion: || crate::prefs::get().reduce_motion,
        set_reduce_motion: |on| crate::prefs::update(|p| p.reduce_motion = on),
    }),
};

pub fn open() {
    let (Some(overlay), Some(window)) = (window::overlay(), window::window()) else { return };
    settings_card::open(&overlay, &window, SPEC, || {
        let page = widgets::page("settings");
        settings_page::build(&page);
        (page.root.upcast(), page.body)
    });
}
