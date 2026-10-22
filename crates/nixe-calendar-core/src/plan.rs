//! Planning around events: working hours, free time, suggested times, clashes,
//! and the links an event carries (a video meeting, a place on a map). Nothing
//! here changes an event; suggestions are for the user to choose.

use crate::{Event, Span};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::{Deserialize, Serialize};

/// When the user works: the days (Monday first) and the hours.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkingHours {
    pub days: [bool; 7],
    /// Minutes after midnight.
    pub start: u32,
    pub end: u32,
}

impl Default for WorkingHours {
    fn default() -> Self {
        Self { days: [true, true, true, true, true, false, false], start: 9 * 60, end: 17 * 60 }
    }
}

impl WorkingHours {
    pub fn works(&self, day: NaiveDate) -> bool {
        self.days[day.weekday().num_days_from_monday() as usize]
    }

    fn bounds(&self, day: NaiveDate) -> (NaiveDateTime, NaiveDateTime) {
        let at = |minutes: u32| {
            if minutes >= 24 * 60 {
                (day + Duration::days(1)).and_time(NaiveTime::MIN)
            } else {
                day.and_time(NaiveTime::from_hms_opt(minutes / 60, minutes % 60, 0).unwrap_or(NaiveTime::MIN))
            }
        };
        (at(self.start.min(self.end)), at(self.end.max(self.start)))
    }
}

/// The times events take, for the day or days asked about (cancelled and
/// all-day events don't count as busy).
pub fn busy(events: &[Event], ignore: Option<(&str, Option<&str>)>) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut out: Vec<(NaiveDateTime, NaiveDateTime)> = events
        .iter()
        .filter(|e| !e.cancelled)
        .filter(|e| ignore.is_none_or(|(uid, rid)| !(e.uid == uid && e.rid.as_deref() == rid)))
        .filter_map(|e| match e.span {
            Span::Timed { start, end } if end > start => Some((start, end)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

/// Free stretches between `from` and `to` at least `minutes` long.
pub fn free_between(
    busy: &[(NaiveDateTime, NaiveDateTime)],
    from: NaiveDateTime,
    to: NaiveDateTime,
    minutes: i64,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut free = Vec::new();
    let mut cursor = from;
    for &(start, end) in busy.iter().filter(|(s, e)| *e > from && *s < to) {
        if start.min(to) - cursor >= Duration::minutes(minutes) {
            free.push((cursor, start.min(to)));
        }
        cursor = cursor.max(end);
    }
    if to - cursor >= Duration::minutes(minutes) {
        free.push((cursor, to));
    }
    free
}

/// Free stretches on a day within working hours.
pub fn free_on(
    busy: &[(NaiveDateTime, NaiveDateTime)],
    day: NaiveDate,
    hours: &WorkingHours,
    minutes: i64,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let (from, to) = hours.bounds(day);
    free_between(busy, from, to, minutes)
}

/// The next `count` times an event `minutes` long fits, from `after`, on
/// working days within working hours, starting on the half hour; at most one
/// suggestion an hour, so they spread out. Looks `days` ahead.
pub fn suggest(
    busy: &[(NaiveDateTime, NaiveDateTime)],
    after: NaiveDateTime,
    minutes: i64,
    hours: &WorkingHours,
    count: usize,
    days: i64,
) -> Vec<NaiveDateTime> {
    let mut out: Vec<NaiveDateTime> = Vec::new();
    for offset in 0..days {
        let day = after.date() + Duration::days(offset);
        if !hours.works(day) {
            continue;
        }
        for (from, to) in free_on(busy, day, hours, minutes) {
            let mut start = round_up(from.max(after));
            while start + Duration::minutes(minutes) <= to {
                if out.last().is_none_or(|last| start - *last >= Duration::hours(1)) {
                    out.push(start);
                    if out.len() == count {
                        return out;
                    }
                }
                start += Duration::minutes(30);
            }
        }
    }
    out
}

/// Up to the next half hour.
fn round_up(t: NaiveDateTime) -> NaiveDateTime {
    let minute = t.minute();
    let base = t.with_second(0).and_then(|t| t.with_nanosecond(0)).unwrap_or(t);
    if minute == 0 && t.second() == 0 && t.nanosecond() == 0 {
        base
    } else if minute <= 30 && !(minute == 30 && t.second() > 0) {
        base.with_minute(30).unwrap_or(base)
    } else {
        base.with_minute(0).unwrap_or(base) + Duration::hours(1)
    }
}

/// Events that overlap a time (the event being edited left out).
pub fn clashes<'a>(
    events: &'a [Event],
    start: NaiveDateTime,
    end: NaiveDateTime,
    ignore: Option<(&str, Option<&str>)>,
) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|e| !e.cancelled)
        .filter(|e| ignore.is_none_or(|(uid, rid)| !(e.uid == uid && e.rid.as_deref() == rid)))
        .filter(|e| matches!(e.span, Span::Timed { start: s, end: f } if f > start && s < end))
        .collect()
}

const MEETING_HOSTS: [&str; 7] =
    ["meet.google.com", "zoom.us", "teams.microsoft.com", "teams.live.com", "meet.jit.si", "whereby.com", "webex.com"];

/// A video meeting link (https, on a known meeting host) in an event's link,
/// place or notes.
pub fn meeting_link(e: &Event) -> Option<String> {
    meeting_link_in(e.url.as_deref(), &e.location, &e.description)
}

/// As `meeting_link`, from an event's parts.
pub fn meeting_link_in(url: Option<&str>, location: &str, description: &str) -> Option<String> {
    let text = [url.unwrap_or(""), location, description].join(" ");
    text.split(|c: char| c.is_whitespace() || "<>\"'()[]".contains(c))
        .map(|w| w.trim_end_matches(['.', ',', ';']))
        .find(|w| is_meeting_link(w))
        .map(str::to_string)
}

pub fn is_meeting_link(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    if url.len() > 2048 || url.chars().any(|c| c.is_control()) {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    !host.contains('@') && !host.contains(':') && MEETING_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// A link that shows a place on OpenStreetMap; `None` for a meeting link or
/// nothing like a place.
pub fn map_link(location: &str) -> Option<String> {
    let place = location.trim();
    if place.is_empty() || place.contains("://") || place.len() > 300 {
        return None;
    }
    let query: String = place
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    Some(format!("https://www.openstreetmap.org/search?query={query}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn event(uid: &str, start: NaiveDateTime, end: NaiveDateTime) -> Event {
        Event {
            calendar: "c".into(),
            uid: uid.into(),
            rid: None,
            title: uid.into(),
            location: String::new(),
            span: Span::Timed { start, end },
            repeats: false,
            cancelled: false,
            description: String::new(),
            url: None,
            rule: None,
            alarms: Vec::new(),
            organizer: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn free_time_and_suggestions() {
        // Wednesday 2026-10-21: busy 9:00–10:30 and 13:00–14:00.
        let events = vec![event("a", t(21, 9, 0), t(21, 10, 30)), event("b", t(21, 13, 0), t(21, 14, 0))];
        let busy = busy(&events, None);
        let hours = WorkingHours::default();
        let free = free_on(&busy, t(21, 0, 0).date(), &hours, 30);
        assert_eq!(free, vec![(t(21, 10, 30), t(21, 13, 0)), (t(21, 14, 0), t(21, 17, 0))]);
        // An hour-long meeting from 8:10: 10:30, then an hour apart.
        let s = suggest(&busy, t(21, 8, 10), 60, &hours, 4, 7);
        assert_eq!(s, vec![t(21, 10, 30), t(21, 11, 30), t(21, 14, 0), t(21, 15, 0)]);
        // Friday 16:20, two hours: Monday morning (no weekend).
        let s = suggest(&[], t(23, 16, 20), 120, &hours, 1, 7);
        assert_eq!(s, vec![t(26, 9, 0)]);
        // The event being moved doesn't block itself.
        assert_eq!(super::busy(&events, Some(("a", None))).len(), 1);
    }

    #[test]
    fn clashes_and_rounding() {
        let events = vec![event("a", t(21, 9, 0), t(21, 10, 0)), event("b", t(21, 10, 0), t(21, 11, 0))];
        assert_eq!(clashes(&events, t(21, 9, 30), t(21, 10, 15), None).len(), 2);
        assert!(clashes(&events, t(21, 11, 0), t(21, 12, 0), None).is_empty(), "ending as the other starts isn't a clash");
        assert_eq!(clashes(&events, t(21, 9, 30), t(21, 9, 45), Some(("a", None))).len(), 0);
        assert_eq!(round_up(t(21, 9, 0)), t(21, 9, 0));
        assert_eq!(round_up(t(21, 9, 1)), t(21, 9, 30));
        assert_eq!(round_up(t(21, 9, 31)), t(21, 10, 0));
    }

    #[test]
    fn links() {
        let mut e = event("a", t(21, 9, 0), t(21, 10, 0));
        e.description = "Join: <https://meet.google.com/abc-defg-hij>. Or call.".into();
        assert_eq!(meeting_link(&e).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        e.description = "https://acme.zoom.us/j/123?pwd=x".into();
        assert!(meeting_link(&e).is_some());
        for bad in [
            "http://meet.google.com/x",
            "https://meet.google.com.evil.example/",
            "https://evil.example/meet.google.com",
            "https://u@zoom.us/",
        ] {
            assert!(!is_meeting_link(bad), "{bad}");
        }
        assert_eq!(
            map_link("Café Ole, 12 Main St").as_deref(),
            Some("https://www.openstreetmap.org/search?query=Caf%C3%A9+Ole%2C+12+Main+St")
        );
        assert_eq!(map_link("https://zoom.us/j/1"), None);
        assert_eq!(map_link("  "), None);
    }
}
