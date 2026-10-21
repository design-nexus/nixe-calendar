//! Repeat rules for the editor: the common RRULE shapes as choices, and back.
//! A rule with parts this model doesn't know is kept as it is ("Custom").

use chrono::{Datelike, NaiveDate, Weekday};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ends {
    Never,
    /// The last day an occurrence may start.
    On(NaiveDate),
    /// This many occurrences in all.
    After(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repeat {
    pub frequency: Frequency,
    pub interval: u32,
    /// Weekly: the days (empty means the start's weekday).
    pub days: Vec<Weekday>,
    /// Monthly: the nth weekday (`2, Tue` is the second Tuesday, `-1, Fri` the
    /// last Friday) instead of the start's day of the month.
    pub nth: Option<(i32, Weekday)>,
    pub ends: Ends,
}

impl Repeat {
    pub fn every(frequency: Frequency) -> Self {
        Self { frequency, interval: 1, days: Vec::new(), nth: None, ends: Ends::Never }
    }

    /// Reads an RRULE value. `None` when it uses parts this model can't show
    /// (the editor then keeps the rule unchanged).
    pub fn parse(rule: &str) -> Option<Self> {
        let mut repeat = Repeat::every(Frequency::Daily);
        let mut freq = None;
        for part in rule.trim_start_matches("RRULE:").split(';').filter(|p| !p.is_empty()) {
            let (key, value) = part.split_once('=')?;
            match key.to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = Some(match value.to_ascii_uppercase().as_str() {
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        "YEARLY" => Frequency::Yearly,
                        _ => return None,
                    })
                }
                "INTERVAL" => repeat.interval = value.parse().ok().filter(|n| *n > 0)?,
                "COUNT" => repeat.ends = Ends::After(value.parse().ok()?),
                "UNTIL" => repeat.ends = Ends::On(NaiveDate::parse_from_str(value.get(..8)?, "%Y%m%d").ok()?),
                "BYDAY" => {
                    for day in value.split(',') {
                        let split = day.len().checked_sub(2)?;
                        let (n, name) = day.split_at(split);
                        let weekday = ical_weekday(name)?;
                        if n.is_empty() {
                            repeat.days.push(weekday);
                        } else {
                            if repeat.nth.is_some() {
                                return None;
                            }
                            repeat.nth = Some((n.parse().ok()?, weekday));
                        }
                    }
                }
                "WKST" | "BYMONTHDAY" | "BYMONTH" => {}
                _ => return None,
            }
        }
        repeat.frequency = freq?;
        let fits = match repeat.frequency {
            Frequency::Weekly => repeat.nth.is_none(),
            Frequency::Monthly => repeat.days.is_empty(),
            Frequency::Daily | Frequency::Yearly => repeat.days.is_empty() && repeat.nth.is_none(),
        };
        fits.then_some(repeat)
    }

    /// The RRULE value. `all_day` writes UNTIL as a date; otherwise as the end of
    /// that day in UTC.
    pub fn to_rule(&self, all_day: bool) -> String {
        let mut rule = format!(
            "FREQ={}",
            match self.frequency {
                Frequency::Daily => "DAILY",
                Frequency::Weekly => "WEEKLY",
                Frequency::Monthly => "MONTHLY",
                Frequency::Yearly => "YEARLY",
            }
        );
        if self.interval > 1 {
            rule.push_str(&format!(";INTERVAL={}", self.interval));
        }
        if self.frequency == Frequency::Weekly && !self.days.is_empty() {
            let mut days = self.days.clone();
            days.sort_by_key(|d| d.num_days_from_monday());
            days.dedup();
            rule.push_str(&format!(";BYDAY={}", days.iter().map(|d| ical_day(*d)).collect::<Vec<_>>().join(",")));
        }
        if self.frequency == Frequency::Monthly
            && let Some((n, day)) = self.nth
        {
            rule.push_str(&format!(";BYDAY={n}{}", ical_day(day)));
        }
        match self.ends {
            Ends::Never => {}
            Ends::After(n) => rule.push_str(&format!(";COUNT={n}")),
            Ends::On(date) if all_day => rule.push_str(&format!(";UNTIL={}", date.format("%Y%m%d"))),
            Ends::On(date) => rule.push_str(&format!(";UNTIL={}T235959Z", date.format("%Y%m%d"))),
        }
        rule
    }

    /// "Every week on Mon, Wed", "Every 2 months on the last Friday", "Every day, 5 times".
    pub fn describe(&self, start: NaiveDate) -> String {
        let n = self.interval;
        let unit = |one: &str, many: &str| if n == 1 { format!("Every {one}") } else { format!("Every {n} {many}") };
        let mut text = match self.frequency {
            Frequency::Daily => unit("day", "days"),
            Frequency::Weekly => {
                let days = if self.days.is_empty() { vec![start.weekday()] } else { self.days.clone() };
                if days.len() == 5 && WEEKDAYS.iter().all(|d| days.contains(d)) && n == 1 {
                    "Every weekday".to_string()
                } else {
                    let mut days = days;
                    days.sort_by_key(|d| d.num_days_from_monday());
                    format!("{} on {}", unit("week", "weeks"), days.iter().map(|d| short(*d)).collect::<Vec<_>>().join(", "))
                }
            }
            Frequency::Monthly => match self.nth {
                Some((nth, day)) => format!("{} on the {} {}", unit("month", "months"), ordinal_word(nth), long(day)),
                None => format!("{} on the {}", unit("month", "months"), ordinal(start.day())),
            },
            Frequency::Yearly => format!("{} on {}", unit("year", "years"), start.format("%B %-d")),
        };
        match self.ends {
            Ends::Never => {}
            Ends::After(1) => text.push_str(", once"),
            Ends::After(count) => text.push_str(&format!(", {count} times")),
            Ends::On(date) => text.push_str(&format!(", until {}", date.format("%b %-d, %Y"))),
        }
        text
    }

    /// The choices a repeat menu offers for an event starting on `start`.
    pub fn presets(start: NaiveDate) -> Vec<Repeat> {
        let weekly = Repeat { days: vec![start.weekday()], ..Repeat::every(Frequency::Weekly) };
        let weekdays = Repeat { days: WEEKDAYS.to_vec(), ..Repeat::every(Frequency::Weekly) };
        let monthly_nth = Repeat { nth: Some((nth_of_month(start), start.weekday())), ..Repeat::every(Frequency::Monthly) };
        vec![
            Repeat::every(Frequency::Daily),
            weekdays,
            weekly,
            Repeat::every(Frequency::Monthly),
            monthly_nth,
            Repeat::every(Frequency::Yearly),
        ]
    }
}

/// Which weekday of the month a date is: 1..4, or -1 for the last one.
pub fn nth_of_month(date: NaiveDate) -> i32 {
    let next_week = date + chrono::Duration::days(7);
    if next_week.month() != date.month() { -1 } else { ((date.day() - 1) / 7 + 1) as i32 }
}

const WEEKDAYS: [Weekday; 5] = [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri];

fn ical_day(d: Weekday) -> &'static str {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][d.num_days_from_monday() as usize]
}

fn ical_weekday(s: &str) -> Option<Weekday> {
    let i = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|d| d.eq_ignore_ascii_case(s))?;
    Some(Weekday::try_from(i as u8).ok()?)
}

fn short(d: Weekday) -> &'static str {
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][d.num_days_from_monday() as usize]
}

fn long(d: Weekday) -> &'static str {
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][d.num_days_from_monday() as usize]
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn ordinal_word(n: i32) -> String {
    match n {
        -1 => "last".into(),
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        4 => "fourth".into(),
        n => ordinal(n.unsigned_abs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    #[test]
    fn round_trips_common_rules() {
        for rule in [
            "FREQ=DAILY",
            "FREQ=WEEKLY;BYDAY=MO,WE,FR",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU",
            "FREQ=MONTHLY;BYDAY=-1FR",
            "FREQ=MONTHLY;COUNT=6",
            "FREQ=YEARLY;UNTIL=20301231",
        ] {
            let parsed = Repeat::parse(rule).unwrap_or_else(|| panic!("{rule}"));
            assert_eq!(parsed.to_rule(true), rule);
        }
        assert_eq!(
            Repeat::parse("FREQ=WEEKLY;UNTIL=20261231T045959Z").unwrap().to_rule(false),
            "FREQ=WEEKLY;UNTIL=20261231T235959Z"
        );
    }

    #[test]
    fn unknown_parts_are_custom() {
        assert_eq!(Repeat::parse("FREQ=HOURLY"), None);
        assert_eq!(Repeat::parse("FREQ=MONTHLY;BYDAY=1MO,3MO"), None);
        assert_eq!(Repeat::parse("FREQ=YEARLY;BYSETPOS=2"), None);
        assert_eq!(Repeat::parse("INTERVAL=2"), None);
    }

    #[test]
    fn describes_rules() {
        let start = d(10, 21); // A Wednesday, the third one of the month.
        let r = |s: &str| Repeat::parse(s).unwrap().describe(start);
        assert_eq!(r("FREQ=DAILY"), "Every day");
        assert_eq!(r("FREQ=WEEKLY"), "Every week on Wed");
        assert_eq!(r("FREQ=WEEKLY;BYDAY=FR,MO"), "Every week on Mon, Fri");
        assert_eq!(r("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"), "Every weekday");
        assert_eq!(r("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU"), "Every 2 weeks on Tue");
        assert_eq!(r("FREQ=MONTHLY"), "Every month on the 21st");
        assert_eq!(r("FREQ=MONTHLY;BYDAY=-1FR"), "Every month on the last Friday");
        assert_eq!(r("FREQ=YEARLY;COUNT=5"), "Every year on October 21, 5 times");
        assert_eq!(r("FREQ=DAILY;UNTIL=20261101"), "Every day, until Nov 1, 2026");
    }

    #[test]
    fn presets_follow_the_start() {
        let presets = Repeat::presets(d(10, 21));
        assert_eq!(presets[2].to_rule(false), "FREQ=WEEKLY;BYDAY=WE");
        assert_eq!(presets[4].to_rule(false), "FREQ=MONTHLY;BYDAY=3WE");
        assert_eq!(nth_of_month(d(10, 30)), -1);
        assert_eq!(nth_of_month(d(10, 1)), 1);
    }
}
