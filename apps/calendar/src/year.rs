//! The year: twelve months, days with events marked. A day opens in the day
//! view.

use crate::draw::{self, Style};
use crate::prefs;
use crate::widgets;
use chrono::{Datelike, Local, NaiveDate};
use gtk::prelude::*;
use nyx_calendar_core::{Event, month_weeks};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

const CELL_H: f64 = 24.0;
const HEAD_H: f64 = 20.0;

#[derive(Clone)]
pub struct YearView {
    pub root: gtk::ScrolledWindow,
    months: Vec<(gtk::Label, gtk::DrawingArea)>,
    data: Rc<RefCell<(i32, HashSet<NaiveDate>)>>,
}

impl YearView {
    pub fn new(on_pick: impl Fn(NaiveDate) + 'static) -> Self {
        let on_pick = Rc::new(on_pick);
        let data = Rc::new(RefCell::new((Local::now().year(), HashSet::new())));
        let flow = gtk::FlowBox::new();
        flow.add_css_class("year-grid");
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_homogeneous(true);
        flow.set_min_children_per_line(2);
        flow.set_max_children_per_line(4);
        flow.set_column_spacing(24);
        flow.set_row_spacing(18);
        flow.set_valign(gtk::Align::Start);
        let mut months = Vec::new();
        for m in 1..=12u32 {
            let cell = gtk::Box::new(gtk::Orientation::Vertical, 4);
            cell.add_css_class("year-month");
            let title = widgets::label("", "year-month-title");
            title.set_xalign(0.0);
            cell.append(&title);
            let area = gtk::DrawingArea::new();
            area.set_content_height((HEAD_H + 6.0 * CELL_H) as i32);
            area.set_content_width(7 * 26);
            let d = data.clone();
            area.set_draw_func(move |area, cr, w, _| {
                let (year, busy) = &*d.borrow();
                if let Some(month) = NaiveDate::from_ymd_opt(*year, m, 1) {
                    draw_month(area, cr, w as f64, month, busy);
                }
            });
            let click = gtk::GestureClick::new();
            let (d, target, pick) = (data.clone(), area.clone(), on_pick.clone());
            click.connect_released(move |_, _, x, y| {
                let Some(month) = NaiveDate::from_ymd_opt(d.borrow().0, m, 1) else { return };
                let rows = month_weeks(month, prefs::first_weekday());
                let cw = target.width() as f64 / 7.0;
                let (col, row) = ((x / cw).floor(), ((y - HEAD_H) / CELL_H).floor());
                if y > HEAD_H && (0.0..7.0).contains(&col) && row >= 0.0 && (row as usize) < rows.len() {
                    let day = rows[row as usize][col as usize];
                    if day.month() == m {
                        pick(day);
                    }
                }
            });
            area.add_controller(click);
            cell.append(&area);
            flow.insert(&cell, -1);
            months.push((title, area));
        }
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("year-page");
        page.append(&flow);
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&page)
            .build();
        let view = Self { root, months, data };
        let v = view.clone();
        nyx_theme::subscribe(&view.root, move || v.redraw());
        view
    }

    /// Shows a year, marking the days that have events.
    pub fn set(&self, year: i32, events: &[Event]) {
        let mut busy = HashSet::new();
        for e in events.iter().filter(|e| !e.cancelled) {
            let (first, last) = e.days();
            let mut day = first;
            while day <= last && day.year() <= year {
                busy.insert(day);
                match day.succ_opt() {
                    Some(next) => day = next,
                    None => break,
                }
            }
        }
        *self.data.borrow_mut() = (year, busy);
        for (i, (title, _)) in self.months.iter().enumerate() {
            if let Some(month) = NaiveDate::from_ymd_opt(year, i as u32 + 1, 1) {
                title.set_text(&month.format("%B").to_string());
            }
        }
        self.redraw();
    }

    fn redraw(&self) {
        for (_, area) in &self.months {
            area.queue_draw();
        }
    }
}

fn draw_month(area: &gtk::DrawingArea, cr: &gtk::cairo::Context, width: f64, month: NaiveDate, busy: &HashSet<NaiveDate>) {
    let c = draw::colors();
    let rows = month_weeks(month, prefs::first_weekday());
    let cw = width / 7.0;
    let today = Local::now().date_naive();
    if let Some(first) = rows.first() {
        for (i, day) in first.iter().enumerate() {
            draw::set(cr, c.dim);
            let letter: String = day.format("%a").to_string().chars().take(2).collect();
            draw::text(area, cr, &letter, i as f64 * cw, 2.0, cw, Style { scale: 0.7, bold: true, center: true, ..draw::NORMAL });
        }
    }
    for (r, row) in rows.iter().enumerate() {
        let y = HEAD_H + r as f64 * CELL_H;
        for (i, day) in row.iter().enumerate() {
            if day.month() != month.month() {
                continue;
            }
            let x = i as f64 * cw;
            if *day == today {
                draw::set(cr, c.accent);
                draw::rounded(cr, x + (cw - 22.0) / 2.0, y + 2.0, 22.0, CELL_H - 4.0, 6.0);
                let _ = cr.fill();
                draw::set(cr, c.on_accent);
            } else {
                draw::set(cr, c.text);
            }
            draw::text(
                area,
                cr,
                &day.day().to_string(),
                x,
                y + 4.0,
                cw,
                Style { scale: 0.8, bold: *day == today, center: true, ..draw::NORMAL },
            );
            if busy.contains(day) && *day != today {
                draw::set(cr, c.accent);
                cr.arc(x + cw / 2.0, y + CELL_H - 3.0, 1.8, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
        }
    }
}
