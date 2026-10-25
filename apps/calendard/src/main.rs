//! nixe-calendard: Calendar's background service. It turns due alerts from
//! Evolution Data Server into desktop notifications (with Snooze and Dismiss)
//! and keeps `agenda.sqlite` current for Glimpse. No GTK.
//!
//! While Evolution's own `evolution-alarm-notify` runs, it doesn't notify, so
//! alerts aren't shown twice; both use EDS's record of dismissed alerts. Alerts
//! from task lists are left to Nixe Reminders.

mod agenda;
mod notify;

use chrono::{Duration, Local};
use nixe_calendar_core::{CalendarInfo, Event, midnight};
use nixe_eds::{Calendar, Kind, Registry, Reminders, View, Watch, range_query};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

const BUS_NAME: &str = "io.github.design_nexus.nixe.CalendarEngine";
const SNOOZE_MINUTES: i64 = 10;

const USAGE: &str = "Usage: nixe-calendard [--write-agenda]\n\
\n\
  Shows calendar alerts as notifications and keeps agenda.sqlite current.\n\
  --write-agenda   write agenda.sqlite once and exit\n";

#[derive(Default)]
struct State {
    registry: Option<Registry>,
    _watch: Option<Watch>,
    reminders: Option<Reminders>,
    open: HashMap<String, Calendar>,
    views: Vec<View>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static PENDING: Cell<bool> = const { Cell::new(false) };
}

fn main() {
    nixe_core::app::init(nixe_core::app::App { slug: "nixe-calendar", name: "Calendar" });
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "--write-agenda") {
        match Registry::connect() {
            Ok(registry) => {
                STATE.with(|s| s.borrow_mut().registry = Some(registry));
                match write_agenda() {
                    Ok(n) => println!("{n} events in {}", agenda_path().display()),
                    Err(e) => {
                        eprintln!("nixe-calendard: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("nixe-calendard: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let main_loop = glib::MainLoop::new(None, false);
    let quit = main_loop.clone();
    let _owner = gio::bus_own_name(
        gio::BusType::Session,
        BUS_NAME,
        gio::BusNameOwnerFlags::DO_NOT_QUEUE,
        |_, _| {},
        |_, _| start(),
        move |_, _| {
            eprintln!("nixe-calendard: already running");
            quit.quit();
        },
    );
    main_loop.run();
}

fn agenda_path() -> std::path::PathBuf {
    nixe_core::paths::data_dir().join("agenda.sqlite")
}

fn start() {
    match Registry::connect() {
        Ok(registry) => {
            let watch = registry.watch(schedule);
            let reminders = Reminders::new(&registry);
            let notifier = Rc::new(notify::Notifier::new());
            reminders.connect_due(move |due, _snoozed| {
                if notify::other_notifier_running() {
                    return;
                }
                // Task lists' alerts are Nixe Reminders' to show.
                let events_only = |source: &str| {
                    STATE.with(|s| s.borrow().registry.as_ref().is_some_and(|r| r.source(source, Kind::Calendar).is_some()))
                };
                for reminder in due.into_iter().filter(|r| events_only(&r.source)) {
                    notifier.show(reminder);
                }
            });
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.registry = Some(registry);
                s._watch = Some(watch);
                s.reminders = Some(reminders);
            });
            schedule();
            // Rewrite hourly too, so "today" moves on without any change.
            glib::timeout_add_seconds_local(3600, || {
                schedule();
                glib::ControlFlow::Continue
            });
        }
        Err(err) => {
            eprintln!("nixe-calendard: {err}; trying again in 30 s");
            glib::timeout_add_seconds_local_once(30, start);
        }
    }
}

/// Snoozes or dismisses through the watcher (called from notification actions).
pub fn with_reminders(f: impl FnOnce(&Reminders)) {
    STATE.with(|s| {
        if let Some(r) = &s.borrow().reminders {
            f(r);
        }
    });
}

pub fn snooze_until() -> i64 {
    (Local::now() + Duration::minutes(SNOOZE_MINUTES)).timestamp()
}

/// Rewrites the agenda soon (changes come in bursts during a sync).
fn schedule() {
    if PENDING.with(|p| p.replace(true)) {
        return;
    }
    glib::timeout_add_seconds_local_once(2, || {
        PENDING.with(|p| p.set(false));
        if let Err(e) = write_agenda() {
            eprintln!("nixe-calendard: agenda: {e}");
        }
    });
}

fn write_agenda() -> Result<usize, String> {
    let today = Local::now().date_naive();
    let (from, to) = (today - Duration::days(agenda::DAYS_BEFORE), today + Duration::days(agenda::DAYS_AFTER));
    let (a, b) = (midnight(from), midnight(to));
    let (calendars, events) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let state = &mut *s;
        let Some(registry) = &state.registry else { return (Vec::new(), Vec::new()) };
        let calendars: Vec<CalendarInfo> = registry
            .sources(Kind::Calendar)
            .into_iter()
            .map(|src| CalendarInfo {
                name: src.label().to_string(),
                uid: src.uid,
                color: src.color,
                selected: src.selected,
                group: src.group,
                account: src.account,
                needs_sign_in: false,
                offline: false,
                read_only: false,
                no_following: false,
            })
            .collect();
        let mut events: Vec<Event> = Vec::new();
        let mut views = Vec::new();
        for cal in calendars.iter().filter(|c| c.selected) {
            if !state.open.contains_key(&cal.uid) {
                match Calendar::open(registry, &cal.uid, Kind::Calendar, None) {
                    Ok(open) => {
                        state.open.insert(cal.uid.clone(), open);
                    }
                    Err(e) => {
                        eprintln!("nixe-calendard: {}: {e}", cal.name);
                        continue;
                    }
                }
            }
            let open = &state.open[&cal.uid];
            events.extend(open.instances(a, b).iter().filter_map(|item| Event::from_item(&cal.uid, item)));
            if let Ok(view) = open.watch(&range_query(a, b), schedule) {
                views.push(view);
            }
        }
        state.views = views;
        let shown: Vec<&String> = calendars.iter().filter(|c| c.selected).map(|c| &c.uid).collect();
        state.open.retain(|uid, _| shown.contains(&uid));
        (calendars, events)
    });
    agenda::write(&agenda_path(), &calendars, &events, Local::now().timestamp()).map_err(|e| e.to_string())?;
    Ok(events.iter().filter(|e| !e.cancelled).count())
}
