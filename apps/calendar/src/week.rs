//! The week view: day headers and all-day lanes on top, a scrolling grid of
//! hours below. Drawn with Cairo in theme colors; event colors are the
//! calendars' own.

use crate::prefs;
use crate::widgets;
use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use gtk::prelude::*;
use nixe_calendar_core::{Event, Span, WeekLayout};
use nixe_theme::Rgb;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const GUTTER: f64 = 56.0;
const HOUR: f64 = 48.0;
const DAY_HEADER: f64 = 46.0;
const LANE: f64 = 22.0;

/// A calendar's name and color, by uid.
pub type Calendars = HashMap<String, (String, Option<String>)>;

#[derive(Default)]
struct Data {
    days: [NaiveDate; 7],
    /// Every event of the week, and the ones the filter shows (indices refer to these).
    all: Vec<Event>,
    events: Vec<Event>,
    layout: WeekLayout,
    calendars: Calendars,
    filter: String,
    /// Event rectangles from the last draw, for clicks: (x, y, w, h, event index).
    grid_hits: Vec<(f64, f64, f64, f64, usize)>,
    header_hits: Vec<(f64, f64, f64, f64, usize)>,
}

#[derive(Clone)]
pub struct WeekView {
    pub root: gtk::Box,
    header: gtk::DrawingArea,
    grid: gtk::DrawingArea,
    scroll: gtk::ScrolledWindow,
    data: Rc<RefCell<Data>>,
}

impl WeekView {
    pub fn new() -> Self {
        let data = Rc::new(RefCell::new(Data::default()));
        let header = gtk::DrawingArea::new();
        header.add_css_class("week-header");
        header.set_content_height((DAY_HEADER + LANE + 6.0) as i32);
        let grid = gtk::DrawingArea::new();
        grid.add_css_class("week-grid");
        grid.set_content_height((24.0 * HOUR) as i32);
        grid.set_vexpand(true);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&grid)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("week-view");
        root.append(&header);
        root.append(&scroll);

        let d = data.clone();
        header.set_draw_func(move |area, cr, w, _h| draw_header(area, cr, w as f64, &mut d.borrow_mut()));
        let d = data.clone();
        grid.set_draw_func(move |area, cr, w, _h| draw_grid(area, cr, w as f64, &mut d.borrow_mut()));

        for (area, top) in [(&header, true), (&grid, false)] {
            let click = gtk::GestureClick::new();
            let d = data.clone();
            let target = area.clone();
            click.connect_released(move |_, _, x, y| {
                let hit = {
                    let d = d.borrow();
                    let hits = if top { &d.header_hits } else { &d.grid_hits };
                    hits.iter().rev().find(|(hx, hy, hw, hh, _)| x >= *hx && x <= hx + hw && y >= *hy && y <= hy + hh).copied()
                };
                if let Some((hx, hy, hw, hh, index)) = hit {
                    show_details(&target, &d.borrow(), index, (hx, hy, hw, hh));
                }
            });
            area.add_controller(click);
        }

        let view = Self { root, header, grid, scroll, data };
        let v = view.clone();
        nixe_theme::subscribe(&view.root, move || v.redraw());
        let v = view.clone();
        glib::timeout_add_seconds_local(60, move || {
            v.grid.queue_draw();
            glib::ControlFlow::Continue
        });
        view
    }

    pub fn set(&self, days: [NaiveDate; 7], events: Vec<Event>, calendars: Calendars) {
        {
            let mut d = self.data.borrow_mut();
            let shown = visible(&events, &d.filter);
            d.layout = WeekLayout::new(&days, &shown);
            d.days = days;
            d.events = shown;
            d.all = events;
            d.calendars = calendars;
            let lanes = d.layout.lanes.max(1) as f64;
            self.header.set_content_height((DAY_HEADER + lanes * LANE + 6.0) as i32);
        }
        self.redraw();
    }

    pub fn set_filter(&self, filter: &str) {
        let (days, events, calendars) = {
            let mut d = self.data.borrow_mut();
            d.filter = filter.trim().to_lowercase();
            (d.days, d.all.clone(), d.calendars.clone())
        };
        self.set(days, events, calendars);
    }

    pub fn shown(&self) -> usize {
        self.data.borrow().events.len()
    }

    pub fn scroll_to_hour(&self, hour: u32) {
        let adj = self.scroll.vadjustment();
        let target = hour as f64 * HOUR;
        glib::idle_add_local_once(move || {
            adj.set_value(target.min((adj.upper() - adj.page_size()).max(0.0)));
        });
    }

    fn redraw(&self) {
        self.header.queue_draw();
        self.grid.queue_draw();
    }
}

fn visible(events: &[Event], filter: &str) -> Vec<Event> {
    events
        .iter()
        .filter(|e| filter.is_empty() || e.title.to_lowercase().contains(filter) || e.location.to_lowercase().contains(filter))
        .cloned()
        .collect()
}

struct Colors {
    bg: Rgb,
    text: Rgb,
    dim: Rgb,
    accent: Rgb,
    on_accent: Rgb,
    line: Rgb,
}

fn colors() -> Colors {
    let p = nixe_theme::palette();
    let bg = Rgb::hex(&p.bg);
    let accent = Rgb::hex(&p.accent);
    Colors {
        bg,
        text: Rgb::hex(&p.text),
        dim: Rgb::hex(&p.dim_text),
        accent,
        on_accent: if p.light { Rgb(1.0, 1.0, 1.0) } else { bg },
        line: bg.mix(Rgb::hex(&p.border), 0.82),
    }
}

fn set(cr: &gtk::cairo::Context, c: Rgb) {
    cr.set_source_rgb(c.0, c.1, c.2);
}

fn set_a(cr: &gtk::cairo::Context, c: Rgb, a: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, a);
}

fn column_width(width: f64) -> f64 {
    ((width - GUTTER) / 7.0).max(10.0)
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

/// Text in a box, ellipsized; `scale` relative to the window font, `bold` 600.
#[allow(clippy::too_many_arguments)]
fn text(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, s: &str, x: f64, y: f64, w: f64, scale: f64, bold: bool, strike: bool) -> f64 {
    let layout = area.create_pango_layout(Some(s));
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrFloat::new_scale(scale));
    if bold {
        attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Semibold));
    }
    if strike {
        attrs.insert(gtk::pango::AttrInt::new_strikethrough(true));
    }
    layout.set_attributes(Some(&attrs));
    layout.set_width((w.max(1.0) * gtk::pango::SCALE as f64) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    layout.pixel_size().1 as f64
}

fn event_color(d: &Data, event: &Event, fallback: Rgb) -> Rgb {
    d.calendars.get(&event.calendar).and_then(|(_, c)| c.as_deref()).map(Rgb::hex).unwrap_or(fallback)
}

fn draw_header(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, d: &mut Data) {
    let c = colors();
    let col = column_width(width);
    let today = Local::now().date_naive();
    d.header_hits.clear();
    for (i, day) in d.days.iter().enumerate() {
        let x = GUTTER + i as f64 * col;
        set(cr, c.dim);
        text(area, cr, &day.format("%a").to_string().to_uppercase(), x + 8.0, 6.0, col - 12.0, 0.76, true, false);
        let number = day.day().to_string();
        if *day == today {
            set(cr, c.accent);
            rounded(cr, x + 5.0, 22.0, 30.0, 20.0, 6.0);
            let _ = cr.fill();
            set(cr, c.on_accent);
        } else {
            set(cr, c.text);
        }
        text(area, cr, &number, x + 9.0, 23.0, 26.0, 1.0, true, false);
    }
    // Lines between days, through the all-day lanes.
    let height = area.content_height() as f64;
    set(cr, c.line);
    cr.set_line_width(1.0);
    for i in 0..=7 {
        let x = (GUTTER + i as f64 * col).floor() + 0.5;
        cr.move_to(x, DAY_HEADER);
        cr.line_to(x, height);
    }
    cr.move_to(0.0, height - 0.5);
    cr.line_to(width, height - 0.5);
    let _ = cr.stroke();
    set(cr, c.dim);
    text(area, cr, "all day", 6.0, DAY_HEADER + 4.0, GUTTER - 10.0, 0.72, false, false);

    let bars = d.layout.all_day.clone();
    for bar in bars {
        let event = &d.events[bar.event];
        let color = event_color(d, event, c.accent);
        let x = GUTTER + bar.first_day as f64 * col + 3.0;
        let w = (bar.last_day - bar.first_day + 1) as f64 * col - 6.0;
        let y = DAY_HEADER + bar.lane as f64 * LANE + 2.0;
        let h = LANE - 4.0;
        set_a(cr, color, if event.cancelled { 0.12 } else { 0.28 });
        rounded(cr, x, y, w, h, 4.0);
        let _ = cr.fill();
        set(cr, c.text);
        text(area, cr, &event.title, x + 6.0, y + 1.0, w - 10.0, 0.86, false, event.cancelled);
        d.header_hits.push((x, y, w, h, bar.event));
    }
}

fn draw_grid(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, d: &mut Data) {
    let c = colors();
    let col = column_width(width);
    let now = Local::now().naive_local();
    let twelve = prefs::twelve_hour();

    if let Some(i) = d.days.iter().position(|day| *day == now.date()) {
        set_a(cr, c.accent, 0.05);
        cr.rectangle(GUTTER + i as f64 * col, 0.0, col, 24.0 * HOUR);
        let _ = cr.fill();
    }
    cr.set_line_width(1.0);
    for h in 0..24 {
        let y = h as f64 * HOUR;
        set(cr, c.line);
        cr.move_to(GUTTER - 6.0, y.floor() + 0.5);
        cr.line_to(width, y.floor() + 0.5);
        let _ = cr.stroke();
        if h > 0 {
            set(cr, c.dim);
            let label = hour_label(h, twelve);
            text(area, cr, &label, 4.0, y - 8.0, GUTTER - 14.0, 0.76, false, false);
        }
    }
    set(cr, c.line);
    for i in 0..=7 {
        let x = (GUTTER + i as f64 * col).floor() + 0.5;
        cr.move_to(x, 0.0);
        cr.line_to(x, 24.0 * HOUR);
    }
    let _ = cr.stroke();

    d.grid_hits.clear();
    let placed = d.layout.timed.clone();
    for p in placed {
        let event = &d.events[p.event];
        let color = event_color(d, event, c.accent);
        let slot = col / p.columns as f64;
        let x = GUTTER + p.day as f64 * col + p.column as f64 * slot + 2.0;
        let w = slot - 4.0;
        let y = p.top as f64 / 60.0 * HOUR + 1.0;
        let h = (p.bottom - p.top) as f64 / 60.0 * HOUR - 2.0;
        let fill = c.bg.mix(color, if event.cancelled { 0.12 } else { 0.26 });
        set(cr, fill);
        rounded(cr, x, y, w, h, 4.0);
        let _ = cr.fill();
        set(cr, color);
        cr.rectangle(x, y + 1.0, 3.0, (h - 2.0).max(1.0));
        let _ = cr.fill();
        cr.save().ok();
        cr.rectangle(x, y, w, h);
        cr.clip();
        set(cr, c.text);
        let used = text(area, cr, &event.title, x + 8.0, y + 2.0, w - 12.0, 0.86, true, event.cancelled);
        if h > used + 14.0 {
            set(cr, c.dim);
            let when = match event.span {
                Span::Timed { start, end } => time_range(start, end, twelve),
                Span::AllDay { .. } => String::new(),
            };
            let detail = if event.location.is_empty() { when } else { format!("{when} · {}", event.location) };
            text(area, cr, &detail, x + 8.0, y + 2.0 + used, w - 12.0, 0.76, false, false);
        }
        cr.restore().ok();
        d.grid_hits.push((x, y, w, h, p.event));
    }

    if let Some(i) = d.days.iter().position(|day| *day == now.date()) {
        let y = (now.hour() as f64 + now.minute() as f64 / 60.0) * HOUR;
        let x = GUTTER + i as f64 * col;
        set(cr, c.accent);
        cr.set_line_width(2.0);
        cr.move_to(x, y);
        cr.line_to(x + col, y);
        let _ = cr.stroke();
        cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}

fn hour_label(h: u32, twelve: bool) -> String {
    if !twelve {
        return format!("{h:02}:00");
    }
    match h {
        0 => "12 AM".into(),
        12 => "Noon".into(),
        h if h < 12 => format!("{h} AM"),
        h => format!("{} PM", h - 12),
    }
}

fn time_text(t: NaiveTime, twelve: bool) -> String {
    if twelve {
        let h = t.hour() % 12;
        let h = if h == 0 { 12 } else { h };
        let m = if t.minute() == 0 { String::new() } else { format!(":{:02}", t.minute()) };
        format!("{h}{m} {}", if t.hour() < 12 { "AM" } else { "PM" })
    } else {
        t.format("%H:%M").to_string()
    }
}

fn time_range(start: NaiveDateTime, end: NaiveDateTime, twelve: bool) -> String {
    if start == end {
        return time_text(start.time(), twelve);
    }
    format!("{} – {}", time_text(start.time(), twelve), time_text(end.time(), twelve))
}

fn show_details(area: &gtk::DrawingArea, d: &Data, index: usize, rect: (f64, f64, f64, f64)) {
    let Some(event) = d.events.get(index) else { return };
    let twelve = prefs::twelve_hour();
    let card = widgets::vbox(6);
    card.add_css_class("event-details");
    let title = widgets::label(&event.title, "event-title");
    title.set_wrap(true);
    title.set_xalign(0.0);
    card.append(&title);
    let when = match event.span {
        Span::AllDay { first, last } if first == last => first.format("%A, %B %-d").to_string(),
        Span::AllDay { first, last } => format!("{} – {}", first.format("%a, %b %-d"), last.format("%a, %b %-d")),
        Span::Timed { start, end } if start.date() == end.date() => {
            format!("{} · {}", start.format("%A, %B %-d"), time_range(start, end, twelve))
        }
        Span::Timed { start, end } => format!(
            "{} {} – {} {}",
            start.format("%a, %b %-d"),
            time_text(start.time(), twelve),
            end.format("%a, %b %-d"),
            time_text(end.time(), twelve)
        ),
    };
    let mut lines = vec![when];
    if event.repeats {
        lines.push("Repeats".into());
    }
    if event.cancelled {
        lines.push("Cancelled".into());
    }
    if !event.location.is_empty() {
        lines.push(event.location.clone());
    }
    if let Some((name, _)) = d.calendars.get(&event.calendar) {
        lines.push(name.clone());
    }
    for line in lines {
        let l = widgets::label(&line, "dim");
        l.set_wrap(true);
        l.set_xalign(0.0);
        card.append(&l);
    }
    let popover = gtk::Popover::new();
    popover.set_child(Some(&card));
    popover.set_parent(area);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(rect.0 as i32, rect.1 as i32, rect.2 as i32, rect.3.max(1.0) as i32)));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}
