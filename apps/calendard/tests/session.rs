//! End to end on a private session bus with scratch XDG directories (run through
//! tools/test-calendard.sh): an event with an alert due in 20 seconds goes into
//! EDS's local calendar, nixe-calendard runs, and this test plays the desktop's
//! notification server and checks what it is asked to show.

use gio::prelude::*;
use nixe_eds::{Calendar, Draft, Kind, Registry, Scope, When};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const NODE: &str = r#"<node><interface name="org.freedesktop.Notifications">
  <method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
    <arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
    <arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
  <method name="GetCapabilities"><arg type="as" direction="out"/></method>
</interface></node>"#;

/// What the fake server was asked to show: summary, body, actions.
type Shown = Rc<RefCell<Vec<(String, String, Vec<String>)>>>;

fn pump(until: Instant, mut done: impl FnMut() -> bool) -> bool {
    let ctx = glib::MainContext::default();
    while Instant::now() < until {
        while ctx.iteration(false) {}
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
#[ignore]
fn due_alert_becomes_a_notification_and_the_agenda_is_written() {
    assert!(std::env::var("NIXE_EDS_SCRATCH").is_ok(), "run through tools/test-calendard.sh");
    let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();

    // The fake notification server.
    let seen: Shown = Rc::default();
    let info = gio::DBusNodeInfo::for_xml(NODE).unwrap().lookup_interface("org.freedesktop.Notifications").unwrap();
    let sink = seen.clone();
    let _object = conn
        .register_object("/org/freedesktop/Notifications", &info)
        .method_call(move |_, _, _, _, method, params, invocation| match method {
            "Notify" => {
                let summary = params.child_value(3).get::<String>().unwrap_or_default();
                let body = params.child_value(4).get::<String>().unwrap_or_default();
                let actions = params.child_value(5).get::<Vec<String>>().unwrap_or_default();
                sink.borrow_mut().push((summary, body, actions));
                invocation.return_value(Some(&(7u32,).to_variant()));
            }
            _ => invocation.return_value(Some(&(vec!["body".to_string(), "actions".to_string()],).to_variant())),
        })
        .build()
        .unwrap();
    let _name = gio::bus_own_name_on_connection(&conn, "org.freedesktop.Notifications", gio::BusNameOwnerFlags::NONE, |_, _| {}, |_, _| {});

    let registry = Registry::connect().unwrap();
    let cal = Calendar::open(&registry, "system-calendar", Kind::Calendar, None).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let uid = cal
        .create(&Draft {
            summary: Some("Dentist".into()),
            location: Some("Main St".into()),
            start: Some(Some(When::Time(now + 20))),
            end: Some(Some(When::Time(now + 1820))),
            alarms: Some(vec![0]),
            ..Draft::default()
        })
        .unwrap();

    let mut daemon = std::process::Command::new(env!("CARGO_BIN_EXE_nixe-calendard")).spawn().unwrap();
    let agenda = std::path::PathBuf::from(std::env::var("XDG_DATA_HOME").unwrap()).join("nixe-calendar/agenda.sqlite");
    let notified = pump(Instant::now() + Duration::from_secs(90), || !seen.borrow().is_empty());
    let wrote = pump(Instant::now() + Duration::from_secs(10), || agenda.exists());
    let _ = daemon.kill();
    let _ = daemon.wait();
    cal.remove(&uid, None, Scope::All).unwrap();

    assert!(notified, "a notification was asked for");
    let seen = seen.borrow();
    let (summary, body, actions) = &seen[0];
    assert_eq!(summary, "Dentist");
    assert!(body.contains("Main St"), "{body}");
    assert!(actions.contains(&"snooze".to_string()) && actions.contains(&"dismiss".to_string()));
    assert!(wrote, "agenda.sqlite was written");
}
