//! This app's own preferences (`~/.config/nixe-calendar/settings.toml`).
//! Which calendars are shown is EDS's, shared with other calendar apps.

pub use nixe_theme::ThemeMode;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub mode: ThemeMode,
    pub theme: String,
    pub reduce_motion: bool,
    pub sidebar_collapsed: bool,
    pub glow: bool,
    pub last_section: String,
    /// `sunday` or `monday`.
    pub week_start: String,
    /// The hour the week view scrolls to when it opens.
    pub day_starts: u32,
    /// `day`, `week`, `month` or `agenda`.
    pub view: String,
    /// The calendar new events go to.
    pub last_calendar: String,
    /// When the user works (suggested times, free time).
    pub working_hours: nixe_calendar_core::plan::WorkingHours,
    /// Named groups of calendars to show together.
    pub sets: Vec<CalendarSet>,
    /// Show reminders with a due date (from the task lists) in the views.
    pub show_reminders: bool,
    /// How long a new event lasts, in minutes.
    pub event_minutes: i64,
    /// The alert a new timed event gets (minutes before; none when empty).
    pub default_alert: Option<i64>,
    /// The calendar new events go to; empty: the last one used.
    pub new_events_calendar: String,
    /// Show invitations the user said no to.
    pub show_declined: bool,
    /// The view Calendar opens on: `day`, `week`, `month`, `year` or `agenda`.
    pub open_view: String,
}

/// Calendars shown together ("Work", "Family").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarSet {
    pub name: String,
    /// Calendar (EDS source) uids.
    pub calendars: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Omarchy,
            theme: "tokyo-night".into(),
            reduce_motion: false,
            sidebar_collapsed: false,
            glow: true,
            last_section: "week".into(),
            week_start: default_week_start().into(),
            day_starts: 8,
            view: "week".into(),
            last_calendar: String::new(),
            working_hours: Default::default(),
            sets: Vec::new(),
            show_reminders: false,
            event_minutes: 60,
            default_alert: None,
            new_events_calendar: String::new(),
            show_declined: true,
            open_view: "week".into(),
        }
    }
}

/// The region of the time locale ("US" in en_US.UTF-8).
fn region() -> String {
    let locale =
        ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
    locale.split(['.', '@']).next().unwrap_or("").rsplit('_').next().unwrap_or("").to_string()
}

/// Sunday where the locale's weeks start on Sunday (US, Canada, Japan, ...), else Monday.
fn default_week_start() -> &'static str {
    let sunday = ["US", "CA", "JP", "MX", "BR", "IL", "PH", "KR", "TW", "HK", "IN", "ZA"];
    if sunday.contains(&region().as_str()) { "sunday" } else { "monday" }
}

/// Times as "2 PM" where the locale uses a 12-hour clock, else "14:00".
pub fn twelve_hour() -> bool {
    thread_local!(static TWELVE: bool = ["US", "CA", "AU", "NZ", "IN", "PH", "PK", "EG", "SA"].contains(&region().as_str()));
    TWELVE.with(|t| *t)
}

pub fn first_weekday() -> chrono::Weekday {
    if get().week_start == "monday" { chrono::Weekday::Mon } else { chrono::Weekday::Sun }
}

nixe_core::prefs_store!(Prefs);
