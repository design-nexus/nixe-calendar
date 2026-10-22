//! Glimpse plugin for Calendar. It reads `agenda.sqlite` (written by
//! nixe-calendard; Glimpse mounts it read-only into the sandbox) and never
//! reaches the calendars themselves.
//!
//! `today`, `tomorrow`, `calendar` (the next 7 days) and `next meeting` list
//! events; other words search titles, places and notes. A result opens Calendar
//! on its day through the host; an event about to start with a meeting link
//! also offers to join it:
//!
//! `{"api_version": 1, "app": "calendar", "op": "open", "day": "YYYY-MM-DD"}`
//! `{"api_version": 1, "app": "calendar", "op": "join", "url": "https://…"}`
//!
//! The sandbox has no time zone, so local times use the offset calendard
//! recorded (`meta.utc_offset`).

use anyhow::{Context, Result, bail};
use glimpse_plugin_sdk::{Action, Hit, Request, serve};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use std::path::PathBuf;

fn main() {
    if let Err(error) = serve(&["search", "preview"], answer) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn database() -> PathBuf {
    if let Some(path) = std::env::var_os("NIXE_CALENDAR_DB") {
        return PathBuf::from(path);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    data.unwrap_or_else(|| PathBuf::from(".local/share")).join("nixe-calendar/agenda.sqlite")
}

fn open() -> Result<Connection> {
    let path = database();
    if !path.exists() {
        bail!("Calendar hasn't written an agenda yet.");
    }
    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .context("the agenda couldn't be read")
}

#[derive(Clone, Debug, PartialEq)]
struct Event {
    uid: String,
    rid: Option<String>,
    title: String,
    calendar: String,
    location: String,
    description: String,
    url: Option<String>,
    starts: i64,
    ends: i64,
    all_day: bool,
    first_day: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Asked {
    Day(i64),
    Week,
    Next,
    Search(String),
}

fn asked(query: &str) -> Option<Asked> {
    let q = query.trim().to_lowercase();
    match q.as_str() {
        "today" | "agenda" | "calendar today" => Some(Asked::Day(0)),
        "tomorrow" | "calendar tomorrow" => Some(Asked::Day(1)),
        "calendar" | "this week" | "week" | "upcoming" => Some(Asked::Week),
        "next" | "next meeting" | "next event" => Some(Asked::Next),
        _ => {
            let words = q.strip_prefix("calendar ").unwrap_or(&q).trim();
            (words.chars().count() >= 3).then(|| Asked::Search(words.to_string()))
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn offset(db: &Connection) -> i64 {
    db.query_row("SELECT value FROM meta WHERE key = 'utc_offset'", [], |r| r.get::<_, String>(0))
        .optional()
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Days since 1970-01-01 → (year, month, day).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn ymd(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_of(ymd: &str) -> Option<i64> {
    let mut parts = ymd.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Inverse of `civil`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// "Today", "Tomorrow", "Tue", or "Tue Oct 28", relative to `today`.
fn day_name(days: i64, today: i64) -> String {
    match days - today {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        2..=6 => WEEKDAYS[days.rem_euclid(7) as usize].into(),
        _ => {
            let (_, m, d) = civil(days);
            format!("{} {} {d}", WEEKDAYS[days.rem_euclid(7) as usize], MONTHS[(m - 1) as usize])
        }
    }
}

fn clock(ts: i64, offset: i64) -> String {
    let secs = (ts + offset).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

fn when(e: &Event, offset: i64, today: i64) -> String {
    let day = days_of(&e.first_day).unwrap_or(today);
    if e.all_day {
        return format!("{} · all day", day_name(day, today));
    }
    format!("{} {}–{}", day_name(day, today), clock(e.starts, offset), clock(e.ends, offset))
}

/// A video meeting link in the event, if any (https only).
fn meeting_link(e: &Event) -> Option<String> {
    const HOSTS: [&str; 6] =
        ["meet.google.com/", "zoom.us/j/", "zoom.us/my/", "teams.microsoft.com/l/meetup-join", "meet.jit.si/", "whereby.com/"];
    let text = [e.url.as_deref().unwrap_or(""), &e.location, &e.description].join(" ");
    text.split(|c: char| c.is_whitespace() || "<>\"'()[]".contains(c))
        .map(|w| w.trim_end_matches(['.', ',', ';']))
        .find(|w| w.starts_with("https://") && HOSTS.iter().any(|h| w.contains(h)) && w.len() < 2048)
        .map(str::to_string)
}

fn events(db: &Connection, from: i64, to: i64, words: Option<&str>, limit: usize) -> Result<Vec<Event>> {
    let pattern = words.map(|w| format!("%{}%", w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let mut stmt = db.prepare(
        "SELECT uid, rid, title, calendar_name, location, description, url, starts, ends, all_day, first_day FROM events
         WHERE ends > ?1 AND starts < ?2
           AND (?3 IS NULL OR title LIKE ?3 ESCAPE '\\' OR location LIKE ?3 ESCAPE '\\' OR description LIKE ?3 ESCAPE '\\')
         ORDER BY all_day DESC, starts LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![from, to, pattern, limit as i64], |r| {
        Ok(Event {
            uid: r.get(0)?,
            rid: r.get(1)?,
            title: r.get(2)?,
            calendar: r.get(3)?,
            location: r.get(4)?,
            description: r.get(5)?,
            url: r.get(6)?,
            starts: r.get(7)?,
            ends: r.get(8)?,
            all_day: r.get::<_, i64>(9)? != 0,
            first_day: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn id(e: &Event) -> String {
    format!("{}\u{1f}{}", e.uid, e.rid.as_deref().unwrap_or(""))
}

fn answer(request: &Request) -> Result<Value> {
    match request.op.as_str() {
        "search" => {
            let query = request.args["query"].as_str().unwrap_or("");
            let limit = request.args["limit"].as_i64().unwrap_or(10).clamp(1, 10) as usize;
            let Some(what) = asked(query) else { return Ok(json!([])) };
            let db = open()?;
            Ok(serde_json::to_value(hits(&db, what, now(), limit)?)?)
        }
        "preview" => {
            let wanted = request.args["result_id"].as_str().unwrap_or("");
            let wanted = wanted.strip_prefix("join:").unwrap_or(wanted);
            let db = open()?;
            let offset = offset(&db);
            let today = (now() + offset).div_euclid(86_400);
            let found = events(&db, i64::MIN, i64::MAX, None, 5000)?.into_iter().find(|e| id(e) == wanted);
            let Some(e) = found else { bail!("that event is no longer in the agenda") };
            let mut lines = vec![e.title.clone(), when(&e, offset, today), e.calendar.clone()];
            if !e.location.is_empty() {
                lines.push(e.location.clone());
            }
            if !e.description.is_empty() {
                lines.push(String::new());
                lines.push(e.description.chars().take(1500).collect());
            }
            Ok(json!({ "text": lines.join("\n") }))
        }
        other => bail!("unsupported operation {other}"),
    }
}

fn hits(db: &Connection, what: Asked, now: i64, limit: usize) -> Result<Vec<Hit>> {
    let offset = offset(db);
    let today = (now + offset).div_euclid(86_400);
    let midnight = |days: i64| days * 86_400 - offset;
    let found = match what {
        Asked::Day(n) => events(db, midnight(today + n), midnight(today + n + 1), None, limit)?,
        Asked::Week => events(db, now, midnight(today + 7), None, limit)?,
        Asked::Next => events(db, now, i64::MAX, None, 20)?.into_iter().filter(|e| !e.all_day).take(1).collect(),
        Asked::Search(words) => events(db, midnight(today - 1), i64::MAX, Some(&words), limit)?,
    };
    let mut out = Vec::new();
    for e in &found {
        // Starting within 15 minutes, or on now: offer to join.
        if !e.all_day
            && e.starts - now <= 15 * 60
            && e.ends > now
            && let Some(url) = meeting_link(e)
        {
            out.push(Hit {
                id: format!("join:{}", id(e)),
                title: format!("Join {}", e.title),
                subtitle: format!("{} · {}", when(e, offset, today), e.calendar),
                action: Action::Activate {
                    token: json!({"api_version": 1, "app": "calendar", "op": "join", "url": url}).to_string(),
                },
            });
        }
        let mut subtitle = format!("{} · {}", when(e, offset, today), e.calendar);
        if !e.location.is_empty() {
            subtitle.push_str(&format!(" · {}", e.location));
        }
        out.push(Hit {
            id: id(e),
            title: if e.title.trim().is_empty() { "(no title)".into() } else { e.title.clone() },
            subtitle,
            action: Action::Activate {
                token: json!({"api_version": 1, "app": "calendar", "op": "open", "day": e.first_day}).to_string(),
            },
        });
    }
    out.truncate(limit);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_asked_and_dates() {
        assert_eq!(asked("Today"), Some(Asked::Day(0)));
        assert_eq!(asked("calendar"), Some(Asked::Week));
        assert_eq!(asked("next meeting"), Some(Asked::Next));
        assert_eq!(asked("calendar dentist"), Some(Asked::Search("dentist".into())));
        assert_eq!(asked("de"), None);
        for days in [-1000, 0, 20_382, 20_747, 30_000] {
            assert_eq!(days_of(&ymd(days)), Some(days));
        }
        assert_eq!(ymd(20_382), "2025-10-21");
        assert_eq!(day_name(20_382, 20_382), "Today");
        assert_eq!(day_name(20_385, 20_382), "Fri");
        assert_eq!(day_name(20_392, 20_382), "Fri Oct 31");
        assert_eq!(clock(1_792_591_200, -4 * 3600), "10:00");
    }

    #[test]
    fn meeting_links() {
        let mut e = Event {
            uid: "u".into(),
            rid: None,
            title: "Standup".into(),
            calendar: "Work".into(),
            location: "Room 4".into(),
            description: "Join: <https://meet.google.com/abc-defg-hij>. Or call.".into(),
            url: None,
            starts: 0,
            ends: 1,
            all_day: false,
            first_day: "1970-01-01".into(),
        };
        assert_eq!(meeting_link(&e).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        e.description = "http://zoom.us/j/1 and https://example.com/zoom.us/j/".into();
        assert_eq!(
            meeting_link(&e).as_deref(),
            Some("https://example.com/zoom.us/j/"),
            "https only; the host checks the host name"
        );
        e.description = "no link".into();
        assert_eq!(meeting_link(&e), None);
    }

    #[test]
    fn agenda_queries() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE events (calendar TEXT, calendar_name TEXT, color TEXT, uid TEXT, rid TEXT, title TEXT, location TEXT,
               description TEXT, url TEXT, starts INTEGER, ends INTEGER, all_day INTEGER, first_day TEXT, last_day TEXT);
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO meta VALUES ('utc_offset', '-14400');",
        )
        .unwrap();
        // "Now" is 2026-10-21 09:50 local (UTC-4).
        let now = days_of("2026-10-21").unwrap() * 86_400 + 4 * 3600 + 9 * 3600 + 50 * 60;
        let at = |day: &str, h: i64| days_of(day).unwrap() * 86_400 + 4 * 3600 + h * 3600;
        let add = |uid: &str, title: &str, day: &str, h: i64, all_day: bool, desc: &str| {
            db.execute(
                "INSERT INTO events VALUES ('c', 'Work', NULL, ?1, NULL, ?2, '', ?3, NULL, ?4, ?5, ?6, ?7, ?7)",
                params![uid, title, desc, at(day, h), at(day, h + 1), all_day as i64, day],
            )
            .unwrap();
        };
        add("a", "Standup", "2026-10-21", 10, false, "https://meet.google.com/abc-defg-hij");
        add("b", "Dentist", "2026-10-22", 14, false, "");
        add("c", "Holiday", "2026-10-21", 0, true, "");
        let today = hits(&db, Asked::Day(0), now, 10).unwrap();
        let titles: Vec<&str> = today.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(titles, ["Holiday", "Join Standup", "Standup"]);
        assert_eq!(today[2].subtitle, "Today 10:00–11:00 · Work");
        assert_eq!(today[0].subtitle, "Today · all day · Work");
        let tomorrow = hits(&db, Asked::Day(1), now, 10).unwrap();
        assert_eq!(tomorrow[0].subtitle, "Tomorrow 14:00–15:00 · Work");
        let Action::Activate { token } = &tomorrow[0].action else { panic!() };
        assert!(token.contains("\"day\":\"2026-10-22\""));
        assert_eq!(hits(&db, Asked::Search("dent".into()), now, 10).unwrap().len(), 1);
        assert_eq!(hits(&db, Asked::Next, now, 10).unwrap().last().unwrap().title, "Standup");
    }
}
