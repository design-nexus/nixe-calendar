//! Settings as a card over the window.

use crate::settings_page;
use crate::widgets;
use crate::window;
use gtk::prelude::*;
use nixe_ui::settings_card::{self, Spec};

pub use nixe_ui::settings_card::fit;

const GROUPS: &[(&str, &str, &str)] = &[
    ("Calendar window", "applications-graphics-symbolic", "Theme, glow, motion"),
    ("Week", "x-office-calendar-symbolic", "First day, scroll position"),
    ("Working hours", "appointment-soon-symbolic", "Days and hours, reminders in the views"),
    ("Calendar sets", "view-list-symbolic", "Groups of calendars"),
    ("Accounts", "system-users-symbolic", "Online accounts in Settings"),
    ("Keyboard", "input-keyboard-symbolic", "Shortcuts"),
];

const SPEC: Spec = Spec { groups: GROUPS, search_text: None };

pub fn open() {
    let (Some(overlay), Some(window)) = (window::overlay(), window::window()) else { return };
    settings_card::open(&overlay, &window, SPEC, || {
        let page = widgets::page("settings");
        settings_page::build(&page);
        (page.root.upcast(), page.body)
    });
}
