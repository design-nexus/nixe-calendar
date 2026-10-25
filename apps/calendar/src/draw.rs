//! Drawing helpers shared by the views: theme colors, shapes, text, times.

use crate::prefs;
use chrono::{NaiveDateTime, NaiveTime, Timelike};
use gtk::prelude::*;
use nixe_calendar_core::{Event, Span};
use nixe_theme::Rgb;
use std::collections::HashMap;

/// A calendar's name and color, by uid.
pub type Calendars = HashMap<String, (String, Option<String>)>;

/// Where a click on empty space landed.
#[derive(Clone, Copy, Debug)]
pub enum Slot {
    At(NaiveDateTime),
    Day(chrono::NaiveDate),
}

/// A callback given the widget and rectangle to point a popover at.
pub type At<T> = Box<dyn Fn(&gtk::Widget, gtk::gdk::Rectangle, T)>;

/// What views report to the window.
pub struct Hooks {
    /// An event was clicked.
    pub event: At<Event>,
    /// Empty space was double-clicked.
    pub empty: At<Slot>,
    /// A day's number (or "+N more") was clicked.
    pub day: Box<dyn Fn(chrono::NaiveDate)>,
    /// An event was right-clicked, at a point in the widget.
    pub event_menu: Menu<Event>,
    /// Empty space was right-clicked.
    pub empty_menu: Menu<Slot>,
}

/// A callback given the widget and the point a menu opens at.
pub type Menu<T> = Box<dyn Fn(&gtk::Widget, f64, f64, T)>;

pub struct Colors {
    pub bg: Rgb,
    pub text: Rgb,
    pub dim: Rgb,
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub line: Rgb,
}

pub fn colors() -> Colors {
    let p = nixe_theme::palette();
    let bg = Rgb::hex(&p.bg);
    Colors {
        bg,
        text: Rgb::hex(&p.text),
        dim: Rgb::hex(&p.dim_text),
        accent: Rgb::hex(&p.accent),
        on_accent: if p.light { Rgb(1.0, 1.0, 1.0) } else { bg },
        line: bg.mix(Rgb::hex(&p.border), 0.82),
    }
}

pub fn set(cr: &gtk::cairo::Context, c: Rgb) {
    cr.set_source_rgb(c.0, c.1, c.2);
}

pub fn set_a(cr: &gtk::cairo::Context, c: Rgb, a: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, a);
}

pub fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

/// Text options for [`text`].
#[derive(Clone, Copy, Default)]
pub struct Style {
    pub scale: f64,
    pub bold: bool,
    pub strike: bool,
    /// Center within the width.
    pub center: bool,
}

pub const NORMAL: Style = Style { scale: 1.0, bold: false, strike: false, center: false };

/// Draws ellipsized text in a box of width `w`; returns its height.
pub fn text(area: &impl IsA<gtk::Widget>, cr: &gtk::cairo::Context, s: &str, x: f64, y: f64, w: f64, style: Style) -> f64 {
    let layout = area.create_pango_layout(Some(s));
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrFloat::new_scale(if style.scale == 0.0 { 1.0 } else { style.scale }));
    if style.bold {
        attrs.insert(gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Semibold));
    }
    if style.strike {
        attrs.insert(gtk::pango::AttrInt::new_strikethrough(true));
    }
    layout.set_attributes(Some(&attrs));
    layout.set_width((w.max(1.0) * gtk::pango::SCALE as f64) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if style.center {
        layout.set_alignment(gtk::pango::Alignment::Center);
    }
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    layout.pixel_size().1 as f64
}

/// Draws one ellipsized line of Pango markup.
#[allow(clippy::too_many_arguments)]
pub fn markup(
    area: &impl IsA<gtk::Widget>,
    cr: &gtk::cairo::Context,
    markup: &str,
    x: f64,
    y: f64,
    w: f64,
    scale: f64,
    strike: bool,
) {
    let layout = area.create_pango_layout(None);
    layout.set_markup(markup);
    let attrs = layout.attributes().unwrap_or_default();
    attrs.insert(gtk::pango::AttrFloat::new_scale(scale));
    if strike {
        attrs.insert(gtk::pango::AttrInt::new_strikethrough(true));
    }
    layout.set_attributes(Some(&attrs));
    layout.set_width((w.max(1.0) * gtk::pango::SCALE as f64) as i32);
    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}

pub fn event_color(calendars: &Calendars, event: &Event, fallback: Rgb) -> Rgb {
    calendars.get(&event.calendar).and_then(|(_, c)| c.as_deref()).map(Rgb::hex).unwrap_or(fallback)
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> gtk::gdk::Rectangle {
    gtk::gdk::Rectangle::new(x as i32, y as i32, w.max(1.0) as i32, h.max(1.0) as i32)
}

pub fn time_text(t: NaiveTime) -> String {
    if prefs::twelve_hour() {
        let h = t.hour() % 12;
        let h = if h == 0 { 12 } else { h };
        let m = if t.minute() == 0 { String::new() } else { format!(":{:02}", t.minute()) };
        format!("{h}{m} {}", if t.hour() < 12 { "AM" } else { "PM" })
    } else {
        t.format("%H:%M").to_string()
    }
}

pub fn hour_label(h: u32) -> String {
    if !prefs::twelve_hour() {
        return format!("{h:02}:00");
    }
    match h {
        0 => "12 AM".into(),
        12 => "Noon".into(),
        h if h < 12 => format!("{h} AM"),
        h => format!("{} PM", h - 12),
    }
}

pub fn time_range(start: NaiveDateTime, end: NaiveDateTime) -> String {
    if start == end {
        return time_text(start.time());
    }
    format!("{} – {}", time_text(start.time()), time_text(end.time()))
}

/// "Wed, Oct 21 · 2 – 3 PM", "Dec 24 – 25", ...
pub fn when_text(span: Span) -> String {
    match span {
        Span::AllDay { first, last } if first == last => first.format("%A, %B %-d").to_string(),
        Span::AllDay { first, last } => format!("{} – {}", first.format("%a, %b %-d"), last.format("%a, %b %-d")),
        Span::Timed { start, end } if start.date() == end.date() => {
            format!("{} · {}", start.format("%A, %B %-d"), time_range(start, end))
        }
        Span::Timed { start, end } => format!(
            "{} {} – {} {}",
            start.format("%a, %b %-d"),
            time_text(start.time()),
            end.format("%a, %b %-d"),
            time_text(end.time())
        ),
    }
}
