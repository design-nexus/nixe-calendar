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
