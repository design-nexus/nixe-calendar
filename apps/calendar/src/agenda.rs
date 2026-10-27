//! The agenda: the coming days with events, as a list. Also shows search results.

use crate::draw::{self, Calendars, Hooks};
use crate::widgets;
use chrono::{Local, NaiveDate};
use gtk::prelude::*;
use nyx_calendar_core::{Event, Span, agenda};
use nyx_theme::Rgb;
use std::rc::Rc;

#[derive(Clone)]
pub struct AgendaView {
    pub root: gtk::ScrolledWindow,
    list: gtk::Box,
    hooks: Rc<Hooks>,
}

impl AgendaView {
    pub fn new(hooks: Rc<Hooks>) -> Self {
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("agenda");
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::External)
            .vexpand(true)
            .child(&list)
            .build();
        Self { root, list, hooks }
    }

    /// Lists `days` days from `from`. `empty` is shown when nothing is there.
    pub fn set(&self, from: NaiveDate, days: i64, events: &[Event], calendars: &Calendars, empty: &str) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let groups = agenda(events, from, days);
        if groups.is_empty() {
            self.list.append(&widgets::empty_state("x-office-calendar-symbolic", empty, "", None));
            return;
        }
        let today = Local::now().date_naive();
        for group in groups {
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            head.add_css_class("agenda-day");
            let title = widgets::label(&group.day.format("%A, %B %-d").to_string().to_uppercase(), "group-title");
            title.set_xalign(0.0);
            head.append(&title);
            if group.day == today {
                head.append(&widgets::tag("Today", "accent"));
            }
            self.list.append(&head);
            let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
            card.add_css_class("group-list");
            card.add_css_class("agenda-card");
            for index in group.events {
                card.append(&self.row(&events[index], group.day, calendars));
            }
            self.list.append(&card);
        }
    }

    fn row(&self, event: &Event, day: NaiveDate, calendars: &Calendars) -> gtk::Button {
        let row = gtk::Button::new();
        row.add_css_class("agenda-row");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let time = match event.span {
            Span::AllDay { .. } => "All day".to_string(),
            Span::Timed { start, end } if start.date() == day && end.date() == day => draw::time_range(start, end),
            Span::Timed { start, .. } if start.date() == day => format!("From {}", draw::time_text(start.time())),
            Span::Timed { end, .. } if end.date() == day => format!("Until {}", draw::time_text(end.time())),
            Span::Timed { .. } => "All day".to_string(),
        };
        let when = widgets::label(&time, "agenda-time");
        when.set_xalign(0.0);
        when.set_width_chars(16);
        content.append(&when);
        let color = calendars.get(&event.calendar).and_then(|(_, c)| c.clone());
        let dot = gtk::DrawingArea::new();
        dot.set_content_width(10);
        dot.set_content_height(10);
        dot.set_valign(gtk::Align::Center);
        dot.set_draw_func(move |_, cr, _, _| {
            let c = color.as_deref().map(Rgb::hex).unwrap_or_else(|| Rgb::hex(&nyx_theme::palette().accent));
            draw::set(cr, c);
            cr.arc(5.0, 5.0, 4.5, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        });
        content.append(&dot);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        let title = widgets::label(&event.title, "agenda-title");
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if event.cancelled {
            title.add_css_class("cancelled");
        }
        text.append(&title);
        let mut meta = Vec::new();
        if !event.location.is_empty() {
            meta.push(event.location.clone());
        }
        if let Some((name, _)) = calendars.get(&event.calendar) {
            meta.push(name.clone());
        }
        if !meta.is_empty() {
            let m = widgets::label(&meta.join(" · "), "dim");
            m.set_xalign(0.0);
            m.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text.append(&m);
        }
        content.append(&text);
        row.set_child(Some(&content));
        let (hooks, event) = (self.hooks.clone(), event.clone());
        let (menu_hooks, menu_event) = (hooks.clone(), event.clone());
        row.connect_clicked(move |button| {
            let rect = gtk::gdk::Rectangle::new(0, 0, button.width(), button.height());
            (hooks.event)(button.upcast_ref(), rect, event.clone());
        });
        widgets::on_right_click(&row, move |w, x, y| (menu_hooks.event_menu)(w, x, y, menu_event.clone()));
        row
    }
}
