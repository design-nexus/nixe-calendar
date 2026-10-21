//! The EDS worker. One thread owns the source registry, the open calendars and
//! their live views, and runs its own GLib main context so EDS signals arrive
//! there. The window asks for things with the functions below and gets plain
//! data back through `window::apply` on the GTK thread.

use chrono::{Duration, NaiveDate};
use nixe_calendar_core::{CalendarInfo, Event, midnight};
use nixe_eds::{Calendar, Kind, Registry, View, Watch, range_query};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// What the worker tells the window.
#[derive(Debug)]
pub enum Update {
    /// EDS can't be reached; the text says why.
    Unavailable(String),
    Calendars(Vec<CalendarInfo>),
    Events { week: NaiveDate, events: Vec<Event>, problems: Vec<String> },
    /// Something went wrong that doesn't change what is shown.
    Problems(Vec<String>),
    /// A refresh the user asked for has finished.
    Refreshed,
}

#[derive(Default)]
struct State {
    registry: Option<Registry>,
    _watch: Option<Watch>,
    accounts: Option<nixe_accounts::Client>,
    open: HashMap<String, Calendar>,
    views: Vec<View>,
    /// The week and calendars the views watch.
    watching: Option<(NaiveDate, Vec<String>)>,
    week: Option<NaiveDate>,
    sent: Vec<CalendarInfo>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static PENDING_SOURCES: Cell<bool> = const { Cell::new(false) };
    static PENDING_EVENTS: Cell<bool> = const { Cell::new(false) };
}

static CTX: OnceLock<glib::MainContext> = OnceLock::new();

pub fn start() {
    let ctx = glib::MainContext::new();
    if CTX.set(ctx.clone()).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("calendar-eds".into())
        .spawn(move || {
            let main = ctx.clone();
            let _ = ctx.with_thread_default(move || {
                connect();
                glib::MainLoop::new(Some(&main), false).run();
            });
        })
        .expect("start the calendar worker");
}

/// Shows the week starting on `week`.
pub fn show_week(week: NaiveDate) {
    run(move || {
        STATE.with(|s| s.borrow_mut().week = Some(week));
        load_events();
    });
}

/// Shows or hides a calendar (saved by EDS, so other calendar apps agree).
pub fn set_selected(uid: String, selected: bool) {
    run(move || {
        let result = STATE.with(|s| s.borrow().registry.as_ref().map(|r| r.set_selected(&uid, Kind::Calendar, selected)));
        if let Some(Err(err)) = result {
            send(Update::Problems(vec![err.to_string()]));
        }
        schedule_sources();
    });
}

/// Asks every open calendar to sync with its server now.
pub fn refresh() {
    run(|| {
        let problems: Vec<String> = STATE.with(|s| {
            let s = s.borrow();
            s.open.iter().filter_map(|(uid, cal)| cal.refresh().err().map(|e| format!("{}: {e}", name_of(&s, uid)))).collect()
        });
        if problems.is_empty() {
            send(Update::Refreshed);
        } else {
            send(Update::Problems(problems));
        }
        schedule_sources();
    });
}

fn run(f: impl FnOnce() + Send + 'static) {
    if let Some(ctx) = CTX.get() {
        ctx.invoke(f);
    }
}

fn send(update: Update) {
    glib::MainContext::default().invoke(move || crate::window::apply(update));
}

/// Runs `f` on the worker after `delay`.
fn after(delay: std::time::Duration, f: impl FnOnce() + Send + 'static) {
    let mut f = Some(f);
    let source = glib::timeout_source_new(delay, None, glib::Priority::DEFAULT, move || {
        if let Some(f) = f.take() {
            f();
        }
        glib::ControlFlow::Break
    });
    source.attach(CTX.get());
}

fn connect() {
    match Registry::connect() {
        Ok(registry) => {
            let watch = registry.watch(schedule_sources);
            let accounts = nixe_accounts::Client::connect().ok();
            if let Some(client) = &accounts {
                client.watch(schedule_sources);
            }
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.registry = Some(registry);
                s._watch = Some(watch);
                s.accounts = accounts;
            });
            send_calendars(true);
            load_events();
            poll_online();
        }
        Err(err) => {
            let installed = std::path::Path::new("/usr/lib/evolution-source-registry").exists();
            let text = if installed {
                format!("Calendar data isn't answering ({err}). Trying again…")
            } else {
                "Install evolution-data-server to see your calendars.".to_string()
            };
            send(Update::Unavailable(text));
            after(std::time::Duration::from_secs(10), connect);
        }
    }
}

/// Re-sends the calendars when one goes online or offline.
fn poll_online() {
    after(std::time::Duration::from_secs(30), || {
        send_calendars(false);
        poll_online();
    });
}

fn schedule_sources() {
    if PENDING_SOURCES.with(|p| p.replace(true)) {
        return;
    }
    after(std::time::Duration::from_millis(250), || {
        PENDING_SOURCES.with(|p| p.set(false));
        send_calendars(true);
        load_events();
    });
}

fn schedule_events() {
    if PENDING_EVENTS.with(|p| p.replace(true)) {
        return;
    }
    after(std::time::Duration::from_millis(250), || {
        PENDING_EVENTS.with(|p| p.set(false));
        load_events();
    });
}

fn name_of(state: &State, uid: &str) -> String {
    state.sent.iter().find(|c| c.uid == uid).map(|c| c.name.clone()).unwrap_or_else(|| uid.to_string())
}

fn calendars(state: &State) -> Vec<CalendarInfo> {
    let Some(registry) = &state.registry else { return Vec::new() };
    let attention: HashSet<String> = state
        .accounts
        .as_ref()
        .and_then(|c| c.list().ok())
        .map(|list| list.into_iter().filter(|a| a.attention_needed).map(|a| a.id).collect())
        .unwrap_or_default();
    registry
        .sources(Kind::Calendar)
        .into_iter()
        .map(|s| CalendarInfo {
            name: s.label().to_string(),
            needs_sign_in: s.account.as_ref().is_some_and(|a| attention.contains(a)),
            offline: state.open.get(&s.uid).is_some_and(|c| !c.online()),
            uid: s.uid,
            color: s.color,
            selected: s.selected,
            group: s.group,
            account: s.account,
        })
        .collect()
}

/// Sends the calendar list, unless `only_changes` and nothing changed.
fn send_calendars(always: bool) {
    let list = STATE.with(|s| calendars(&s.borrow()));
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let changed = s.sent != list;
        s.sent = list.clone();
        changed
    });
    if always || changed {
        send(Update::Calendars(list));
    }
}

fn load_events() {
    let Some(week) = STATE.with(|s| s.borrow().week) else { return };
    let (from, to) = (midnight(week), midnight(week + Duration::days(7)));
    let mut problems = Vec::new();
    let mut events = Vec::new();
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = &mut *s;
        let Some(registry) = &state.registry else { return };
        let shown: Vec<CalendarInfo> = state.sent.iter().filter(|c| c.selected).cloned().collect();
        for info in &shown {
            if !state.open.contains_key(&info.uid) {
                match Calendar::open(registry, &info.uid, Kind::Calendar, None) {
                    Ok(cal) => {
                        state.open.insert(info.uid.clone(), cal);
                    }
                    Err(err) => {
                        problems.push(format!("{}: {err}", info.name));
                        continue;
                    }
                }
            }
            let cal = &state.open[&info.uid];
            events.extend(cal.instances(from, to).iter().filter_map(|item| Event::from_item(&info.uid, item)));
        }
        // Watch what is on screen, so syncs and other apps' changes show up.
        let uids: Vec<String> = shown.iter().map(|c| c.uid.clone()).filter(|u| state.open.contains_key(u)).collect();
        if state.watching.as_ref() != Some(&(week, uids.clone())) {
            state.views.clear();
            let query = range_query(from, to);
            for uid in &uids {
                match state.open[uid].watch(&query, schedule_events) {
                    Ok(view) => state.views.push(view),
                    Err(err) => problems.push(format!("{}: {err}", name_of(state, uid))),
                }
            }
            state.watching = Some((week, uids));
        }
        // Calendars no longer shown are closed.
        let keep: HashSet<&String> = shown.iter().map(|c| &c.uid).collect();
        state.open.retain(|uid, _| keep.contains(uid));
    });
    events.sort_by_key(|e| e.sort_key());
    send(Update::Events { week, events, problems });
    send_calendars(false);
}
