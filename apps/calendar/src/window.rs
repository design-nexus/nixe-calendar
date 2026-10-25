//! The Calendar window. The sidebar follows Music: it does not grow, and it
//! collapses to icons under 980 px or when the user collapses it.

use crate::actions;
use crate::agenda::AgendaView;
use crate::days::DaysView;
use crate::details;
use crate::draw::{Calendars, Hooks};
use crate::minical::MiniCalendar;
use crate::month::MonthView;
use crate::prefs;
use crate::quick;
use crate::settings_dialog;
use crate::widgets;
use crate::worker::{self, Update};
use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use nixe_calendar_core::{CalendarInfo, Event, month_weeks, week_days, week_start};
use nixe_theme::Rgb;
use nixe_ui::chrome;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const SHORTCUTS: &[(&[&str], &str)] = &[
    (&["N"], "New event"),
    (&["T"], "Go to today"),
    (&["J"], "Next day, week or month"),
    (&["K"], "Previous day, week or month"),
    (&["D"], "Day view"),
    (&["W"], "Week view"),
    (&["M"], "Month view"),
    (&["Y"], "Year view"),
    (&["A"], "Agenda"),
    (&["E"], "Edit the open event"),
    (&["Delete"], "Delete the open event"),
    (&["R"], "Refresh calendars"),
    (&["Ctrl", "F"], "Find events (Enter searches the year)"),
    (&["Ctrl", "I"], "Import an .ics file"),
    (&["Esc"], "Clear the search"),
    (&["Ctrl", "B"], "Collapse or expand sidebar"),
    (&["Ctrl", ","], "Settings"),
    (&["F1"], "Show keyboard shortcuts"),
    (&["Ctrl", "Q"], "Close"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Day,
    Week,
    Month,
    Year,
    Agenda,
}

impl ViewKind {
    fn id(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::Agenda => "agenda",
        }
    }

    fn from_id(id: &str) -> Self {
        match id {
            "day" => Self::Day,
            "month" => Self::Month,
            "year" => Self::Year,
            "agenda" => Self::Agenda,
            _ => Self::Week,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
            Self::Agenda => "Agenda",
        }
    }
}

const AGENDA_DAYS: i64 = 30;
const SEARCH_DAYS: i64 = 365;

struct Ui {
    /// Reminders due in the range (the overlay), and their lists.
    reminders: Vec<Event>,
    task_lists: Vec<worker::TaskList>,
    window: gtk::ApplicationWindow,
    overlay: gtk::Overlay,
    nav: gtk::Box,
    nav_list: gtk::Box,
    search: gtk::SearchEntry,
    banners: gtk::Box,
    title: gtk::Label,
    switcher: gtk::Box,
    new_button: gtk::Button,
    pages: gtk::Stack,
    days: DaysView,
    month: MonthView,
    year: crate::year::YearView,
    agenda: AgendaView,
    mini: MiniCalendar,
    view: ViewKind,
    anchor: NaiveDate,
    range: (NaiveDate, NaiveDate),
    calendars: Vec<CalendarInfo>,
    events: Vec<Event>,
    me: Vec<String>,
    /// A year-wide search: the query and what it found.
    results: Option<(String, Vec<Event>)>,
    unavailable: Option<String>,
    loaded: bool,
    refreshing: bool,
}

thread_local! {
    static UI: RefCell<Option<Rc<RefCell<Ui>>>> = const { RefCell::new(None) };
    static NARROW: Cell<bool> = const { Cell::new(false) };
    static TOAST: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
    static STATUS: RefCell<Option<gtk::Label>> = const { RefCell::new(None) };
}

fn ui() -> Option<Rc<RefCell<Ui>>> {
    UI.with(|u| u.borrow().clone())
}

pub fn window() -> Option<gtk::ApplicationWindow> {
    ui().map(|u| u.borrow().window.clone())
}

pub fn overlay() -> Option<gtk::Overlay> {
    ui().map(|u| u.borrow().overlay.clone())
}

pub fn calendars() -> Vec<CalendarInfo> {
    ui().map(|u| u.borrow().calendars.clone()).unwrap_or_default()
}

/// The user's own addresses, for invitations.
pub fn me() -> Vec<String> {
    ui().map(|u| u.borrow().me.clone()).unwrap_or_default()
}

/// The calendar new events go to: the last one used, else the first writable shown one.
pub fn default_calendar() -> String {
    let list = calendars();
    let chosen = prefs::get().new_events_calendar;
    if list.iter().any(|c| c.uid == chosen && !c.read_only) {
        return chosen;
    }
    let last = prefs::get().last_calendar;
    if list.iter().any(|c| c.uid == last && !c.read_only) {
        return last;
    }
    list.iter()
        .find(|c| c.selected && !c.read_only)
        .or_else(|| list.iter().find(|c| !c.read_only))
        .map(|c| c.uid.clone())
        .unwrap_or_default()
}

pub fn remember_calendar(uid: &str) {
    prefs::update(|p| p.last_calendar = uid.to_string());
}

pub fn present(app: &gtk::Application, section: Option<&str>) {
    let first = ui().is_none();
    if first {
        build(app);
        worker::start();
        let view = ViewKind::from_id(&prefs::get().open_view);
        set_view(view);
    }
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    let snapshot = std::env::var_os("CALENDAR_SNAPSHOT");
    if snapshot.is_some() {
        window.set_default_size(env_num("CALENDAR_SNAPSHOT_W", 1120), env_num("CALENDAR_SNAPSHOT_H", 800));
    }
    window.present();
    if first {
        let days = ui.borrow().days.clone();
        days.scroll_to_hour(prefs::get().day_starts);
    }
    match section {
        Some("settings") => settings_dialog::open(),
        Some(id @ ("day" | "week" | "month" | "year" | "agenda")) => set_view(ViewKind::from_id(id)),
        _ => {}
    }
    if let Some(out) = snapshot {
        snapshot_and_quit(app, std::path::PathBuf::from(out));
    }
}

fn env_num(key: &str, default: i32) -> i32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Renders the window to a PNG and quits (layout checks: `CALENDAR_SNAPSHOT=out.png`;
/// `CALENDAR_SNAPSHOT_DATE`, `_VIEW`, `_W`, `_H`, `_DELAY`).
fn snapshot_and_quit(app: &gtk::Application, out: std::path::PathBuf) {
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    let nav = ui.borrow().nav.clone();
    let (w, h) = (env_num("CALENDAR_SNAPSHOT_W", 1120), env_num("CALENDAR_SNAPSHOT_H", 800));
    window.set_opacity(0.01);
    window.set_title(Some("Calendar snapshot"));
    if let Ok(view) = std::env::var("CALENDAR_SNAPSHOT_VIEW") {
        set_view(ViewKind::from_id(&view));
    }
    if let Some(date) = std::env::var("CALENDAR_SNAPSHOT_DATE").ok().and_then(|d| d.parse::<NaiveDate>().ok()) {
        go_to(date);
    }
    window.set_default_size(w, h);
    window.set_size_request(w, h);
    let app = app.clone();
    let delay = env_num("CALENDAR_SNAPSHOT_DELAY", 2500) as u64;
    glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
        let (w, h, nav_w, compact) = (window.width(), window.height(), nav.width(), nav.has_css_class("compact"));
        if let Some(child) = window.child() {
            let paintable = gtk::WidgetPaintable::new(Some(&child));
            let snapshot = gtk::Snapshot::new();
            snapshot.append_color(&gdk::RGBA::BLACK, &gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32));
            paintable.snapshot(&snapshot, w as f64, h as f64);
            if let (Some(node), Some(renderer)) = (snapshot.to_node(), window.renderer())
                && let Err(e) = renderer.render_texture(node, None).save_to_png(&out)
            {
                eprintln!("snapshot failed: {e}");
            }
        }
        println!("snapshot {w}x{h} nav={nav_w} compact={compact} -> {}", out.display());
        app.quit();
    });
}

/// Applies what the worker found.
pub fn apply(update: Update) {
    let Some(ui) = ui() else { return };
    match update {
        Update::Unavailable(text) => {
            let mut u = ui.borrow_mut();
            u.unavailable = Some(text);
            u.loaded = true;
        }
        Update::Calendars(list) => {
            let mut u = ui.borrow_mut();
            u.unavailable = None;
            u.loaded = true;
            u.calendars = list;
        }
        Update::Identities(me) => {
            ui.borrow_mut().me = me;
            return;
        }
        Update::Events { from, to, events, problems } => {
            for p in problems {
                toast(&p);
            }
            if (from, to) != ui.borrow().range {
                return;
            }
            ui.borrow_mut().events = events;
        }
    }
    redraw();
}

pub(crate) fn redraw() {
    rebuild_nav();
    rebuild_banners();
    show_view();
    refresh_status();
}

fn calendar_map(list: &[CalendarInfo]) -> Calendars {
    list.iter().map(|c| (c.uid.clone(), (c.name.clone(), c.color.clone()))).collect()
}

fn filtered(events: &[Event], query: &str) -> Vec<Event> {
    let q = query.trim().to_lowercase();
    let (declined, me) = (prefs::get().show_declined, me());
    events
        .iter()
        .filter(|e| declined || e.invitation(&me).is_none_or(|a| a.response != nixe_eds::Response::Declined))
        .filter(|e| {
            q.is_empty()
                || e.title.to_lowercase().contains(&q)
                || e.location.to_lowercase().contains(&q)
                || e.description.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

/// Draws the current view from what's loaded.
fn show_view() {
    let Some(ui) = ui() else { return };
    let u = ui.borrow();
    let mut calendars = calendar_map(&u.calendars);
    for (uid, name, color) in &u.task_lists {
        calendars.insert(uid.clone(), (name.clone(), color.clone()));
    }
    let query = u.search.text().to_string();
    let empty = u.loaded && u.unavailable.is_none() && u.calendars.is_empty();
    if empty {
        u.pages.set_visible_child_name("empty");
        return;
    }
    if let Some((q, found)) = &u.results {
        let today = Local::now().date_naive();
        u.agenda.set(today - Duration::days(SEARCH_DAYS), 2 * SEARCH_DAYS, found, &calendars, &format!("Nothing matches “{q}”"));
        u.pages.set_visible_child_name("agenda");
        let results = format!("Results for “{q}”");
        u.title.set_text(&results);
        u.title.set_tooltip_text(Some(&results));
        return;
    }
    let mut all = u.events.clone();
    if prefs::get().show_reminders {
        all.extend(u.reminders.iter().cloned());
        all.sort_by_key(|e| e.sort_key());
    }
    let events = filtered(&all, &query);
    match u.view {
        ViewKind::Day | ViewKind::Week => {
            let days: Vec<NaiveDate> = (0..(u.range.1 - u.range.0).num_days()).map(|i| u.range.0 + Duration::days(i)).collect();
            u.days.set(days, &events, calendars);
            u.pages.set_visible_child_name("days");
        }
        ViewKind::Month => {
            u.month.set(u.anchor.month(), month_weeks(u.anchor, prefs::first_weekday()), &events, calendars);
            u.pages.set_visible_child_name("month");
        }
        ViewKind::Year => {
            u.year.set(u.anchor.year(), &events);
            u.pages.set_visible_child_name("year");
        }
        ViewKind::Agenda => {
            u.agenda.set(u.range.0, (u.range.1 - u.range.0).num_days(), &events, &calendars, "Nothing in the next 30 days");
            u.pages.set_visible_child_name("agenda");
        }
    }
    set_view_title(&u);
}

/// The toolbar title; narrow windows get the short form, with the full one as its tooltip.
fn set_view_title(u: &Ui) {
    let full = view_title(u.view, u.anchor, u.range);
    if NARROW.with(|n| n.get()) {
        u.title.set_text(&short_title(u.view, u.anchor, u.range));
        u.title.set_tooltip_text(Some(&full));
    } else {
        u.title.set_text(&full);
        u.title.set_tooltip_text(None);
    }
}

/// Fits beside the view switcher in an 800 px window: "Oct 2026", "Oct – Nov", "Sat, Oct 24".
fn short_title(view: ViewKind, anchor: NaiveDate, (from, to): (NaiveDate, NaiveDate)) -> String {
    let last = to - Duration::days(1);
    match view {
        ViewKind::Day => anchor.format("%a, %b %-d").to_string(),
        ViewKind::Month => anchor.format("%b %Y").to_string(),
        ViewKind::Year => anchor.format("%Y").to_string(),
        ViewKind::Week | ViewKind::Agenda => {
            if from.month() == last.month() && from.year() == last.year() {
                from.format("%b %Y").to_string()
            } else {
                format!("{} – {}", from.format("%b"), last.format("%b"))
            }
        }
    }
}

fn view_title(view: ViewKind, anchor: NaiveDate, (from, to): (NaiveDate, NaiveDate)) -> String {
    let last = to - Duration::days(1);
    match view {
        ViewKind::Day => anchor.format("%A, %B %-d, %Y").to_string(),
        ViewKind::Month => anchor.format("%B %Y").to_string(),
        ViewKind::Year => anchor.format("%Y").to_string(),
        ViewKind::Week | ViewKind::Agenda => {
            if from.month() == last.month() && view == ViewKind::Week {
                from.format("%B %Y").to_string()
            } else if from.year() == last.year() {
                format!("{} – {}", from.format("%b %-d"), last.format("%b %-d, %Y"))
            } else {
                format!("{} – {}", from.format("%b %-d, %Y"), last.format("%b %-d, %Y"))
            }
        }
    }
}

fn range_for(view: ViewKind, anchor: NaiveDate) -> (NaiveDate, NaiveDate) {
    match view {
        ViewKind::Day => (anchor, anchor + Duration::days(1)),
        ViewKind::Week => {
            let s = week_start(anchor, prefs::first_weekday());
            (s, week_days(s)[6] + Duration::days(1))
        }
        ViewKind::Month => {
            let rows = month_weeks(anchor, prefs::first_weekday());
            let first = rows.first().map(|r| r[0]).unwrap_or(anchor);
            let last = rows.last().map(|r| r[6]).unwrap_or(anchor);
            (first, last + Duration::days(1))
        }
        ViewKind::Agenda => (anchor, anchor + Duration::days(AGENDA_DAYS)),
        ViewKind::Year => {
            let first = NaiveDate::from_ymd_opt(anchor.year(), 1, 1).unwrap_or(anchor);
            (first, NaiveDate::from_ymd_opt(anchor.year() + 1, 1, 1).unwrap_or(anchor))
        }
    }
}

/// Shows `date` in the current view.
pub fn go_to(date: NaiveDate) {
    let Some(ui) = ui() else { return };
    let view = ui.borrow().view;
    let range = range_for(view, date);
    {
        let mut u = ui.borrow_mut();
        if u.range != range {
            u.events.clear();
        }
        u.anchor = date;
        u.range = range;
        u.results = None;
    }
    worker::show_range(range.0, range.1);
    refresh_reminders();
    let mini = ui.borrow().mini.clone();
    mini.show(range.0, range.1, date);
    show_view();
    refresh_status();
}

pub fn go_to_today() {
    go_to(Local::now().date_naive());
}

pub(crate) fn set_view(view: ViewKind) {
    let Some(ui) = ui() else { return };
    let anchor = ui.borrow().anchor;
    ui.borrow_mut().view = view;
    prefs::update(|p| p.view = view.id().to_string());
    chrome::set_title(&ui.borrow().window, "Calendar", view.name());
    let switcher = ui.borrow().switcher.clone();
    select_segment(&switcher, view.id());
    go_to(anchor);
}

fn select_segment(switcher: &gtk::Box, id: &str) {
    let ids = ["day", "week", "month", "year", "agenda"];
    let mut child = switcher.first_child();
    let mut i = 0;
    while let Some(widget) = child {
        if let Some(toggle) = widget.downcast_ref::<gtk::ToggleButton>()
            && ids.get(i) == Some(&id)
            && !toggle.is_active()
        {
            toggle.set_active(true);
        }
        i += 1;
        child = widget.next_sibling();
    }
}

fn shift(step: i64) {
    let Some(ui) = ui() else { return };
    let (view, anchor) = (ui.borrow().view, ui.borrow().anchor);
    let date = match view {
        ViewKind::Day => anchor + Duration::days(step),
        ViewKind::Week => anchor + Duration::weeks(step),
        ViewKind::Agenda => anchor + Duration::days(step * 7),
        ViewKind::Year => anchor.with_year(anchor.year() + step as i32).unwrap_or(anchor),
        ViewKind::Month => {
            let first = anchor.with_day(1).unwrap_or(anchor);
            if step < 0 {
                first.checked_sub_months(Months::new(step.unsigned_abs() as u32)).unwrap_or(first)
            } else {
                first.checked_add_months(Months::new(step as u32)).unwrap_or(first)
            }
        }
    };
    go_to(date);
}

fn refresh() {
    let Some(ui) = ui() else { return };
    if ui.borrow().refreshing {
        return;
    }
    ui.borrow_mut().refreshing = true;
    refresh_status();
    worker::refresh(|problems| {
        if let Some(state) = self::ui() {
            state.borrow_mut().refreshing = false;
        }
        if problems.is_empty() {
            toast("Calendars refreshed.");
        } else {
            for p in problems {
                toast(&p);
            }
        }
        refresh_status();
    });
}

/// Searches every shown calendar a year either side of today.
fn search_year(query: String) {
    if query.trim().is_empty() {
        return;
    }
    let today = Local::now().date_naive();
    let q = query.clone();
    worker::ask(
        move || worker::events_between(today - Duration::days(SEARCH_DAYS), today + Duration::days(SEARCH_DAYS)),
        move |events| {
            let Some(ui) = ui() else { return };
            let found = filtered(&events, &q);
            ui.borrow_mut().results = Some((q, found));
            show_view();
            refresh_status();
        },
    );
}

fn refresh_status() {
    let Some(ui) = ui() else { return };
    let u = ui.borrow();
    let mut parts = Vec::new();
    let count = match &u.results {
        Some((_, found)) => found.len(),
        None => filtered(&u.events, &u.search.text()).len(),
    };
    if u.view == ViewKind::Week && u.results.is_none() {
        parts.push(format!("Week {}", u.range.0.iso_week().week()));
    }
    parts.push(match count {
        0 => "No events".into(),
        1 => "1 event".into(),
        n => format!("{n} events"),
    });
    if u.refreshing {
        parts.push("Refreshing…".into());
    } else if u.calendars.iter().any(|c| c.selected && c.offline) {
        parts.push("Offline".into());
    }
    STATUS.with(|s| {
        if let Some(label) = s.borrow().as_ref() {
            label.set_text(&parts.join(" · "));
        }
    });
}

pub fn open_online_accounts() {
    let settings = nixe_accounts::find_bin("nixe-settings");
    let started =
        settings.is_some_and(|bin| std::process::Command::new(bin).args(["--section", "online-accounts"]).spawn().is_ok());
    if !started && nixe_accounts::open_management().is_err() {
        toast("Install Nixe Settings or gnome-online-accounts-gtk to add accounts.");
    }
}

/// Quick create from the toolbar's + (or N).
pub(crate) fn new_event() {
    let Some(ui) = ui() else { return };
    let button = ui.borrow().new_button.clone();
    let rect = gdk::Rectangle::new(0, 0, button.width(), button.height());
    quick::show(button.upcast_ref(), rect, None);
}

// ---------- Banners ----------

fn rebuild_banners() {
    let Some(ui) = ui() else { return };
    let u = ui.borrow();
    let banners = u.banners.clone();
    while let Some(child) = banners.first_child() {
        banners.remove(&child);
    }
    if let Some(text) = &u.unavailable {
        banners.append(&widgets::banner(&glib::markup_escape_text(text), true));
    }
    let mut asked = Vec::new();
    for cal in u.calendars.iter().filter(|c| c.needs_sign_in) {
        let group = cal.group_label().to_string();
        if asked.contains(&group) {
            continue;
        }
        let banner = widgets::banner(&format!("<b>{}</b> needs you to sign in again.", glib::markup_escape_text(&group)), true);
        let sign_in = gtk::Button::with_label("Sign in");
        sign_in.set_valign(gtk::Align::Center);
        sign_in.connect_clicked(|_| open_online_accounts());
        banner.append(&sign_in);
        banners.append(&banner);
        asked.push(group);
    }
    if u.calendars.iter().any(|c| c.selected && c.offline) {
        banners.append(&widgets::banner("Offline. Showing your saved calendars; changes sync when you're back online.", false));
    }
    banners.set_visible(banners.first_child().is_some());
}

// ---------- Sidebar ----------

fn nav_button(icon: &str, title: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("nav-item");
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&gtk::Image::from_icon_name(icon));
    let label = widgets::label(title, "nav-label");
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class("compact-hide");
    content.append(&label);
    button.set_child(Some(&content));
    button.set_tooltip_text(Some(tooltip));
    button
}

/// A calendar's color: a filled square when shown, an outline when hidden.
fn swatch(color: Option<&str>, shown: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(16);
    area.set_content_height(16);
    area.set_valign(gtk::Align::Center);
    let color = color.map(Rgb::hex);
    area.set_draw_func(move |_, cr, w, h| {
        let c = color.unwrap_or_else(|| Rgb::hex(&nixe_theme::palette().accent));
        cr.set_source_rgb(c.0, c.1, c.2);
        let (x, y, s) = (w as f64 / 2.0 - 6.0, h as f64 / 2.0 - 6.0, 12.0);
        if shown {
            cr.rectangle(x, y, s, s);
            let _ = cr.fill();
        } else {
            cr.set_line_width(1.5);
            cr.rectangle(x + 0.75, y + 0.75, s - 1.5, s - 1.5);
            let _ = cr.stroke();
        }
    });
    area
}

fn calendar_button(cal: &CalendarInfo) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("nav-item");
    button.add_css_class("calendar-item");
    if !cal.selected {
        button.add_css_class("hidden-calendar");
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&swatch(cal.color.as_deref(), cal.selected));
    let label = widgets::label(&cal.name, "nav-label");
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.add_css_class("compact-hide");
    content.append(&label);
    if cal.offline && cal.selected {
        let offline = gtk::Image::from_icon_name("network-offline-symbolic");
        offline.add_css_class("compact-hide");
        offline.add_css_class("dim");
        offline.set_tooltip_text(Some("Offline: showing the saved copy"));
        content.append(&offline);
    } else if cal.read_only {
        let lock = gtk::Image::from_icon_name("changes-prevent-symbolic");
        lock.add_css_class("compact-hide");
        lock.add_css_class("dim");
        lock.set_tooltip_text(Some("Read only"));
        content.append(&lock);
    }
    button.set_child(Some(&content));
    let verb = if cal.selected { "Hide" } else { "Show" };
    button.set_tooltip_text(Some(&format!("{verb} {}", cal.name)));
    let uid = cal.uid.clone();
    let selected = cal.selected;
    button.connect_clicked(move |_| worker::set_selected(uid.clone(), !selected));
    crate::menus::attach_calendar(&button, cal);
    button
}

fn group_label(text: &str, add: bool) -> gtk::Widget {
    let label = widgets::label(&text.to_uppercase(), "nav-group");
    label.add_css_class("compact-hide");
    if !add {
        return label.upcast();
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("compact-hide");
    label.set_hexpand(true);
    label.set_xalign(0.0);
    row.append(&label);
    let plus = widgets::icon_button("list-add-symbolic", "Add an account");
    plus.add_css_class("flat");
    plus.add_css_class("nav-group-add");
    plus.set_valign(gtk::Align::End);
    plus.connect_clicked(|_| open_online_accounts());
    row.append(&plus);
    row.upcast()
}

fn rebuild_nav() {
    let Some(ui) = ui() else { return };
    let list = ui.borrow().nav_list.clone();
    let mini = ui.borrow().mini.root.clone();
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    list.append(&mini);

    let calendars = ui.borrow().calendars.clone();
    // Calendar Sets: show a group of calendars together.
    if !calendars.is_empty() {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("compact-hide");
        let label = widgets::label("SETS", "nav-group");
        label.add_css_class("first");
        label.set_hexpand(true);
        label.set_xalign(0.0);
        row.append(&label);
        let plus = widgets::icon_button("list-add-symbolic", "Save the shown calendars as a set");
        plus.add_css_class("flat");
        plus.add_css_class("nav-group-add");
        plus.set_valign(gtk::Align::End);
        plus.connect_clicked(|_| save_set());
        row.append(&plus);
        list.append(&row);
        let shown: Vec<String> = calendars.iter().filter(|c| c.selected).map(|c| c.uid.clone()).collect();
        let all = calendars.iter().all(|c| c.selected);
        let everything = nav_button("view-app-grid-symbolic", "All calendars", "Show every calendar");
        if all {
            everything.add_css_class("active");
        }
        everything.connect_clicked(|_| show_set(None));
        list.append(&everything);
        for set in prefs::get().sets {
            let mut want = set.calendars.clone();
            want.retain(|u| calendars.iter().any(|c| c.uid == *u));
            want.sort();
            let mut now = shown.clone();
            now.sort();
            let button = nav_button("view-list-symbolic", &set.name, &format!("Show {}", set.name));
            if !all && want == now {
                button.add_css_class("active");
            }
            crate::menus::attach_set(&button, &set);
            let set = set.clone();
            button.connect_clicked(move |_| show_set(Some(&set)));
            list.append(&button);
        }
    }
    let mut groups: Vec<(String, Vec<CalendarInfo>)> = Vec::new();
    for cal in calendars {
        let label = cal.group_label().to_string();
        match groups.iter_mut().find(|(g, _)| *g == label) {
            Some((_, items)) => items.push(cal),
            None => groups.push((label, vec![cal])),
        }
    }
    for (i, (label, items)) in groups.iter().enumerate() {
        list.append(&group_label(label, i == 0));
        for cal in items {
            list.append(&calendar_button(cal));
        }
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    list.append(&spacer);
    let today = nav_button("x-office-calendar-symbolic", "Today", "Go to today (T)");
    today.connect_clicked(|_| go_to_today());
    list.append(&today);
    let accounts = nav_button("system-users-symbolic", "Online accounts", "Add or sign in to calendar accounts in Settings");
    accounts.connect_clicked(|_| open_online_accounts());
    list.append(&accounts);

    let nav = ui.borrow().nav.clone();
    apply_compact(&nav, NARROW.with(|n| n.get()) || prefs::get().sidebar_collapsed);
}

// ---------- Building ----------

fn hooks() -> Rc<Hooks> {
    Rc::new(Hooks {
        event: Box::new(details::show),
        empty: Box::new(|widget, rect, slot| quick::show(widget, rect, Some(slot))),
        day: Box::new(|date| {
            set_view(ViewKind::Day);
            go_to(date);
        }),
        event_menu: Box::new(crate::menus::event),
        empty_menu: Box::new(crate::menus::empty),
    })
}

fn toolbar() -> (gtk::Box, gtk::Label, gtk::Box, gtk::Button) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.add_css_class("toolbar");
    bar.add_css_class("list-toolbar");
    let prev = widgets::icon_button("go-previous-symbolic", "Previous (K)");
    prev.connect_clicked(|_| shift(-1));
    let today = widgets::icon_button("go-home-symbolic", "Today (T)");
    today.connect_clicked(|_| go_to_today());
    let next = widgets::icon_button("go-next-symbolic", "Next (J)");
    next.connect_clicked(|_| shift(1));
    let title = widgets::label("", "toolbar-title");
    title.set_margin_start(8);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title.set_xalign(0.0);
    let switcher = widgets::segmented(
        &widgets::opts(&[("day", "Day"), ("week", "Week"), ("month", "Month"), ("year", "Year"), ("agenda", "Agenda")]),
        &prefs::get().view,
        |id| {
            if ui().is_some_and(|u| u.borrow().view.id() != id) {
                set_view(ViewKind::from_id(&id));
            }
        },
    );
    switcher.set_tooltip_text(Some("Day (D), Week (W), Month (M), Year (Y), Agenda (A)"));
    let add = widgets::icon_button("list-add-symbolic", "New event (N)");
    add.add_css_class("suggested-action");
    add.connect_clicked(|_| new_event());
    let reload = widgets::icon_button("view-refresh-symbolic", "Refresh (R)");
    reload.connect_clicked(|_| refresh());
    for w in [
        prev.upcast_ref::<gtk::Widget>(),
        today.upcast_ref(),
        next.upcast_ref(),
        title.upcast_ref(),
        switcher.upcast_ref(),
        add.upcast_ref(),
        reload.upcast_ref(),
    ] {
        bar.append(w);
    }
    (bar, title, switcher, add)
}

fn build(app: &gtk::Application) {
    let window =
        gtk::ApplicationWindow::builder().application(app).title("Calendar").default_width(1120).default_height(800).build();
    window.add_css_class("calendar-window");
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_icon_name(Some("io.github.design_nexus.nixe.Calendar"));
    nixe_ui::keys::install(
        &window,
        nixe_ui::keys::Common {
            settings: Some(settings_dialog::open),
            shortcuts: Some(show_shortcuts),
            close: true,
            ..Default::default()
        },
    );

    let nav = gtk::Box::new(gtk::Orientation::Vertical, 0);
    nav.add_css_class("settings-navigation");
    nav.set_hexpand(false);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.set_hexpand(false);
    let nav_scroll = gtk::ScrolledWindow::builder()
        .hexpand(false)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(&list)
        .build();
    let head = chrome::sidebar_head("Calendar", Some("Find events (Ctrl+F)"));
    head.toggle.connect_clicked(|_| toggle_sidebar());
    nav.append(&head.root);
    nav.append(&nav_scroll);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Find events — Enter searches the whole year"));
    search.set_visible(false);
    search.connect_search_changed(|_| {
        if let Some(ui) = ui() {
            ui.borrow_mut().results = None;
        }
        show_view();
        refresh_status();
    });
    search.connect_activate(|entry| search_year(entry.text().to_string()));
    search.connect_stop_search(|entry| {
        entry.set_text("");
        entry.set_visible(false);
    });
    if let Some(find) = &head.search {
        let entry = search.clone();
        find.connect_clicked(move |_| {
            if entry.is_visible() && entry.text().is_empty() {
                entry.set_visible(false);
            } else {
                entry.set_visible(true);
                entry.grab_focus();
            }
        });
    }

    let banners = gtk::Box::new(gtk::Orientation::Vertical, 8);
    banners.add_css_class("banners");
    banners.set_visible(false);
    let (bar, title, switcher, new_button) = toolbar();

    let hooks = hooks();
    let days = DaysView::new(hooks.clone());
    let month = MonthView::new(hooks.clone());
    let agenda = AgendaView::new(hooks);
    let mini = MiniCalendar::new(go_to);
    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.add_named(&days.root, Some("days"));
    pages.add_named(&month.root, Some("month"));
    let year = crate::year::YearView::new(|day| {
        set_view(ViewKind::Day);
        go_to(day);
    });
    pages.add_named(&year.root, Some("year"));
    pages.add_named(&agenda.root, Some("agenda"));
    let empty = widgets::empty_state(
        "x-office-calendar-symbolic",
        "No calendars yet",
        "Add a Google, Nextcloud or other account in Settings → Online Accounts, and its calendars show up here.",
        Some(("Add an account", Box::new(open_online_accounts))),
    );
    pages.add_named(&empty, Some("empty"));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("settings-content");
    content.set_hexpand(true);
    content.append(&chrome::search_strip(&search));
    content.append(&banners);
    content.append(&bar);
    content.append(&pages);

    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.set_vexpand(true);
    body.append(&nav);
    body.append(&content);

    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("window-frame");
    frame.append(&body);
    frame.append(&status_bar());

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&frame));
    window.set_child(Some(&overlay));
    install_keys(&window, &search);

    let today = Local::now().date_naive();
    let ui = Ui {
        reminders: Vec::new(),
        task_lists: Vec::new(),
        window: window.clone(),
        overlay: overlay.clone(),
        nav: nav.clone(),
        nav_list: list,
        search,
        banners,
        title,
        switcher,
        new_button,
        pages,
        days,
        month,
        year,
        agenda,
        mini,
        view: ViewKind::from_id(&prefs::get().view),
        anchor: today,
        range: (today, today),
        calendars: Vec::new(),
        events: Vec::new(),
        me: Vec::new(),
        results: None,
        unavailable: None,
        loaded: false,
        refreshing: false,
    };
    UI.with(|slot| *slot.borrow_mut() = Some(Rc::new(RefCell::new(ui))));

    let apply_width = {
        let nav = nav.clone();
        let window = window.clone();
        move || {
            let width = if window.width() > 0 { window.width() } else { window.default_width() };
            let narrow = width > 0 && width < 980;
            settings_dialog::fit(&window);
            let prev = NARROW.with(|n| n.get());
            let sized = nav.has_css_class("sized");
            if narrow == prev && sized {
                return;
            }
            nav.add_css_class("sized");
            NARROW.with(|n| n.set(narrow));
            apply_compact(&nav, narrow || prefs::get().sidebar_collapsed);
            if let Some(ui) = self::ui()
                && let Ok(u) = ui.try_borrow()
                && u.results.is_none()
            {
                set_view_title(&u);
            }
        }
    };
    let aw = apply_width.clone();
    window.connect_realize(move |_| aw());
    let probe = gtk::DrawingArea::new();
    probe.set_can_target(false);
    probe.set_can_focus(false);
    overlay.add_overlay(&probe);
    overlay.set_measure_overlay(&probe, false);
    probe.connect_resize(move |_, _, _| {
        let aw = apply_width.clone();
        glib::idle_add_local_once(aw);
    });
    redraw();
}

fn status_bar() -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    bar.add_css_class("status-bar");
    bar.append(&chrome::shortcuts_button(show_shortcuts));
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bar.append(&spacer);
    let readout = widgets::label("", "status-readout");
    readout.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    STATUS.with(|s| *s.borrow_mut() = Some(readout.clone()));
    bar.append(&readout);
    bar.append(&chrome::status_action("document-open-symbolic", "Import an .ics file (Ctrl+I)", actions::import_dialog));
    bar.append(&chrome::gear(settings_dialog::open));
    bar
}

fn apply_compact(nav: &gtk::Box, compact: bool) {
    if let Some(head) = nav.first_child().and_downcast::<gtk::Box>() {
        chrome::set_head_compact(&head, compact);
    }
    if compact {
        nav.add_css_class("compact");
    } else {
        nav.remove_css_class("compact");
    }
    fn walk(widget: &gtk::Widget, compact: bool) {
        if widget.has_css_class("compact-hide") {
            widget.set_visible(!compact);
        }
        if widget.has_css_class("nav-item")
            && let Some(content) = widget.downcast_ref::<gtk::Button>().and_then(|b| b.child())
        {
            content.set_halign(if compact { gtk::Align::Center } else { gtk::Align::Fill });
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            walk(&next, compact);
            child = next.next_sibling();
        }
    }
    walk(nav.upcast_ref(), compact);
}

pub fn toggle_sidebar() {
    prefs::update(|p| p.sidebar_collapsed = !p.sidebar_collapsed);
    let Some(ui) = ui() else { return };
    let nav = ui.borrow().nav.clone();
    apply_compact(&nav, NARROW.with(|n| n.get()) || prefs::get().sidebar_collapsed);
}

fn typing(window: &gtk::Window) -> bool {
    let Some(focus) = gtk::prelude::GtkWindowExt::focus(window) else { return false };
    focus.is::<gtk::Text>() || focus.is::<gtk::TextView>() || focus.ancestor(gtk::Entry::static_type()).is_some()
}

fn install_keys(window: &gtk::ApplicationWindow, search: &gtk::SearchEntry) {
    let keys = gtk::EventControllerKey::new();
    let search = search.clone();
    let win = window.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        if ctrl {
            match key {
                gdk::Key::f => {
                    search.set_visible(true);
                    search.grab_focus();
                }
                gdk::Key::b => toggle_sidebar(),
                gdk::Key::i => actions::import_dialog(),
                gdk::Key::n => new_event(),
                _ => return glib::Propagation::Proceed,
            }
            return glib::Propagation::Stop;
        }
        if typing(win.upcast_ref()) {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::t => go_to_today(),
            gdk::Key::j | gdk::Key::Right => shift(1),
            gdk::Key::k | gdk::Key::Left => shift(-1),
            gdk::Key::d => set_view(ViewKind::Day),
            gdk::Key::w => set_view(ViewKind::Week),
            gdk::Key::m => set_view(ViewKind::Month),
            gdk::Key::a => set_view(ViewKind::Agenda),
            gdk::Key::y => set_view(ViewKind::Year),
            gdk::Key::n => new_event(),
            gdk::Key::r => refresh(),
            gdk::Key::slash => {
                search.set_visible(true);
                search.grab_focus();
            }
            gdk::Key::Escape if search.is_visible() => {
                search.set_text("");
                search.set_visible(false);
            }
            gdk::Key::F1 => show_shortcuts(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);
}

pub fn show_shortcuts() {
    let (dialog, card) = widgets::dialog("Keyboard shortcuts", 520);
    let list = widgets::vbox(0);
    list.add_css_class("group-list");
    for (keys, what) in SHORTCUTS {
        list.append(&widgets::row(what, "", Some(widgets::key_caps(keys).upcast_ref())));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .propagate_natural_height(true)
        .max_content_height(560)
        .child(&list)
        .build();
    card.append(&scroll);
    let close = gtk::Button::with_label("Close");
    close.set_halign(gtk::Align::End);
    let dialog2 = dialog.clone();
    close.connect_clicked(move |_| dialog2.close());
    card.append(&close);
    dialog.present();
}

/// Opens a web link (a video call, a map) in the default app.
pub fn open_link(uri: &str) {
    if !uri.starts_with("https://") {
        return;
    }
    gtk::UriLauncher::new(uri).launch(window().as_ref(), gtk::gio::Cancellable::NONE, |r| {
        if let Err(e) = r {
            toast(&format!("Couldn't open the link: {}", e.message()));
        }
    });
}

pub fn toast(message: &str) {
    show_toast(message, None);
}

/// A toast with Undo; it stays 6 s.
pub fn toast_undo(message: &str, undo: impl Fn() + 'static) {
    show_toast(message, Some(Box::new(undo)));
}

fn show_toast(message: &str, undo: Option<Box<dyn Fn()>>) {
    let Some(ui) = ui() else {
        eprintln!("nixe-calendar: {message}");
        return;
    };
    let overlay = ui.borrow().overlay.clone();
    if let Some(old) = TOAST.with(|t| t.borrow_mut().take())
        && old.parent().is_some()
    {
        overlay.remove_overlay(&old);
    }
    let label = gtk::Label::new(Some(message));
    label.set_wrap(true);
    label.set_max_width_chars(70);
    let bx = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bx.add_css_class("toast");
    bx.set_halign(gtk::Align::Center);
    bx.set_valign(gtk::Align::End);
    bx.set_margin_bottom(48);
    bx.append(&label);
    let lasts = if undo.is_some() { 6000 } else { 3500 };
    if let Some(undo) = undo {
        let button = gtk::Button::with_label("Undo");
        let (o, b) = (overlay.clone(), bx.clone());
        button.connect_clicked(move |_| {
            undo();
            if b.parent().is_some() {
                o.remove_overlay(&b);
            }
        });
        bx.append(&button);
    }
    overlay.add_overlay(&bx);
    TOAST.with(|t| *t.borrow_mut() = Some(bx.clone()));
    glib::timeout_add_local_once(std::time::Duration::from_millis(lasts), move || {
        if bx.parent().is_some() {
            overlay.remove_overlay(&bx);
        }
    });
}

/// Shows a set's calendars and hides the rest (every calendar for `None`).
/// Which calendars show is EDS's, so other calendar apps follow.
pub(crate) fn show_set(set: Option<&prefs::CalendarSet>) {
    for cal in calendars() {
        let want = set.is_none_or(|s| s.calendars.contains(&cal.uid));
        if want != cal.selected {
            worker::set_selected(cal.uid.clone(), want);
        }
    }
}

/// Names the calendars shown now as a set.
fn save_set() {
    let shown: Vec<String> = calendars().into_iter().filter(|c| c.selected).map(|c| c.uid).collect();
    if shown.is_empty() {
        toast("Show the calendars for the set first.");
        return;
    }
    let (dialog, card) = widgets::dialog("New set", 420);
    let note = widgets::label(
        &format!(
            "The {} shown now, together.",
            if shown.len() == 1 { "calendar".to_string() } else { format!("{} calendars", shown.len()) }
        ),
        "dim",
    );
    note.set_xalign(0.0);
    card.append(&note);
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Work, Family…"));
    card.append(&name);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    let cancel = gtk::Button::with_label("Not now");
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    buttons.append(&cancel);
    buttons.append(&save);
    card.append(&buttons);
    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());
    let (d, n) = (dialog.clone(), name.clone());
    let done = move || {
        let text = n.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let calendars = shown.clone();
        prefs::update(|p| {
            p.sets.retain(|s| s.name != text);
            p.sets.push(prefs::CalendarSet { name: text.clone(), calendars });
        });
        rebuild_nav();
        d.close();
    };
    let again = done.clone();
    save.connect_clicked(move |_| done());
    name.connect_activate(move |_| again());
    dialog.present();
    name.grab_focus();
}

pub fn refresh_nav() {
    rebuild_nav();
}

/// Reloads the reminders overlay for the range shown (or clears it).
pub fn refresh_reminders() {
    let Some(ui) = ui() else { return };
    if !prefs::get().show_reminders {
        ui.borrow_mut().reminders.clear();
        show_view();
        return;
    }
    let range = ui.borrow().range;
    worker::ask(
        move || worker::reminders_between(range.0, range.1),
        move |(events, lists)| {
            let Some(ui) = self::ui() else { return };
            if ui.borrow().range != range {
                return;
            }
            {
                let mut u = ui.borrow_mut();
                u.reminders = events;
                u.task_lists = lists;
            }
            show_view();
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn short_titles() {
        let week = |from: NaiveDate| (from, from + Duration::days(7));
        assert_eq!(short_title(ViewKind::Week, d(2026, 10, 14), week(d(2026, 10, 11))), "Oct 2026");
        assert_eq!(short_title(ViewKind::Week, d(2026, 10, 28), week(d(2026, 10, 26))), "Oct – Nov");
        assert_eq!(short_title(ViewKind::Week, d(2026, 12, 30), week(d(2026, 12, 27))), "Dec – Jan");
        assert_eq!(short_title(ViewKind::Day, d(2026, 10, 24), (d(2026, 10, 24), d(2026, 10, 25))), "Sat, Oct 24");
        assert_eq!(short_title(ViewKind::Month, d(2026, 9, 1), (d(2026, 8, 30), d(2026, 10, 11))), "Sep 2026");
    }
}
