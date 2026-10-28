//! The day and week views: day headers and all-day lanes on top, a scrolling
//! grid of hours below, one column per day.

use crate::draw::{self, Calendars, Hooks, Slot, Style};
use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use gtk::prelude::*;
use nyx_calendar_core::{Event, Span, WeekLayout};
use std::cell::RefCell;
use std::rc::Rc;

const GUTTER: f64 = 56.0;
const HOUR: f64 = 48.0;
const DAY_HEADER: f64 = 52.0;
const LANE: f64 = 22.0;

type Hit = (f64, f64, f64, f64, usize);

#[derive(Default)]
struct Data {
    days: Vec<NaiveDate>,
    events: Vec<Event>,
    layout: WeekLayout,
    calendars: Calendars,
    grid_hits: Vec<Hit>,
    header_hits: Vec<Hit>,
}

#[derive(Clone)]
pub struct DaysView {
    pub root: gtk::Box,
    header: gtk::DrawingArea,
    grid: gtk::DrawingArea,
    scroll: gtk::ScrolledWindow,
    data: Rc<RefCell<Data>>,
}

impl DaysView {
    pub fn new(hooks: Rc<Hooks>) -> Self {
        let data = Rc::new(RefCell::new(Data::default()));
        let header = gtk::DrawingArea::new();
        header.add_css_class("week-header");
        header.set_content_height((DAY_HEADER + LANE + 6.0) as i32);
        let grid = gtk::DrawingArea::new();
        grid.set_content_height((24.0 * HOUR) as i32);
        grid.set_vexpand(true);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&grid)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&header);
        root.append(&scroll);

        let d = data.clone();
        header.set_draw_func(move |area, cr, w, _| draw_header(area, cr, w as f64, &mut d.borrow_mut()));
        let d = data.clone();
        grid.set_draw_func(move |area, cr, w, _| draw_grid(area, cr, w as f64, &mut d.borrow_mut()));

        for (area, top) in [(&header, true), (&grid, false)] {
            let menu_hooks = hooks.clone();
            let click = gtk::GestureClick::new();
            let (d, hooks, target) = (data.clone(), hooks.clone(), area.clone());
            click.connect_pressed(move |gesture, n, x, y| {
                let d = d.borrow();
                let hits = if top { &d.header_hits } else { &d.grid_hits };
                let hit =
                    hits.iter().rev().find(|(hx, hy, hw, hh, _)| x >= *hx && x <= hx + hw && y >= *hy && y <= hy + hh).copied();
                let col = column_width(target.width() as f64, d.days.len());
                let day = ((x - GUTTER) / col).floor();
                match hit {
                    Some((hx, hy, hw, hh, index)) if n == 1 => {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        let event = d.events[index].clone();
                        drop(d);
                        (hooks.event)(target.upcast_ref(), draw::rect(hx, hy, hw, hh), event);
                    }
                    None if n == 2 && x > GUTTER && day >= 0.0 && (day as usize) < d.days.len() => {
                        let date = d.days[day as usize];
                        let (slot, rect) = if top {
                            (Slot::Day(date), draw::rect(GUTTER + day * col, DAY_HEADER, col, LANE))
                        } else {
                            // Half-hour steps.
                            let minutes = ((y / HOUR * 2.0).floor() * 30.0).clamp(0.0, 23.5 * 60.0) as u32;
                            let at = date.and_time(NaiveTime::from_hms_opt(minutes / 60, minutes % 60, 0).unwrap_or_default());
                            (Slot::At(at), draw::rect(GUTTER + day * col, minutes as f64 / 60.0 * HOUR, col, HOUR))
                        };
                        drop(d);
                        (hooks.empty)(target.upcast_ref(), rect, slot);
                    }
                    None if n == 1 && top && y < DAY_HEADER && day >= 0.0 && (day as usize) < d.days.len() => {
                        let date = d.days[day as usize];
                        drop(d);
                        (hooks.day)(date);
                    }
                    _ => {}
                }
            });
            area.add_controller(click);

            let menu = gtk::GestureClick::new();
            menu.set_button(gtk::gdk::BUTTON_SECONDARY);
            let (d, hooks, target) = (data.clone(), menu_hooks, area.clone());
            menu.connect_pressed(move |gesture, _, x, y| {
                let d = d.borrow();
                let hits = if top { &d.header_hits } else { &d.grid_hits };
                let hit = hits.iter().rev().find(|(hx, hy, hw, hh, _)| x >= *hx && x <= hx + hw && y >= *hy && y <= hy + hh);
                if let Some(&(.., index)) = hit {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    let event = d.events[index].clone();
                    drop(d);
                    (hooks.event_menu)(target.upcast_ref(), x, y, event);
                    return;
                }
                let col = column_width(target.width() as f64, d.days.len());
                let day = ((x - GUTTER) / col).floor();
                if x <= GUTTER || day < 0.0 || day as usize >= d.days.len() {
                    return;
                }
                let date = d.days[day as usize];
                let slot = if top {
                    Slot::Day(date)
                } else {
                    let minutes = ((y / HOUR * 2.0).floor() * 30.0).clamp(0.0, 23.5 * 60.0) as u32;
                    Slot::At(date.and_time(NaiveTime::from_hms_opt(minutes / 60, minutes % 60, 0).unwrap_or_default()))
                };
                drop(d);
                gesture.set_state(gtk::EventSequenceState::Claimed);
                (hooks.empty_menu)(target.upcast_ref(), x, y, slot);
            });
            area.add_controller(menu);
        }

        let view = Self { root, header, grid, scroll, data };
        let v = view.clone();
        nyx_theme::subscribe(&view.root, move || v.redraw());
        let grid = view.grid.downgrade();
        glib::timeout_add_seconds_local(60, move || match grid.upgrade() {
            Some(g) => {
                g.queue_draw();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        view
    }

    pub fn set(&self, days: Vec<NaiveDate>, events: &[Event], calendars: Calendars) {
        {
            let mut d = self.data.borrow_mut();
            let shown: Vec<Event> =
                events.iter().filter(|e| days.iter().any(|day| (e.days().0..=e.days().1).contains(day))).cloned().collect();
            d.layout = WeekLayout::new(&days, &shown);
            d.days = days;
            d.events = shown;
            d.calendars = calendars;
            let lanes = d.layout.lanes.max(1) as f64;
            self.header.set_content_height((DAY_HEADER + lanes * LANE + 6.0) as i32);
        }
        self.redraw();
    }

    pub fn scroll_to_hour(&self, hour: u32) {
        let adj = self.scroll.vadjustment();
        // A little above the hour, so its label is not cut in half.
        let target = (hour as f64 * HOUR - 12.0).max(0.0);
        glib::idle_add_local_once(move || adj.set_value(target.min((adj.upper() - adj.page_size()).max(0.0))));
    }

    fn redraw(&self) {
        self.header.queue_draw();
        self.grid.queue_draw();
    }
}

fn column_width(width: f64, days: usize) -> f64 {
    ((width - GUTTER) / days.max(1) as f64).max(10.0)
}

fn draw_header(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, d: &mut Data) {
    let c = draw::colors();
    let col = column_width(width, d.days.len());
    let today = Local::now().date_naive();
    let single = d.days.len() == 1;
    d.header_hits.clear();
    // One line per day, as in the mockup: "SUN 25", today's date on an accent square.
    let name_style = Style { scale: 0.76, bold: true, ..draw::NORMAL };
    let date_style = Style { scale: 1.24, bold: true, ..draw::NORMAL };
    let today_style = Style { scale: 1.1, bold: true, ..draw::NORMAL };
    let mid = DAY_HEADER / 2.0;
    for (i, day) in d.days.iter().enumerate() {
        let x = GUTTER + i as f64 * col + 10.0;
        let is_today = *day == today;
        let name = if single { day.format("%A") } else { day.format("%a") }.to_string().to_uppercase();
        let (name_w, name_h) = draw::text_size(area, &name, name_style);
        draw::set(cr, if is_today { c.accent } else { c.dim.mix(c.bg, 0.3) });
        draw::text(area, cr, &name, x, mid - name_h / 2.0, col - 20.0, name_style);
        let date = day.day().to_string();
        let dx = x + name_w + 8.0;
        if is_today {
            let (w, h) = draw::text_size(area, &date, today_style);
            draw::set(cr, c.accent);
            draw::rounded(cr, dx, mid - 16.0, 32.0, 32.0, 9.0);
            let _ = cr.fill();
            draw::set(cr, c.on_accent);
            draw::text(area, cr, &date, dx + (32.0 - w) / 2.0, mid - h / 2.0, 32.0, today_style);
        } else {
            let (_, h) = draw::text_size(area, &date, date_style);
            draw::set(cr, c.text);
            draw::text(area, cr, &date, dx, mid - h / 2.0, (col - 20.0 - name_w - 8.0).max(1.0), date_style);
        }
    }
    // No column lines: the days are told apart by their headings, as in the mockup.
    draw::set(cr, c.dim);
    draw::text(area, cr, "all day", 6.0, DAY_HEADER + 4.0, GUTTER - 10.0, Style { scale: 0.72, ..draw::NORMAL });

    for bar in d.layout.all_day.clone() {
        let event = &d.events[bar.event];
        let color = draw::event_color(&d.calendars, event, c.accent);
        let x = GUTTER + bar.first_day as f64 * col + 3.0;
        let w = (bar.last_day - bar.first_day + 1) as f64 * col - 6.0;
        let y = DAY_HEADER + bar.lane as f64 * LANE + 2.0;
        let h = LANE - 4.0;
        let (title, _) = draw::event_block(cr, &c, color, event.cancelled, x, y, w, h, 5.0);
        draw::set(cr, title);
        draw::text(
            area,
            cr,
            &event.title,
            x + 7.0,
            y + 1.0,
            w - 12.0,
            Style { scale: 0.84, bold: true, strike: event.cancelled, ..draw::NORMAL },
        );
        d.header_hits.push((x, y, w, h, bar.event));
    }
}

fn draw_grid(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, d: &mut Data) {
    let c = draw::colors();
    let col = column_width(width, d.days.len());
    let now = Local::now().naive_local();
    if let Some(i) = d.days.iter().position(|day| *day == now.date())
        && d.days.len() > 1
    {
        draw::set_a(cr, c.accent, 0.05);
        cr.rectangle(GUTTER + i as f64 * col, 0.0, col, 24.0 * HOUR);
        let _ = cr.fill();
    }
    for h in 0..24 {
        let y = h as f64 * HOUR;
        // A faint hour line across the days; no column lines.
        draw::grid_line(cr, &c);
        cr.move_to(GUTTER, y.floor() + 0.5);
        cr.line_to(width, y.floor() + 0.5);
        let _ = cr.stroke();
        if h > 0 {
            draw::set(cr, c.dim);
            draw::text(area, cr, &draw::hour_label(h), 4.0, y - 8.0, GUTTER - 14.0, Style { scale: 0.76, ..draw::NORMAL });
        }
    }
    d.grid_hits.clear();
    for p in d.layout.timed.clone() {
        let event = &d.events[p.event];
        let color = draw::event_color(&d.calendars, event, c.accent);
        let slot = col / p.columns as f64;
        let x = GUTTER + p.day as f64 * col + p.column as f64 * slot + 3.0;
        let w = slot - 7.0;
        let y = p.top as f64 / 60.0 * HOUR + 1.0;
        let h = (p.bottom - p.top) as f64 / 60.0 * HOUR - 2.0;
        let (title, detail_color) = draw::event_block(cr, &c, color, event.cancelled, x, y, w, h, 7.0);
        cr.save().ok();
        cr.rectangle(x, y, w, h);
        cr.clip();
        draw::set(cr, title);
        // Short events keep their title centred in the block.
        let top = if h < 24.0 { y + (h - 16.0).max(0.0) / 2.0 } else { y + 4.0 };
        let used = draw::text(
            area,
            cr,
            &event.title,
            x + 8.0,
            top,
            w - 14.0,
            Style { scale: 0.84, bold: true, strike: event.cancelled, ..draw::NORMAL },
        );
        if h > used + 18.0 {
            draw::set(cr, detail_color);
            let when = match event.span {
                Span::Timed { start, end } => draw::time_range(start, end),
                Span::AllDay { .. } => String::new(),
            };
            let detail = if event.location.is_empty() { when } else { format!("{when} · {}", event.location) };
            draw::text(area, cr, &detail, x + 8.0, top + used, w - 14.0, Style { scale: 0.78, ..draw::NORMAL });
        }
        cr.restore().ok();
        d.grid_hits.push((x, y, w, h, p.event));
    }

    if let Some(i) = d.days.iter().position(|day| *day == now.date()) {
        let y = (now.hour() as f64 + now.minute() as f64 / 60.0) * HOUR;
        let x = GUTTER + i as f64 * col;
        draw::set(cr, c.accent);
        cr.set_line_width(2.0);
        cr.move_to(x, y);
        cr.line_to(x + col, y);
        let _ = cr.stroke();
        cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}
