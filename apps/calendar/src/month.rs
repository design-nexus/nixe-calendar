//! The month view: a grid of weeks, each day with its events as bars.

use crate::draw::{self, Calendars, Hooks, Slot, Style};
use chrono::{Datelike, Local, NaiveDate};
use gtk::prelude::*;
use nixe_calendar_core::{Event, MonthLayout, Span};
use std::cell::RefCell;
use std::rc::Rc;

const WEEKDAY_ROW: f64 = 28.0;
const DAY_LABEL: f64 = 26.0;
const BAR: f64 = 20.0;

type Hit = (f64, f64, f64, f64, usize);

#[derive(Default)]
struct Data {
    month: u32,
    layout: MonthLayout,
    events: Vec<Event>,
    calendars: Calendars,
    hits: Vec<Hit>,
    /// "+N more" and day numbers: (x, y, w, h, date).
    day_hits: Vec<(f64, f64, f64, f64, NaiveDate)>,
}

#[derive(Clone)]
pub struct MonthView {
    pub root: gtk::DrawingArea,
    data: Rc<RefCell<Data>>,
}

impl MonthView {
    pub fn new(hooks: Rc<Hooks>) -> Self {
        let data = Rc::new(RefCell::new(Data::default()));
        let root = gtk::DrawingArea::new();
        root.set_vexpand(true);
        root.set_hexpand(true);
        let d = data.clone();
        root.set_draw_func(move |area, cr, w, h| draw_month(area, cr, w as f64, h as f64, &mut d.borrow_mut()));

        let menu_hooks = hooks.clone();
        let click = gtk::GestureClick::new();
        let (d, target) = (data.clone(), root.clone());
        click.connect_pressed(move |_, n, x, y| {
            let d = d.borrow();
            let inside = |(hx, hy, hw, hh): (f64, f64, f64, f64)| x >= hx && x <= hx + hw && y >= hy && y <= hy + hh;
            if n == 1 {
                if let Some(&(hx, hy, hw, hh, index)) = d.hits.iter().find(|h| inside((h.0, h.1, h.2, h.3))) {
                    let event = d.events[index].clone();
                    drop(d);
                    (hooks.event)(target.upcast_ref(), draw::rect(hx, hy, hw, hh), event);
                    return;
                }
                if let Some(&(_, _, _, _, date)) = d.day_hits.iter().find(|h| inside((h.0, h.1, h.2, h.3))) {
                    drop(d);
                    (hooks.day)(date);
                }
                return;
            }
            if n == 2 && d.hits.iter().all(|h| !inside((h.0, h.1, h.2, h.3))) {
                let (cols, rows) = (7.0, d.layout.rows.len().max(1) as f64);
                let (cw, rh) = (target.width() as f64 / cols, (target.height() as f64 - WEEKDAY_ROW) / rows);
                let (col, row) = ((x / cw).floor(), ((y - WEEKDAY_ROW) / rh).floor());
                if col >= 0.0 && row >= 0.0 && (row as usize) < d.layout.rows.len() && col < 7.0 {
                    let date = d.layout.rows[row as usize][col as usize];
                    let rect = draw::rect(col * cw, WEEKDAY_ROW + row * rh, cw, rh);
                    drop(d);
                    (hooks.empty)(target.upcast_ref(), rect, Slot::Day(date));
                }
            }
        });
        root.add_controller(click);

        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (d, hooks, target) = (data.clone(), menu_hooks, root.clone());
        menu.connect_pressed(move |gesture, _, x, y| {
            let d = d.borrow();
            let inside = |(hx, hy, hw, hh): (f64, f64, f64, f64)| x >= hx && x <= hx + hw && y >= hy && y <= hy + hh;
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(&(.., index)) = d.hits.iter().find(|h| inside((h.0, h.1, h.2, h.3))) {
                let event = d.events[index].clone();
                drop(d);
                (hooks.event_menu)(target.upcast_ref(), x, y, event);
                return;
            }
            let (cols, rows) = (7.0, d.layout.rows.len().max(1) as f64);
            let (cw, rh) = (target.width() as f64 / cols, (target.height() as f64 - WEEKDAY_ROW) / rows);
            let (col, row) = ((x / cw).floor(), ((y - WEEKDAY_ROW) / rh).floor());
            if col >= 0.0 && row >= 0.0 && (row as usize) < d.layout.rows.len() && col < 7.0 {
                let date = d.layout.rows[row as usize][col as usize];
                drop(d);
                (hooks.empty_menu)(target.upcast_ref(), x, y, Slot::Day(date));
            }
        });
        root.add_controller(menu);

        let view = Self { root, data };
        let v = view.clone();
        nixe_theme::subscribe(&view.root, move || v.root.queue_draw());
        view
    }

    pub fn set(&self, month: u32, rows: Vec<[NaiveDate; 7]>, events: &[Event], calendars: Calendars) {
        {
            let mut d = self.data.borrow_mut();
            let (first, last) = (rows.first().map(|r| r[0]), rows.last().map(|r| r[6]));
            let shown: Vec<Event> = events
                .iter()
                .filter(|e| match (first, last) {
                    (Some(a), Some(b)) => e.days().1 >= a && e.days().0 <= b,
                    _ => false,
                })
                .cloned()
                .collect();
            d.layout = MonthLayout::new(rows, &shown);
            d.events = shown;
            d.month = month;
            d.calendars = calendars;
        }
        self.root.queue_draw();
    }
}

fn draw_month(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, height: f64, d: &mut Data) {
    let c = draw::colors();
    let rows = d.layout.rows.len().max(1);
    let cw = width / 7.0;
    let rh = (height - WEEKDAY_ROW) / rows as f64;
    let today = Local::now().date_naive();
    d.hits.clear();
    d.day_hits.clear();

    // Weekday names from the first row.
    if let Some(first) = d.layout.rows.first() {
        for (i, day) in first.iter().enumerate() {
            draw::set(cr, c.dim);
            let name = day.format("%a").to_string().to_uppercase();
            draw::text(area, cr, &name, i as f64 * cw + 10.0, 7.0, cw - 14.0, Style { scale: 0.76, bold: true, ..draw::NORMAL });
        }
    }
    draw::set(cr, c.line);
    cr.set_line_width(1.0);
    cr.move_to(0.0, WEEKDAY_ROW - 0.5);
    cr.line_to(width, WEEKDAY_ROW - 0.5);
    for r in 1..rows {
        let y = (WEEKDAY_ROW + r as f64 * rh).floor() + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(width, y);
    }
    for i in 1..7 {
        let x = (i as f64 * cw).floor() + 0.5;
        cr.move_to(x, WEEKDAY_ROW);
        cr.line_to(x, height);
    }
    let _ = cr.stroke();

    let lanes_fit = (((rh - DAY_LABEL - 4.0) / BAR).floor() as i64).max(0) as u32;
    for (r, row) in d.layout.rows.clone().iter().enumerate() {
        let top = WEEKDAY_ROW + r as f64 * rh;
        for (i, day) in row.iter().enumerate() {
            let x = i as f64 * cw;
            let other_month = day.month() != d.month;
            if *day == today {
                draw::set(cr, c.accent);
                draw::rounded(cr, x + 5.0, top + 4.0, 28.0, 19.0, 6.0);
                let _ = cr.fill();
                draw::set(cr, c.on_accent);
            } else {
                draw::set(cr, if other_month { c.dim } else { c.text });
            }
            let label = if day.day() == 1 { day.format("%b %-d").to_string() } else { day.day().to_string() };
            draw::text(
                area,
                cr,
                &label,
                x + 9.0,
                top + 5.0,
                cw - 14.0,
                Style { scale: 0.92, bold: *day == today, ..draw::NORMAL },
            );
            d.day_hits.push((x, top, cw.min(60.0), DAY_LABEL, *day));
        }
        // Bars that fit; one lane is kept for "+N more" when some don't.
        let bars = d.layout.bars.get(r).cloned().unwrap_or_default();
        let total = d.layout.lanes(r);
        let usable = if total > lanes_fit { lanes_fit.saturating_sub(1) } else { lanes_fit };
        let mut hidden = [0u32; 7];
        for bar in &bars {
            if bar.lane >= usable {
                hidden[bar.first_day..=bar.last_day].iter_mut().for_each(|n| *n += 1);
                continue;
            }
            let event = &d.events[bar.event];
            let color = draw::event_color(&d.calendars, event, c.accent);
            let x = bar.first_day as f64 * cw + 3.0;
            let w = (bar.last_day - bar.first_day + 1) as f64 * cw - 6.0;
            let y = top + DAY_LABEL + bar.lane as f64 * BAR;
            let h = BAR - 3.0;
            let spans_days = event.days().0 != event.days().1 || event.all_day();
            if spans_days {
                draw::set(cr, c.bg.mix(color, if event.cancelled { 0.14 } else { 0.32 }));
                draw::rounded(cr, x, y, w, h, 4.0);
                let _ = cr.fill();
                draw::set(cr, c.text);
                draw::text(
                    area,
                    cr,
                    &event.title,
                    x + 6.0,
                    y + 1.0,
                    w - 10.0,
                    Style { scale: 0.84, strike: event.cancelled, ..draw::NORMAL },
                );
            } else {
                draw::set(cr, color);
                cr.arc(x + 6.0, y + h / 2.0, 3.5, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                let time = match event.span {
                    Span::Timed { start, .. } => draw::time_text(start.time()),
                    Span::AllDay { .. } => String::new(),
                };
                let line = format!(
                    "<span foreground=\"{}\">{}</span>  {}",
                    c.dim.to_hex(),
                    glib::markup_escape_text(&time),
                    glib::markup_escape_text(&event.title)
                );
                draw::set(cr, c.text);
                draw::markup(area, cr, &line, x + 14.0, y + 1.0, w - 18.0, 0.84, event.cancelled);
            }
            d.hits.push((x, y, w, h, bar.event));
        }
        for (i, count) in hidden.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let x = i as f64 * cw;
            let y = top + DAY_LABEL + usable as f64 * BAR;
            draw::set(cr, c.dim);
            draw::text(
                area,
                cr,
                &format!("+{count} more"),
                x + 9.0,
                y + 1.0,
                cw - 14.0,
                Style { scale: 0.8, bold: true, ..draw::NORMAL },
            );
            d.day_hits.push((x, y, cw, BAR, row[i]));
        }
    }
}
