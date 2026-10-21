//! Calendar model and layout for Nixe Calendar. No GTK.
//!
//! Events come from Evolution Data Server through `nixe-eds`; this crate turns
//! them into local wall-clock spans and lays a week out for drawing.

mod week;

pub use week::{AllDayBar, Placed, WeekLayout, week_days, week_start};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use nixe_eds::{Item, When};

/// A calendar in the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarInfo {
    pub uid: String,
    pub name: String,
    /// `#rrggbb`, if EDS has one.
    pub color: Option<String>,
    /// Shown in the views.
    pub selected: bool,
    /// The account or collection it belongs to; `None` for this computer.
    pub group: Option<String>,
    /// The GNOME Online Accounts id, if it belongs to an online account.
    pub account: Option<String>,
    /// Its account needs the user to sign in again.
    pub needs_sign_in: bool,
    /// The backend can't reach its server; events come from the saved copy.
    pub offline: bool,
}

impl CalendarInfo {
    pub fn group_label(&self) -> &str {
        self.group.as_deref().unwrap_or("On this computer")
    }
}

/// When an event happens, in local wall-clock time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Span {
    /// Whole days, `first` to `last` inclusive.
    AllDay { first: NaiveDate, last: NaiveDate },
    Timed { start: NaiveDateTime, end: NaiveDateTime },
}

/// One occurrence of an event, ready to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub calendar: String,
    pub uid: String,
    pub rid: Option<String>,
    pub title: String,
    pub location: String,
    pub span: Span,
    pub repeats: bool,
    pub cancelled: bool,
}

impl Event {
    /// An EDS occurrence in the local time zone, or `None` without a start.
    pub fn from_item(calendar: &str, item: &Item) -> Option<Self> {
        Self::from_item_in(calendar, item, &Local)
    }

    pub fn from_item_in<Tz: TimeZone>(calendar: &str, item: &Item, tz: &Tz) -> Option<Self> {
        let span = match (item.start?, item.end) {
            (When::Date { .. }, end) => {
                let first = date_of(item.start?)?;
                // DTEND of an all-day event is exclusive.
                let last = end.and_then(date_of).map(|d| d.pred_opt().unwrap_or(d)).filter(|l| *l >= first).unwrap_or(first);
                Span::AllDay { first, last }
            }
            (When::Time(start), end) => {
                let start = local(start, tz)?;
                let end = match end {
                    Some(When::Time(end)) => local(end, tz)?,
                    Some(When::Date { .. }) | None => start,
                };
                Span::Timed { start, end: end.max(start) }
            }
        };
        Some(Self {
            calendar: calendar.to_string(),
            uid: item.uid.clone(),
            rid: item.rid.clone(),
            title: if item.summary.trim().is_empty() { "Untitled".into() } else { item.summary.clone() },
            location: item.location.clone(),
            span,
            repeats: item.repeats,
            cancelled: item.status == nixe_eds::Status::Cancelled,
        })
    }

    /// Sort key: all-day first, then by start.
    pub fn sort_key(&self) -> (u8, NaiveDateTime) {
        match self.span {
            Span::AllDay { first, .. } => (0, first.and_hms_opt(0, 0, 0).unwrap_or_default()),
            Span::Timed { start, .. } => (1, start),
        }
    }
}

fn date_of(when: When) -> Option<NaiveDate> {
    match when {
        When::Date { year, month, day } => NaiveDate::from_ymd_opt(year, month, day),
        When::Time(_) => None,
    }
}

fn local<Tz: TimeZone>(secs: i64, tz: &Tz) -> Option<NaiveDateTime> {
    Some(DateTime::from_timestamp(secs, 0)?.with_timezone(tz).naive_local())
}

/// Unix seconds for local midnight at the start of `date` (the earlier one if the
/// clock repeats an hour, the next valid time if it skips).
pub fn midnight(date: NaiveDate) -> i64 {
    midnight_in(date, &Local)
}

pub fn midnight_in<Tz: TimeZone>(date: NaiveDate, tz: &Tz) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).unwrap_or_default();
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t.timestamp(),
        chrono::LocalResult::None => tz.from_utc_datetime(&naive).timestamp(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn item(start: When, end: Option<When>) -> Item {
        Item {
            kind: nixe_eds::ItemKind::Event,
            uid: "u".into(),
            rid: None,
            summary: "Dentist".into(),
            location: String::new(),
            description: String::new(),
            start: Some(start),
            end,
            due: None,
            status: nixe_eds::Status::None,
            completed: None,
            priority: None,
            url: None,
            parent: None,
            repeats: false,
            has_alarms: false,
            attendees: 0,
        }
    }

    #[test]
    fn all_day_end_is_exclusive() {
        let e = Event::from_item("c", &item(When::Date { year: 2026, month: 12, day: 24 }, Some(When::Date { year: 2026, month: 12, day: 26 })))
            .unwrap();
        let d = |day| NaiveDate::from_ymd_opt(2026, 12, day).unwrap();
        assert_eq!(e.span, Span::AllDay { first: d(24), last: d(25) });
    }

    #[test]
    fn timed_events_are_local() {
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        // 2026-11-02 15:00 UTC is 10:00 at UTC-5.
        let e = Event::from_item_in("c", &item(When::Time(1_793_631_600), Some(When::Time(1_793_635_200))), &est).unwrap();
        let at = |h| NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(h, 0, 0).unwrap();
        assert_eq!(e.span, Span::Timed { start: at(10), end: at(11) });
    }

    #[test]
    fn missing_end_is_an_instant_and_empty_title_is_untitled() {
        let mut it = item(When::Time(1_793_631_600), None);
        it.summary = "  ".into();
        let e = Event::from_item_in("c", &it, &chrono::Utc).unwrap();
        assert!(matches!(e.span, Span::Timed { start, end } if start == end));
        assert_eq!(e.title, "Untitled");
    }

    #[test]
    fn midnight_in_fixed_zone() {
        let est = FixedOffset::west_opt(5 * 3600).unwrap();
        assert_eq!(midnight_in(NaiveDate::from_ymd_opt(2026, 11, 2).unwrap(), &est), 1_793_595_600);
    }
}
