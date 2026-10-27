//! The EDS worker. One thread owns the source registry, the open calendars and
//! their live views, and runs its own GLib main context so EDS signals arrive
//! there. The window asks for things with the functions below and gets plain
//! data back on the GTK thread.

use chrono::NaiveDate;
use nyx_calendar_core::{CalendarInfo, Event, Span, midnight};
use nyx_eds::{Calendar, Kind, Registry, View, Watch, range_query};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// What the worker tells the window on its own.
#[derive(Debug)]
pub enum Update {
    /// EDS can't be reached; the text says why.
    Unavailable(String),
    Calendars(Vec<CalendarInfo>),
    /// The user's own addresses, for finding them among attendees.
    Identities(Vec<String>),
    /// Online accounts whose calendars EDS can't load: its backend for them
    /// isn't installed.
    Unsupported(Vec<String>),
    Events {
        from: NaiveDate,
        to: NaiveDate,
        events: Vec<Event>,
        problems: Vec<String>,
    },
}

#[derive(Default)]
struct State {
    registry: Option<Registry>,
    _watch: Option<Watch>,
    accounts: Option<nyx_accounts::Client>,
    open: HashMap<String, Calendar>,
    /// Task lists opened for the reminders overlay.
    tasks: HashMap<String, Calendar>,
    views: Vec<View>,
    /// The range and calendars the views watch.
    watching: Option<(NaiveDate, NaiveDate, Vec<String>)>,
    range: Option<(NaiveDate, NaiveDate)>,
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

/// Shows the days `from` up to (not including) `to`.
pub fn show_range(from: NaiveDate, to: NaiveDate) {
    run(move || {
        STATE.with(|s| s.borrow_mut().range = Some((from, to)));
        load_events();
    });
}

/// Runs `work` on the worker and hands its result to `reply` on the GTK thread.
pub fn ask<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static, reply: impl FnOnce(T) + 'static) {
    let reply = glib::thread_guard::ThreadGuard::new(reply);
    run(move || {
        let value = work();
        glib::MainContext::default().invoke(move || (reply.into_inner())(value));
    });
}

/// Runs `f` with an open calendar (opening it if needed). For use inside [`ask`].
pub fn with_calendar<T>(uid: &str, f: impl FnOnce(&Calendar) -> Result<T, nyx_eds::Error>) -> Result<T, String> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = &mut *s;
        let Some(registry) = &state.registry else { return Err("Calendar data isn't available.".to_string()) };
        if !state.open.contains_key(uid) {
            let cal = Calendar::open(registry, uid, Kind::Calendar, None).map_err(|e| e.to_string())?;
            state.open.insert(uid.to_string(), cal);
        }
        f(&state.open[uid]).map_err(|e| e.to_string())
    })
}

/// A task list for the reminders overlay: uid, name, color.
pub type TaskList = (String, String, Option<String>);

/// Open reminders due between two dates in the shown task lists, as events in
/// a `task:<list>` calendar (a due time lasts 30 minutes; a due day is all
/// day). For the views only. For use inside [`ask`].
pub fn reminders_between(from: NaiveDate, to: NaiveDate) -> (Vec<Event>, Vec<TaskList>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = &mut *s;
        let Some(registry) = &state.registry else { return (Vec::new(), Vec::new()) };
        let lists: Vec<nyx_eds::Source> = registry.sources(Kind::TaskList).into_iter().filter(|l| l.selected).collect();
        let query = format!(
            r#"(and (due-in-time-range? (make-time "{}") (make-time "{}")) (not is-completed?))"#,
            stamp(midnight(from)),
            stamp(midnight(to))
        );
        let mut events = Vec::new();
        for list in &lists {
            if !state.tasks.contains_key(&list.uid) {
                match Calendar::open(registry, &list.uid, Kind::TaskList, Some(10)) {
                    Ok(cal) => {
                        state.tasks.insert(list.uid.clone(), cal);
                    }
                    Err(_) => continue,
                }
            }
            let Ok(items) = state.tasks[&list.uid].items(&query) else { continue };
            for item in items {
                let span = match item.due {
                    Some(nyx_eds::When::Date { year, month, day }) => {
                        let Some(d) = NaiveDate::from_ymd_opt(year, month, day) else { continue };
                        Span::AllDay { first: d, last: d }
                    }
                    Some(nyx_eds::When::Time(t)) => {
                        let Some(at) =
                            chrono::DateTime::from_timestamp(t, 0).map(|t| t.with_timezone(&chrono::Local).naive_local())
                        else {
                            continue;
                        };
                        Span::Timed { start: at, end: at + chrono::Duration::minutes(30) }
                    }
                    None => continue,
                };
                events.push(Event {
                    calendar: format!("task:{}", list.uid),
                    uid: item.uid.clone(),
                    rid: None,
                    title: format!("☐ {}", item.summary),
                    location: String::new(),
                    span,
                    repeats: item.repeats,
                    cancelled: false,
                    description: item.description.clone(),
                    url: None,
                    rule: None,
                    alarms: Vec::new(),
                    organizer: None,
                    attendees: Vec::new(),
                });
            }
        }
        let infos = lists.into_iter().map(|l| (format!("task:{}", l.uid), l.name, l.color)).collect();
        (events, infos)
    })
}

/// EDS's UTC time stamp form.
fn stamp(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y%m%dT%H%M%SZ").to_string()).unwrap_or_default()
}

/// Every occurrence in every shown calendar between two dates. For use inside [`ask`].
pub fn events_between(from: NaiveDate, to: NaiveDate) -> Vec<Event> {
    let shown: Vec<String> = STATE.with(|s| s.borrow().sent.iter().filter(|c| c.selected).map(|c| c.uid.clone()).collect());
    let (a, b) = (midnight(from), midnight(to));
    let mut out = Vec::new();
    for uid in shown {
        if let Ok(items) = with_calendar(&uid, |cal| Ok(cal.instances(a, b))) {
            out.extend(items.iter().filter_map(|item| Event::from_item(&uid, item)));
        }
    }
    out.sort_by_key(|e| e.sort_key());
    out
}

/// Shows or hides a calendar (saved by EDS, so other calendar apps agree).
pub fn set_selected(uid: String, selected: bool) {
    run(move || {
        let _ = STATE.with(|s| s.borrow().registry.as_ref().map(|r| r.set_selected(&uid, Kind::Calendar, selected)));
        schedule_sources();
    });
}

/// Asks every open calendar to sync with its server now; replies with problems.
/// Switches a calendar on or off for every app.
pub fn set_enabled(uid: String, enabled: bool, reply: impl FnOnce(Result<(), String>) + 'static) {
    ask(
        move || {
            let r = STATE.with(|s| s.borrow().registry.as_ref().map(|r| r.set_enabled(&uid, enabled)));
            schedule_sources();
            match r {
                Some(Ok(())) => Ok(()),
                Some(Err(e)) => Err(e.to_string()),
                None => Err("Calendar data isn't available.".into()),
            }
        },
        reply,
    );
}

pub fn set_color(uid: String, color: String, reply: impl FnOnce(Result<(), String>) + 'static) {
    ask(
        move || {
            let r = STATE.with(|s| s.borrow().registry.as_ref().map(|r| r.set_color(&uid, Kind::Calendar, &color)));
            schedule_sources();
            match r {
                Some(Ok(())) => Ok(()),
                Some(Err(e)) => Err(e.to_string()),
                None => Err("Calendar data isn't available.".into()),
            }
        },
        reply,
    );
}

/// Every calendar, with whether it's switched on (Settings → Calendars).
pub fn all_calendars(reply: impl FnOnce(Vec<(nyx_eds::Source, bool)>) + 'static) {
    ask(|| STATE.with(|s| s.borrow().registry.as_ref().map(|r| r.all_sources(Kind::Calendar)).unwrap_or_default()), reply);
}

pub fn refresh(reply: impl FnOnce(Vec<String>) + 'static) {
    ask(
        || {
            let problems = STATE.with(|s| {
                let s = s.borrow();
                s.open.iter().filter_map(|(uid, cal)| cal.refresh().err().map(|e| format!("{}: {e}", name_of(&s, uid)))).collect()
            });
            schedule_sources();
            problems
        },
        reply,
    );
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
            let accounts = nyx_accounts::Client::connect().ok();
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
            send_identities();
            send_unsupported();
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

/// The user's addresses: every online account's identity, and the From
/// address of its mail settings.
fn send_identities() {
    let mut me: Vec<String> = Vec::new();
    STATE.with(|s| {
        let s = s.borrow();
        let Some(client) = &s.accounts else { return };
        for account in client.list().unwrap_or_default() {
            for candidate in [account.identity.clone(), account.presentation.clone()] {
                if candidate.contains('@') && !me.iter().any(|m| m.eq_ignore_ascii_case(&candidate)) {
                    me.push(candidate);
                }
            }
            if let Ok(Some(mail)) = client.mail_settings(&account)
                && mail.address.contains('@')
                && !me.iter().any(|m| m.eq_ignore_ascii_case(&mail.address))
            {
                me.push(mail.address);
            }
        }
    });
    send(Update::Identities(me));
}

/// Microsoft accounts (GOA `ms_graph`, `exchange`) need evolution-ews's EDS
/// backends, which plain evolution-data-server doesn't have.
fn send_unsupported() {
    let installed = |name: &str| {
        ["/usr/lib", "/usr/lib64", "/usr/local/lib"]
            .iter()
            .any(|dir| std::path::Path::new(&format!("{dir}/evolution-data-server/calendar-backends/{name}")).exists())
    };
    let mut missing = Vec::new();
    STATE.with(|s| {
        let s = s.borrow();
        let Some(client) = &s.accounts else { return };
        for account in client.list().unwrap_or_default() {
            let backend = match account.provider_type.as_str() {
                "ms_graph" => "libecalbackendmicrosoft365.so",
                "exchange" => "libecalbackendews.so",
                _ => continue,
            };
            if !installed(backend) {
                missing.push(account.label().to_string());
            }
        }
    });
    send(Update::Unsupported(missing));
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
        send_identities();
        send_unsupported();
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

fn calendar_list(state: &State) -> Vec<CalendarInfo> {
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
        .map(|s| {
            let open = state.open.get(&s.uid);
            CalendarInfo {
                name: s.label().to_string(),
                needs_sign_in: s.account.as_ref().is_some_and(|a| attention.contains(a)),
                offline: open.is_some_and(|c| !c.online()),
                read_only: open.is_some_and(|c| c.read_only()) || s.backend == "contacts",
                no_following: open.is_some_and(|c| c.can("no-thisandfuture")),
                uid: s.uid,
                color: s.color,
                selected: s.selected,
                group: s.group,
                account: s.account,
            }
        })
        .collect()
}

/// Sends the calendar list, unless nothing changed and `always` is false.
fn send_calendars(always: bool) {
    let list = STATE.with(|s| calendar_list(&s.borrow()));
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
    let Some((from, to)) = STATE.with(|s| s.borrow().range) else { return };
    let (a, b) = (midnight(from), midnight(to));
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
            events.extend(cal.instances(a, b).iter().filter_map(|item| Event::from_item(&info.uid, item)));
        }
        // Watch what is on screen, so syncs and other apps' changes show up.
        let uids: Vec<String> = shown.iter().map(|c| c.uid.clone()).filter(|u| state.open.contains_key(u)).collect();
        if state.watching.as_ref() != Some(&(from, to, uids.clone())) {
            state.views.clear();
            let query = range_query(a, b);
            for uid in &uids {
                match state.open[uid].watch(&query, schedule_events) {
                    Ok(view) => state.views.push(view),
                    Err(err) => problems.push(format!("{}: {err}", name_of(state, uid))),
                }
            }
            state.watching = Some((from, to, uids));
        }
    });
    events.sort_by_key(|e| e.sort_key());
    send(Update::Events { from, to, events, problems });
    send_calendars(false);
}
