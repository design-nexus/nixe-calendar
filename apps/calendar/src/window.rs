//! The Calendar window. The sidebar follows Music: it does not grow, and it
//! collapses to icons under 980 px or when the user collapses it.

use crate::prefs;
use crate::settings_dialog;
use crate::week::WeekView;
use crate::widgets;
use crate::worker::{self, Update};
use chrono::{Datelike, Duration, Local, NaiveDate};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use nixe_calendar_core::{CalendarInfo, Event, week_days, week_start};
use nixe_theme::Rgb;
use nixe_ui::chrome;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const SHORTCUTS: &[(&[&str], &str)] = &[
    (&["T"], "Go to today"),
    (&["J"], "Next week"),
    (&["K"], "Previous week"),
    (&["R"], "Refresh calendars"),
    (&["Ctrl", "F"], "Find events this week"),
    (&["Esc"], "Clear the search"),
    (&["Ctrl", "B"], "Collapse or expand sidebar"),
    (&["Ctrl", ","], "Settings"),
    (&["F1"], "Show keyboard shortcuts"),
    (&["Ctrl", "Q"], "Close"),
];

struct Ui {
    window: gtk::ApplicationWindow,
    overlay: gtk::Overlay,
    nav: gtk::Box,
    nav_list: gtk::Box,
    banners: gtk::Box,
    title: gtk::Label,
    pages: gtk::Stack,
    week_view: WeekView,
    calendars: Vec<CalendarInfo>,
    events: Vec<Event>,
    week: NaiveDate,
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

pub fn present(app: &gtk::Application, section: Option<&str>) {
    let first = ui().is_none();
    if first {
        build(app);
        worker::start();
        go_to_today();
    }
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    let snapshot = std::env::var_os("CALENDAR_SNAPSHOT");
    if snapshot.is_some() {
        window.set_default_size(env_num("CALENDAR_SNAPSHOT_W", 1120), env_num("CALENDAR_SNAPSHOT_H", 800));
    }
    window.present();
    if first {
        let week_view = ui.borrow().week_view.clone();
        week_view.scroll_to_hour(prefs::get().day_starts);
    }
    if section == Some("settings") {
        settings_dialog::open();
    }
    if let Some(out) = snapshot {
        snapshot_and_quit(app, std::path::PathBuf::from(out));
    }
}

fn env_num(key: &str, default: i32) -> i32 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Renders the window to a PNG and quits (layout checks: `CALENDAR_SNAPSHOT=out.png`).
fn snapshot_and_quit(app: &gtk::Application, out: std::path::PathBuf) {
    let Some(ui) = ui() else { return };
    let window = ui.borrow().window.clone();
    let nav = ui.borrow().nav.clone();
    let (w, h) = (env_num("CALENDAR_SNAPSHOT_W", 1120), env_num("CALENDAR_SNAPSHOT_H", 800));
    window.set_opacity(0.01);
    window.set_title(Some("Calendar snapshot"));
    if let Some(date) = std::env::var("CALENDAR_SNAPSHOT_DATE").ok().and_then(|d| d.parse::<NaiveDate>().ok()) {
        show(week_start(date, prefs::first_weekday()));
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
            ui.borrow_mut().unavailable = Some(text);
            ui.borrow_mut().loaded = true;
        }
        Update::Calendars(list) => {
            let mut u = ui.borrow_mut();
            u.unavailable = None;
            u.loaded = true;
            u.calendars = list;
        }
        Update::Events { week, events, problems } => {
            for p in problems {
                toast(&p);
            }
            if week != ui.borrow().week {
                return;
            }
            ui.borrow_mut().events = events;
        }
        Update::Problems(problems) => {
            for p in problems {
                toast(&p);
            }
            ui.borrow_mut().refreshing = false;
        }
        Update::Refreshed => {
            ui.borrow_mut().refreshing = false;
            toast("Calendars refreshed.");
        }
    }
    redraw();
}

fn redraw() {
    rebuild_nav();
    rebuild_banners();
    show_week();
    refresh_status();
}

fn show_week() {
    let Some(ui) = ui() else { return };
    let u = ui.borrow();
    let days = week_days(u.week);
    let calendars = u.calendars.iter().map(|c| (c.uid.clone(), (c.name.clone(), c.color.clone()))).collect();
    u.week_view.set(days, u.events.clone(), calendars);
    u.title.set_text(&week_title(&days));
    let empty = u.loaded && u.unavailable.is_none() && u.calendars.is_empty();
    u.pages.set_visible_child_name(if empty { "empty" } else { "week" });
}

fn week_title(days: &[NaiveDate; 7]) -> String {
    let (a, b) = (days[0], days[6]);
    if a.month() == b.month() {
        a.format("%B %Y").to_string()
    } else if a.year() == b.year() {
        format!("{} – {}", a.format("%b"), b.format("%b %Y"))
    } else {
        format!("{} – {}", a.format("%b %Y"), b.format("%b %Y"))
    }
}

pub fn go_to_today() {
    show(week_start(Local::now().date_naive(), prefs::first_weekday()));
}

fn shift(weeks: i64) {
    let Some(ui) = ui() else { return };
    let week = ui.borrow().week + Duration::weeks(weeks);
    show(week);
}

fn show(week: NaiveDate) {
    let Some(ui) = ui() else { return };
    {
        let mut u = ui.borrow_mut();
        if u.week != week {
            u.events.clear();
        }
        u.week = week;
    }
    worker::show_week(week);
    show_week();
    refresh_status();
}

fn refresh() {
    let Some(ui) = ui() else { return };
    if ui.borrow().refreshing {
        return;
    }
    ui.borrow_mut().refreshing = true;
    refresh_status();
    worker::refresh();
}

fn refresh_status() {
    let Some(ui) = ui() else { return };
    let u = ui.borrow();
    let shown: Vec<&CalendarInfo> = u.calendars.iter().filter(|c| c.selected).collect();
    let count = u.week_view.shown();
    let mut parts = vec![format!("Week {}", u.week.iso_week().week())];
    parts.push(match count {
        0 => "No events".into(),
        1 => "1 event".into(),
        n => format!("{n} events"),
    });
    if u.refreshing {
        parts.push("Refreshing…".into());
    } else if shown.iter().any(|c| c.offline) {
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
    let started = settings.is_some_and(|bin| std::process::Command::new(bin).args(["--section", "online-accounts"]).spawn().is_ok());
    if !started && nixe_accounts::open_management().is_err() {
        toast("Install Nixe Settings or gnome-online-accounts-gtk to add accounts.");
    }
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
        banners.append(&widgets::banner("Offline. Showing your saved calendars; they update when you're back online.", false));
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
    }
    button.set_child(Some(&content));
    let verb = if cal.selected { "Hide" } else { "Show" };
    button.set_tooltip_text(Some(&format!("{verb} {}", cal.name)));
    let uid = cal.uid.clone();
    let selected = cal.selected;
    button.connect_clicked(move |_| worker::set_selected(uid.clone(), !selected));
    button
}

fn group_label(text: &str, first: bool, add: bool) -> gtk::Widget {
    let label = widgets::label(&text.to_uppercase(), "nav-group");
    label.add_css_class("compact-hide");
    if first {
        label.add_css_class("first");
    }
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
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    list.append(&group_label("Views", true, false));
    let week = nav_button("x-office-calendar-symbolic", "Week", "This week (T)");
    week.add_css_class("active");
    week.connect_clicked(|_| go_to_today());
    list.append(&week);

    let calendars = ui.borrow().calendars.clone();
    let mut groups: Vec<(String, Vec<CalendarInfo>)> = Vec::new();
    for cal in calendars {
        let label = cal.group_label().to_string();
        match groups.iter_mut().find(|(g, _)| *g == label) {
            Some((_, items)) => items.push(cal),
            None => groups.push((label, vec![cal])),
        }
    }
    for (i, (label, items)) in groups.iter().enumerate() {
        list.append(&group_label(label, false, i == 0));
        for cal in items {
            list.append(&calendar_button(cal));
        }
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    list.append(&spacer);
    let accounts = nav_button("system-users-symbolic", "Online accounts", "Add or sign in to calendar accounts in Settings");
    accounts.connect_clicked(|_| open_online_accounts());
    list.append(&accounts);

    let nav = ui.borrow().nav.clone();
    apply_compact(&nav, NARROW.with(|n| n.get()) || prefs::get().sidebar_collapsed);
}

// ---------- Building ----------

fn toolbar() -> (gtk::Box, gtk::Label) {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    bar.add_css_class("toolbar");
    bar.add_css_class("list-toolbar");
    let prev = widgets::icon_button("go-previous-symbolic", "Previous week (K)");
    prev.connect_clicked(|_| shift(-1));
    let today = widgets::icon_button("go-home-symbolic", "Today (T)");
    today.connect_clicked(|_| go_to_today());
    let next = widgets::icon_button("go-next-symbolic", "Next week (J)");
    next.connect_clicked(|_| shift(1));
    let title = widgets::label("", "toolbar-title");
    title.set_margin_start(8);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let reload = widgets::icon_button("view-refresh-symbolic", "Refresh (R)");
    reload.connect_clicked(|_| refresh());
    for w in [prev.upcast_ref::<gtk::Widget>(), today.upcast_ref(), next.upcast_ref(), title.upcast_ref(), spacer.upcast_ref(), reload.upcast_ref()] {
        bar.append(w);
    }
    (bar, title)
}

fn build(app: &gtk::Application) {
    let window = gtk::ApplicationWindow::builder().application(app).title("Calendar").default_width(1120).default_height(800).build();
    window.add_css_class("calendar-window");
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_icon_name(Some("io.github.design_nexus.nixe.Calendar"));
    chrome::set_title(&window, "Calendar", "Week");
    nixe_ui::keys::install(
        &window,
        nixe_ui::keys::Common { settings: Some(settings_dialog::open), shortcuts: Some(show_shortcuts), close: true, ..Default::default() },
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

    let week_view = WeekView::new();
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Find events this week"));
    search.set_visible(false);
    {
        let wv = week_view.clone();
        search.connect_search_changed(move |entry| {
            wv.set_filter(&entry.text());
            refresh_status();
        });
    }
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
    let (bar, title) = toolbar();

    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.add_named(&week_view.root, Some("week"));
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

    let ui = Ui {
        window: window.clone(),
        overlay: overlay.clone(),
        nav: nav.clone(),
        nav_list: list,
        banners,
        title,
        pages,
        week_view,
        calendars: Vec::new(),
        events: Vec::new(),
        week: week_start(Local::now().date_naive(), prefs::first_weekday()),
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
        if ctrl && key == gdk::Key::f {
            search.set_visible(true);
            search.grab_focus();
            return glib::Propagation::Stop;
        }
        if ctrl && key == gdk::Key::b {
            toggle_sidebar();
            return glib::Propagation::Stop;
        }
        if ctrl || typing(win.upcast_ref()) {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::t => go_to_today(),
            gdk::Key::j | gdk::Key::Right => shift(1),
            gdk::Key::k | gdk::Key::Left => shift(-1),
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

pub fn toast(message: &str) {
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
    overlay.add_overlay(&bx);
    TOAST.with(|t| *t.borrow_mut() = Some(bx.clone()));
    glib::timeout_add_local_once(std::time::Duration::from_millis(3500), move || {
        if bx.parent().is_some() {
            overlay.remove_overlay(&bx);
        }
    });
}
