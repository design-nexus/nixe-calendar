//! The month grid and the agenda list.

use crate::week::{AllDayBar, pack, week_start};
use crate::{Event, Span};
use chrono::{Datelike, Duration, NaiveDate, Weekday};

/// The weeks (rows of seven days) that show `month`'s days: four to six rows.
pub fn month_weeks(any_day: NaiveDate, first: Weekday) -> Vec<[NaiveDate; 7]> {
    let first_of_month = any_day.with_day(1).unwrap_or(any_day);
    let next_month = first_of_month.checked_add_months(chrono::Months::new(1)).unwrap_or(first_of_month);
    let mut start = week_start(first_of_month, first);
    let mut rows = Vec::new();
    while start < next_month {
        rows.push(std::array::from_fn(|i| start + Duration::days(i as i64)));
        start += Duration::days(7);
    }
    rows
}

/// Every event as a bar in its week row: multi-day and all-day ones first,
/// then timed ones by start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MonthLayout {
    pub rows: Vec<[NaiveDate; 7]>,
    pub bars: Vec<Vec<AllDayBar>>,
}

impl MonthLayout {
    pub fn new(rows: Vec<[NaiveDate; 7]>, events: &[Event]) -> Self {
        let mut order: Vec<usize> = (0..events.len()).collect();
        order.sort_by_key(|i| {
            let e = &events[*i];
            let (first, last) = e.days();
            let timed_start = match e.span {
                Span::Timed { start, .. } if first == last => Some(start),
                _ => None,
            };
            (first, timed_start.is_some(), std::cmp::Reverse(last - first), timed_start, e.title.clone())
        });
        let spans: Vec<(usize, NaiveDate, NaiveDate)> = order
            .into_iter()
            .map(|i| {
                let (first, last) = events[i].days();
                (i, first, last)
            })
            .collect();
        let bars = rows.iter().map(|row| pack(row, &spans)).collect();
        Self { rows, bars }
    }

    /// How many lanes a row uses.
    pub fn lanes(&self, row: usize) -> u32 {
        self.bars.get(row).map(|b| b.iter().map(|x| x.lane + 1).max().unwrap_or(0)).unwrap_or(0)
    }
}

/// One day of the agenda: the indices of the events on it, all-day first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agenda {
    pub day: NaiveDate,
    pub events: Vec<usize>,
}

/// The days from `from` for `days` days that have events. A multi-day event is
/// listed on each of its days.
pub fn agenda(events: &[Event], from: NaiveDate, days: i64) -> Vec<Agenda> {
    let mut out = Vec::new();
    for offset in 0..days {
        let day = from + Duration::days(offset);
        let mut on: Vec<usize> = (0..events.len())
            .filter(|i| {
                let (first, last) = events[*i].days();
                first <= day && day <= last
            })
            .collect();
        if on.is_empty() {
            continue;
        }
        on.sort_by_key(|i| events[*i].sort_key());
        out.push(Agenda { day, events: on });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    fn event(title: &str, span: Span) -> Event {
        Event {
            calendar: "c".into(),
            uid: title.into(),
            rid: None,
            title: title.into(),
            location: String::new(),
            span,
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

    fn at(m: u32, day: u32, h: u32) -> chrono::NaiveDateTime {
        d(m, day).and_hms_opt(h, 0, 0).unwrap()
    }

    #[test]
    fn month_rows_cover_the_month() {
        // October 2026 starts on a Thursday and has 31 days.
        let rows = month_weeks(d(10, 21), Weekday::Sun);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0][0], d(9, 27));
        assert_eq!(rows[4][6], d(10, 31));
        let rows = month_weeks(d(10, 1), Weekday::Mon);
        assert_eq!(rows[0][0], d(9, 28));
        assert_eq!(rows.last().unwrap()[6], d(11, 1));
        // February 2026 starts on a Sunday: exactly four Sunday-first rows.
        assert_eq!(month_weeks(d(2, 10), Weekday::Sun).len(), 4);
    }

    #[test]
    fn month_puts_long_events_above_timed_ones() {
        let rows = month_weeks(d(10, 1), Weekday::Sun);
        let events = vec![
            event("lunch", Span::Timed { start: at(10, 21, 12), end: at(10, 21, 13) }),
            event("trip", Span::AllDay { first: d(10, 20), last: d(10, 23) }),
            event("call", Span::Timed { start: at(10, 21, 9), end: at(10, 21, 10) }),
            event("late", Span::Timed { start: at(10, 21, 22), end: at(10, 22, 0) }),
        ];
        let layout = MonthLayout::new(rows, &events);
        let row = &layout.bars[3];
        let lane = |i: usize| row.iter().find(|b| b.event == i).map(|b| b.lane);
        assert_eq!(lane(1), Some(0));
        assert_eq!(lane(2), Some(1));
        assert_eq!(lane(0), Some(2));
        assert_eq!(lane(3), Some(3));
        // Ending at midnight stays on one day.
        assert_eq!(row.iter().find(|b| b.event == 3).map(|b| (b.first_day, b.last_day)), Some((3, 3)));
        assert_eq!(layout.lanes(3), 4);
    }

    #[test]
    fn agenda_lists_days_with_events() {
        let events = vec![
            event("call", Span::Timed { start: at(10, 22, 9), end: at(10, 22, 10) }),
            event("trip", Span::AllDay { first: d(10, 21), last: d(10, 22) }),
        ];
        let days = agenda(&events, d(10, 20), 7);
        assert_eq!(days.iter().map(|a| a.day).collect::<Vec<_>>(), vec![d(10, 21), d(10, 22)]);
        assert_eq!(days[1].events, vec![1, 0]);
    }

    #[test]
    fn invitation_finds_me_unless_organizing() {
        let mut e = event("x", Span::AllDay { first: d(10, 1), last: d(10, 1) });
        let person = |email: &str| crate::Attendee { email: email.into(), ..Default::default() };
        e.attendees = vec![person("alex@example.com"), person("Me@Example.com")];
        e.organizer = Some(person("alex@example.com"));
        let me = vec!["me@example.com".to_string()];
        assert_eq!(e.invitation(&me).map(|a| a.email.as_str()), Some("Me@Example.com"));
        e.organizer = Some(person("me@example.com"));
        assert!(e.invitation(&me).is_none());
    }
}
