//! Laying a week out: timed events in side-by-side columns where they overlap,
//! all-day events in lanes across the top.

use crate::{Event, Span};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike, Weekday};

/// The first day of the week holding `date`.
pub fn week_start(date: NaiveDate, first: Weekday) -> NaiveDate {
    let back = (7 + date.weekday().num_days_from_monday() - first.num_days_from_monday()) % 7;
    date - Duration::days(back as i64)
}

/// The seven days from `start`.
pub fn week_days(start: NaiveDate) -> [NaiveDate; 7] {
    std::array::from_fn(|i| start + Duration::days(i as i64))
}

/// A timed event placed in one day's column. Minutes are from that day's midnight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub event: usize,
    pub day: usize,
    pub top: u32,
    pub bottom: u32,
    /// Column within its overlap cluster, and how many columns the cluster has.
    pub column: u32,
    pub columns: u32,
    /// The event started before this day or ends after it.
    pub clipped_top: bool,
    pub clipped_bottom: bool,
}

/// An all-day (or multi-day) bar across the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllDayBar {
    pub event: usize,
    pub first_day: usize,
    pub last_day: usize,
    pub lane: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeekLayout {
    pub timed: Vec<Placed>,
    pub all_day: Vec<AllDayBar>,
    pub lanes: u32,
}

/// The shortest block drawn, so a 5-minute event is still readable.
const MIN_MINUTES: u32 = 30;
const DAY_MINUTES: u32 = 24 * 60;

impl WeekLayout {
    /// Lays out `events` (indices refer to it) for the week of `days`.
    /// Timed events of 24 hours or more go across the top with the all-day ones.
    pub fn new(days: &[NaiveDate], events: &[Event]) -> Self {
        let mut out = WeekLayout::default();
        let mut bars = Vec::new();
        let mut per_day: Vec<Vec<Placed>> = vec![Vec::new(); days.len()];
        for (index, event) in events.iter().enumerate() {
            match event.span {
                Span::AllDay { first, last } => bars.push((index, first, last)),
                Span::Timed { start, end } if end - start >= Duration::hours(24) => {
                    let last =
                        if end.time() == NaiveTime::MIN { end.date().pred_opt().unwrap_or(end.date()) } else { end.date() };
                    bars.push((index, start.date(), last));
                }
                Span::Timed { start, end } => {
                    for (day_index, day) in days.iter().enumerate() {
                        let day_start = day.and_hms_opt(0, 0, 0).unwrap_or_default();
                        let day_end = day_start + Duration::days(1);
                        let touches = start < day_end && (end > day_start || (end == start && start >= day_start));
                        if !touches {
                            continue;
                        }
                        let top = if start < day_start { 0 } else { minutes(start.time()) };
                        let bottom = if end >= day_end { DAY_MINUTES } else { minutes(end.time()) };
                        let bottom = bottom.max(top + MIN_MINUTES).min(DAY_MINUTES);
                        let top = top.min(DAY_MINUTES - MIN_MINUTES);
                        per_day[day_index].push(Placed {
                            event: index,
                            day: day_index,
                            top,
                            bottom,
                            column: 0,
                            columns: 1,
                            clipped_top: start < day_start,
                            clipped_bottom: end > day_end,
                        });
                    }
                }
            }
        }
        for mut placed in per_day {
            columns(&mut placed);
            out.timed.extend(placed);
        }
        out.all_day = lanes(days, &mut bars, events);
        out.lanes = out.all_day.iter().map(|b| b.lane + 1).max().unwrap_or(0);
        out
    }
}

fn minutes(t: NaiveTime) -> u32 {
    t.hour() * 60 + t.minute()
}

/// Assigns columns within each cluster of overlapping blocks.
fn columns(placed: &mut [Placed]) {
    placed.sort_by_key(|p| (p.top, std::cmp::Reverse(p.bottom)));
    let mut cluster_start = 0;
    let mut cluster_end = 0;
    let mut ends: Vec<u32> = Vec::new();
    for i in 0..placed.len() {
        if i > cluster_start && placed[i].top >= cluster_end {
            finish(&mut placed[cluster_start..i], ends.len() as u32);
            cluster_start = i;
            ends.clear();
        }
        let column = match ends.iter().position(|end| *end <= placed[i].top) {
            Some(c) => c,
            None => {
                ends.push(0);
                ends.len() - 1
            }
        };
        ends[column] = placed[i].bottom;
        placed[i].column = column as u32;
        cluster_end = if i == cluster_start { placed[i].bottom } else { cluster_end.max(placed[i].bottom) };
    }
    let n = ends.len() as u32;
    finish(&mut placed[cluster_start..], n);
}

fn finish(cluster: &mut [Placed], columns: u32) {
    for p in cluster {
        p.columns = columns.max(1);
    }
}

/// Packs bars into the fewest lanes, longest first.
fn lanes(days: &[NaiveDate], bars: &mut [(usize, NaiveDate, NaiveDate)], events: &[Event]) -> Vec<AllDayBar> {
    bars.sort_by_key(|(index, first, last)| (*first, std::cmp::Reverse(*last - *first), events[*index].title.clone()));
    pack(days, bars)
}

/// Packs bars, in the order given, into the fewest lanes; bars outside the
/// week are dropped and the rest clipped to it.
pub(crate) fn pack(days: &[NaiveDate], bars: &[(usize, NaiveDate, NaiveDate)]) -> Vec<AllDayBar> {
    let (Some(&week_first), Some(&week_last)) = (days.first(), days.last()) else { return Vec::new() };
    let mut used: Vec<Vec<bool>> = Vec::new();
    let mut out = Vec::new();
    for (index, first, last) in bars.iter() {
        if *last < week_first || *first > week_last {
            continue;
        }
        let a = (*first.max(&week_first) - week_first).num_days() as usize;
        let b = (*last.min(&week_last) - week_first).num_days() as usize;
        let lane = match used.iter().position(|row| !row[a..=b].iter().any(|x| *x)) {
            Some(l) => l,
            None => {
                used.push(vec![false; days.len()]);
                used.len() - 1
            }
        };
        used[lane][a..=b].iter_mut().for_each(|x| *x = true);
        out.push(AllDayBar { event: *index, first_day: a, last_day: b, lane: lane as u32 });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn timed(title: &str, day: u32, h1: u32, m1: u32, h2: u32, m2: u32) -> Event {
        Event {
            calendar: "c".into(),
            uid: title.into(),
            rid: None,
            title: title.into(),
            location: String::new(),
            span: Span::Timed { start: d(day).and_hms_opt(h1, m1, 0).unwrap(), end: d(day).and_hms_opt(h2, m2, 0).unwrap() },
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

    fn all_day(title: &str, first: u32, last: u32) -> Event {
        Event { span: Span::AllDay { first: d(first), last: d(last) }, ..timed(title, first, 0, 0, 0, 0) }
    }

    #[test]
    fn week_starts_on_the_chosen_day() {
        // 2026-10-21 is a Wednesday.
        assert_eq!(week_start(d(21), Weekday::Sun), d(18));
        assert_eq!(week_start(d(21), Weekday::Mon), d(19));
        assert_eq!(week_start(d(18), Weekday::Sun), d(18));
        assert_eq!(week_days(d(18))[6], d(24));
    }

    #[test]
    fn overlapping_events_share_columns() {
        let days = week_days(d(18));
        let events = vec![
            timed("a", 21, 9, 0, 10, 0),
            timed("b", 21, 9, 30, 11, 0),
            timed("c", 21, 10, 0, 10, 30),
            timed("d", 21, 13, 0, 14, 0),
        ];
        let layout = WeekLayout::new(&days, &events);
        let by = |i: usize| layout.timed.iter().find(|p| p.event == i).unwrap().clone();
        assert_eq!((by(0).column, by(0).columns, by(0).day), (0, 2, 3));
        assert_eq!((by(1).column, by(1).columns), (1, 2));
        // "c" starts when "a" ends, so it reuses column 0.
        assert_eq!((by(2).column, by(2).columns), (0, 2));
        assert_eq!((by(3).column, by(3).columns, by(3).top, by(3).bottom), (0, 1, 780, 840));
    }

    #[test]
    fn events_crossing_midnight_split_across_days() {
        let days = week_days(d(18));
        let mut late = timed("late", 21, 22, 0, 23, 0);
        late.span = Span::Timed { start: d(21).and_hms_opt(22, 0, 0).unwrap(), end: d(22).and_hms_opt(2, 0, 0).unwrap() };
        let layout = WeekLayout::new(&days, &[late]);
        assert_eq!(layout.timed.len(), 2);
        assert_eq!((layout.timed[0].top, layout.timed[0].bottom, layout.timed[0].clipped_bottom), (1320, 1440, true));
        assert_eq!(
            (layout.timed[1].day, layout.timed[1].top, layout.timed[1].bottom, layout.timed[1].clipped_top),
            (4, 0, 120, true)
        );
    }

    #[test]
    fn short_events_get_a_minimum_height() {
        let layout = WeekLayout::new(&week_days(d(18)), &[timed("x", 19, 23, 55, 23, 55)]);
        assert_eq!((layout.timed[0].top, layout.timed[0].bottom), (1410, 1440));
    }

    #[test]
    fn all_day_events_pack_into_lanes_and_clip_to_the_week() {
        let days = week_days(d(18));
        let events = vec![all_day("trip", 16, 20), all_day("holiday", 22, 22), all_day("conf", 19, 23), all_day("later", 26, 27)];
        let layout = WeekLayout::new(&days, &events);
        let bar = |i: usize| layout.all_day.iter().find(|b| b.event == i).cloned();
        assert_eq!(bar(0), Some(AllDayBar { event: 0, first_day: 0, last_day: 2, lane: 0 }));
        assert_eq!(bar(2).map(|b| (b.first_day, b.last_day, b.lane)), Some((1, 5, 1)));
        assert_eq!(bar(1).map(|b| (b.first_day, b.lane)), Some((4, 0)));
        assert_eq!(bar(3), None);
        assert_eq!(layout.lanes, 2);
    }

    #[test]
    fn day_long_timed_events_go_on_top() {
        let mut long = timed("flight", 20, 0, 0, 0, 0);
        long.span = Span::Timed { start: d(20).and_hms_opt(8, 0, 0).unwrap(), end: d(21).and_hms_opt(9, 0, 0).unwrap() };
        let layout = WeekLayout::new(&week_days(d(18)), &[long]);
        assert!(layout.timed.is_empty());
        assert_eq!(layout.all_day[0].first_day, 2);
        assert_eq!(layout.all_day[0].last_day, 3);
    }
}
