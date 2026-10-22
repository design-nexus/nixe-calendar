//! Desktop notifications (org.freedesktop.Notifications) for due alerts, with
//! Open, Snooze and Dismiss.

use chrono::{DateTime, Local};
use gio::prelude::*;
use nixe_eds::Reminder;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const ICON: &str = "io.github.design_nexus.nixe.Calendar";
/// NotificationClosed reason: dismissed by the user.
const CLOSED_BY_USER: u32 = 2;

pub struct Notifier {
    conn: Option<gio::DBusConnection>,
    shown: Rc<RefCell<HashMap<u32, Reminder>>>,
    _subscriptions: Vec<gio::SignalSubscription>,
}

impl Notifier {
    pub fn new() -> Self {
        let conn = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok();
        let shown: Rc<RefCell<HashMap<u32, Reminder>>> = Rc::default();
        let mut subscriptions = Vec::new();
        if let Some(conn) = &conn {
            let s = shown.clone();
            subscriptions.push(conn.subscribe_to_signal(
                Some(NOTIFICATIONS),
                Some(NOTIFICATIONS),
                Some("ActionInvoked"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some((id, action)) = signal.parameters.get::<(u32, String)>() else { return };
                    let Some(reminder) = s.borrow_mut().remove(&id) else { return };
                    match action.as_str() {
                        "snooze" => crate::with_reminders(|r| r.snooze(&reminder, crate::snooze_until())),
                        "default" | "open" => {
                            let _ = std::process::Command::new("nixe-calendar").args(["--section", "day"]).spawn();
                            crate::with_reminders(|r| {
                                let _ = r.dismiss(&reminder);
                            });
                        }
                        _ => crate::with_reminders(|r| {
                            let _ = r.dismiss(&reminder);
                        }),
                    }
                },
            ));
            let s = shown.clone();
            subscriptions.push(conn.subscribe_to_signal(
                Some(NOTIFICATIONS),
                Some(NOTIFICATIONS),
                Some("NotificationClosed"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some((id, reason)) = signal.parameters.get::<(u32, u32)>() else { return };
                    // Closed by the user counts as dismissed; expired or replaced keeps it pending.
                    if let Some(reminder) = s.borrow_mut().remove(&id)
                        && reason == CLOSED_BY_USER
                    {
                        crate::with_reminders(|r| {
                            let _ = r.dismiss(&reminder);
                        });
                    }
                },
            ));
        }
        Self { conn, shown, _subscriptions: subscriptions }
    }

    pub fn show(&self, reminder: Reminder) {
        let Some(conn) = &self.conn else { return };
        let summary =
            if reminder.item.summary.trim().is_empty() { "Untitled".to_string() } else { reminder.item.summary.clone() };
        let body = body(&reminder, Local::now().timestamp());
        let actions: Vec<String> = ["default", "Open", "open", "Open", "snooze", "Snooze 10 min", "dismiss", "Dismiss"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut hints: HashMap<String, glib::Variant> = HashMap::new();
        hints.insert("desktop-entry".into(), ICON.to_variant());
        hints.insert("urgency".into(), 1u8.to_variant());
        hints.insert("category".into(), "x-gnome.calendar".to_variant());
        let params = ("Calendar", 0u32, ICON, summary.as_str(), body.as_str(), actions, hints, 0i32).to_variant();
        let shown = self.shown.clone();
        conn.call(
            Some(NOTIFICATIONS),
            PATH,
            NOTIFICATIONS,
            "Notify",
            Some(&params),
            Some(glib::VariantTy::new("(u)").expect("type")),
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
            move |result| match result.ok().and_then(|v| v.get::<(u32,)>()) {
                Some((id,)) => {
                    shown.borrow_mut().insert(id, reminder);
                }
                None => eprintln!("nixe-calendard: couldn't show a notification"),
            },
        );
    }
}

/// "In 10 minutes · 2:00 PM · Main St", "Now · Main St", "Tomorrow".
fn body(reminder: &Reminder, now: i64) -> String {
    let mut parts = Vec::new();
    let minutes = (reminder.starts - now).div_euclid(60);
    let all_day = reminder.item.all_day();
    parts.push(match minutes {
        m if m <= 0 && reminder.ends > now => "Now".to_string(),
        m if m <= 0 => "Earlier".to_string(),
        1 => "In 1 minute".to_string(),
        m if m < 60 => format!("In {m} minutes"),
        m if m < 120 => "In an hour".to_string(),
        m if m < 24 * 60 => format!("In {} hours", m / 60),
        m if m < 48 * 60 => "Tomorrow".to_string(),
        m => format!("In {} days", m / (24 * 60)),
    });
    if !all_day && let Some(start) = DateTime::from_timestamp(reminder.starts, 0) {
        let local = start.with_timezone(&Local);
        parts.push(if twelve_hour() { local.format("%-I:%M %p").to_string() } else { local.format("%H:%M").to_string() });
    }
    if !reminder.item.location.trim().is_empty() {
        parts.push(reminder.item.location.trim().to_string());
    }
    parts.join(" · ")
}

/// A 12-hour clock where the locale uses one (as the Calendar window does).
fn twelve_hour() -> bool {
    let locale =
        ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
    let region = locale.split(['.', '@']).next().unwrap_or("").rsplit('_').next().unwrap_or("").to_string();
    ["US", "CA", "AU", "NZ", "IN", "PH", "PK", "EG", "SA"].contains(&region.as_str())
}

/// Evolution's own reminder window is running (it would show the same alerts).
pub fn other_notifier_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else { return false };
    entries.flatten().any(|entry| {
        std::fs::read_to_string(entry.path().join("comm")).is_ok_and(|comm| comm.trim().starts_with("evolution-alarm"))
    })
}
