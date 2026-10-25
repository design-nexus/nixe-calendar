//! `agenda.sqlite`: the shown calendars' events from yesterday to 60 days out,
//! for Glimpse and other readers that can't reach EDS. Rewritten whole and
//! swapped in atomically; readers open it read-only.
//!
//! Schema (`user_version` 1) is a contract recorded in the suite manifest:
//!
//! ```sql
//! events(calendar, calendar_name, color, uid, rid, title, location, description, url,
//!        starts INTEGER, ends INTEGER, all_day INTEGER, first_day TEXT, last_day TEXT)
//! meta(key PRIMARY KEY, value)   -- written_at (Unix seconds), utc_offset (local
//!                                  seconds east of UTC then; for sandboxed readers)
//! ```
//!
//! `starts`/`ends` are Unix seconds (an all-day event runs from local midnight of
//! its first day to local midnight after its last); `first_day`/`last_day` are
//! local dates as YYYY-MM-DD.

use chrono::Duration;
use nixe_calendar_core::{CalendarInfo, Event, Span, midnight, timestamp};
use rusqlite::{Connection, params};
use std::path::Path;

pub const DAYS_BEFORE: i64 = 1;
pub const DAYS_AFTER: i64 = 60;

/// Writes `events` to `path` (through a temporary file and a rename).
pub fn write(path: &Path, calendars: &[CalendarInfo], events: &[Event], now: i64) -> rusqlite::Result<()> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("sqlite.tmp");
    let _ = std::fs::remove_file(&tmp);
    {
        let mut db = Connection::open(&tmp)?;
        db.execute_batch(
            "PRAGMA user_version = 1;
             CREATE TABLE events (
               calendar TEXT NOT NULL, calendar_name TEXT NOT NULL, color TEXT,
               uid TEXT NOT NULL, rid TEXT, title TEXT NOT NULL, location TEXT NOT NULL,
               description TEXT NOT NULL, url TEXT,
               starts INTEGER NOT NULL, ends INTEGER NOT NULL, all_day INTEGER NOT NULL,
               first_day TEXT NOT NULL, last_day TEXT NOT NULL);
             CREATE INDEX events_starts ON events(starts);
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let tx = db.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO events (calendar, calendar_name, color, uid, rid, title, location, description, url,
                                     starts, ends, all_day, first_day, last_day)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?;
            for e in events.iter().filter(|e| !e.cancelled) {
                let cal = calendars.iter().find(|c| c.uid == e.calendar);
                let (starts, ends, all_day) = match e.span {
                    Span::AllDay { first, last } => (midnight(first), midnight(last + Duration::days(1)), true),
                    Span::Timed { start, end } => (timestamp(start), timestamp(end), false),
                };
                let (first, last) = e.days();
                insert.execute(params![
                    e.calendar,
                    cal.map(|c| c.name.as_str()).unwrap_or(""),
                    cal.and_then(|c| c.color.as_deref()),
                    e.uid,
                    e.rid,
                    e.title,
                    e.location,
                    e.description,
                    e.url,
                    starts,
                    ends,
                    all_day,
                    first.to_string(),
                    last.to_string(),
                ])?;
            }
            tx.execute("INSERT INTO meta (key, value) VALUES ('written_at', ?1)", params![now.to_string()])?;
            let offset = chrono::Local::now().offset().local_minus_utc();
            tx.execute("INSERT INTO meta (key, value) VALUES ('utc_offset', ?1)", params![offset.to_string()])?;
        }
        tx.commit()?;
    }
    std::fs::rename(&tmp, path).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn event(title: &str, span: Span, cancelled: bool) -> Event {
        Event {
            calendar: "c1".into(),
            uid: title.into(),
            rid: None,
            title: title.into(),
            location: "Here".into(),
            span,
            repeats: false,
            cancelled,
            description: String::new(),
            url: None,
            rule: None,
            alarms: Vec::new(),
            organizer: None,
            attendees: Vec::new(),
        }
    }

    #[test]
    fn writes_events_and_skips_cancelled() {
        let dir = std::env::temp_dir().join(format!("nixe-calendard-test-{}", std::process::id()));
        let path = dir.join("agenda.sqlite");
        let d = NaiveDate::from_ymd_opt(2026, 10, 22).unwrap();
        let calendars = vec![CalendarInfo {
            uid: "c1".into(),
            name: "Work".into(),
            color: Some("#7aa2f7".into()),
            selected: true,
            group: None,
            account: None,
            needs_sign_in: false,
            offline: false,
            read_only: false,
            no_following: false,
        }];
        let events = vec![
            event(
                "Standup",
                Span::Timed { start: d.and_hms_opt(9, 0, 0).unwrap(), end: d.and_hms_opt(9, 15, 0).unwrap() },
                false,
            ),
            event("Holiday", Span::AllDay { first: d, last: d }, false),
            event("Gone", Span::AllDay { first: d, last: d }, true),
        ];
        write(&path, &calendars, &events, 42).unwrap();
        // Writing again replaces the file rather than appending.
        write(&path, &calendars, &events, 43).unwrap();
        let db = Connection::open(&path).unwrap();
        let count: i64 = db.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2);
        let (name, all_day, first): (String, bool, String) = db
            .query_row("SELECT calendar_name, all_day, first_day FROM events WHERE title = 'Holiday'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), all_day, first.as_str()), ("Work", true, "2026-10-22"));
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 1);
        let written: String = db.query_row("SELECT value FROM meta WHERE key = 'written_at'", [], |r| r.get(0)).unwrap();
        let offset: String = db.query_row("SELECT value FROM meta WHERE key = 'utc_offset'", [], |r| r.get(0)).unwrap();
        assert!(offset.parse::<i32>().is_ok_and(|o| o.abs() <= 14 * 3600));
        assert_eq!(written, "43");
        let _ = std::fs::remove_dir_all(dir);
    }
}
