//! The sidebar's month: pick a day, see where the views are.

use crate::draw::{self, Style};
use crate::prefs;
use crate::widgets;
use chrono::{Datelike, Local, Months, NaiveDate};
use gtk::prelude::*;
use nixe_calendar_core::month_weeks;
use std::cell::RefCell;
use std::rc::Rc;

const CELL_H: f64 = 26.0;
const HEAD_H: f64 = 20.0;

struct Data {
    month: NaiveDate,
    /// The days the main view shows.
    range: (NaiveDate, NaiveDate),
}

#[derive(Clone)]
pub struct MiniCalendar {
    pub root: gtk::Box,
    title: gtk::Label,
    area: gtk::DrawingArea,
    data: Rc<RefCell<Data>>,
}

impl MiniCalendar {
    pub fn new(on_pick: impl Fn(NaiveDate) + 'static) -> Self {
        let today = Local::now().date_naive();
        let data = Rc::new(RefCell::new(Data { month: today.with_day(1).unwrap_or(today), range: (today, today) }));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.add_css_class("mini-calendar");
        root.add_css_class("compact-hide");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let title = widgets::label("", "mini-title");
        title.set_hexpand(true);
        title.set_xalign(0.0);
        let prev = widgets::icon_button("go-previous-symbolic", "Previous month");
        let next = widgets::icon_button("go-next-symbolic", "Next month");
        for b in [&prev, &next] {
            b.add_css_class("flat");
            b.add_css_class("mini-nav");
        }
        head.append(&title);
        head.append(&prev);
        head.append(&next);
        root.append(&head);
        let area = gtk::DrawingArea::new();
        area.set_content_height((HEAD_H + 6.0 * CELL_H) as i32);
        root.append(&area);

        let d = data.clone();
        area.set_draw_func(move |area, cr, w, _| draw_mini(area, cr, w as f64, &d.borrow()));
        let click = gtk::GestureClick::new();
        let (d, target) = (data.clone(), area.clone());
        click.connect_released(move |_, _, x, y| {
            let rows = month_weeks(d.borrow().month, prefs::first_weekday());
            let cw = target.width() as f64 / 7.0;
            let (col, row) = ((x / cw).floor(), ((y - HEAD_H) / CELL_H).floor());
            if y > HEAD_H && (0.0..7.0).contains(&col) && row >= 0.0 && (row as usize) < rows.len() {
                on_pick(rows[row as usize][col as usize]);
            }
        });
        area.add_controller(click);

        let view = Self { root, title, area, data };
        for (button, months) in [(&prev, -1i32), (&next, 1)] {
            let v = view.clone();
            button.connect_clicked(move |_| v.shift(months));
        }
        let v = view.clone();
        nixe_theme::subscribe(&view.root, move || v.area.queue_draw());
        view.update_title();
        view
    }

    /// Marks the days the main view shows and turns to that month.
    pub fn show(&self, from: NaiveDate, to: NaiveDate, anchor: NaiveDate) {
        {
            let mut d = self.data.borrow_mut();
            d.range = (from, to);
            d.month = anchor.with_day(1).unwrap_or(anchor);
        }
        self.update_title();
        self.area.queue_draw();
    }

    fn shift(&self, months: i32) {
        {
            let mut d = self.data.borrow_mut();
            d.month = if months < 0 {
                d.month.checked_sub_months(Months::new(months.unsigned_abs())).unwrap_or(d.month)
            } else {
                d.month.checked_add_months(Months::new(months as u32)).unwrap_or(d.month)
            };
        }
        self.update_title();
        self.area.queue_draw();
    }

    fn update_title(&self) {
        self.title.set_text(&self.data.borrow().month.format("%B %Y").to_string());
    }
}

fn draw_mini(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, d: &Data) {
    let c = draw::colors();
    let rows = month_weeks(d.month, prefs::first_weekday());
    let cw = width / 7.0;
    let today = Local::now().date_naive();
    if let Some(first) = rows.first() {
        for (i, day) in first.iter().enumerate() {
            draw::set(cr, c.dim);
            let letter: String = day.format("%a").to_string().chars().take(2).collect();
            draw::text(area, cr, &letter, i as f64 * cw, 2.0, cw, Style { scale: 0.72, bold: true, center: true, ..draw::NORMAL });
        }
    }
    let (from, to) = d.range;
    for (r, row) in rows.iter().enumerate() {
        let y = HEAD_H + r as f64 * CELL_H;
        // The shown range as one band per row.
        let inside: Vec<usize> = (0..7).filter(|i| row[*i] >= from && row[*i] < to).collect();
        if let (Some(a), Some(b)) = (inside.first(), inside.last()) {
            draw::set(cr, c.bg.mix(c.accent, 0.18));
            draw::rounded(cr, *a as f64 * cw + 1.0, y + 2.0, (b - a + 1) as f64 * cw - 2.0, CELL_H - 4.0, 6.0);
            let _ = cr.fill();
        }
        for (i, day) in row.iter().enumerate() {
            let x = i as f64 * cw;
            if *day == today {
                draw::set(cr, c.accent);
                draw::rounded(cr, x + (cw - 24.0) / 2.0, y + 3.0, 24.0, CELL_H - 6.0, 6.0);
                let _ = cr.fill();
                draw::set(cr, c.on_accent);
            } else if day.month() != d.month.month() {
                draw::set(cr, c.dim);
            } else {
                draw::set(cr, c.text);
            }
            let style = Style { scale: 0.84, bold: *day == today, center: true, ..draw::NORMAL };
            draw::text(area, cr, &day.day().to_string(), x, y + 5.0, cw, style);
        }
    }
}
