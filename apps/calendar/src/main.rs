//! Calendar — your online and local calendars, for Omarchy.

mod actions;
mod agenda;
mod control;
mod days;
mod details;
mod draw;
mod editor;
mod minical;
mod month;
mod prefs;
mod quick;
mod settings_dialog;
mod settings_page;
mod theme;
mod widgets;
mod window;
mod worker;
mod year;

use gtk::prelude::*;
use gtk::{gio, glib};

pub const APP_ID: &str = "io.github.design_nexus.nixe.Calendar";

const USAGE: &str = "Usage: nixe-calendar [OPTIONS] [FILE.ics...]\n\
\n\
  FILE.ics        import the events in an iCalendar file\n\
  --section ID    open day, week, month, year, agenda or settings\n\
  --date DAY      show that day (YYYY-MM-DD)\n\
  --toggle        close the window if it's open, otherwise open it\n\
  --describe      print the control tools (JSON) and exit\n";

/// Starts the reminders service if it's installed and not running (it's enabled
/// for login by the installer, which doesn't start it).
fn start_reminders() {
    if std::env::var_os("CALENDAR_SNAPSHOT").is_some() {
        return;
    }
    let unit = nixe_core::paths::config_home().join("systemd/user/nixe-calendard.service");
    if unit.exists() {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "start", "--no-block", "nixe-calendard.service"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

fn main() -> glib::ExitCode {
    nixe_core::app::init(nixe_core::app::App { slug: "nixe-calendar", name: "Calendar" });
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }
    // The control tools as JSON, without a window (Nixe Assistant's abilities).
    if args.iter().any(|a| a == "--describe") {
        println!("{}", control::tools());
        return glib::ExitCode::SUCCESS;
    }
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: still single-threaded; nothing else reads the environment yet.
        unsafe { std::env::set_var("GSK_RENDERER", "ngl") };
    }

    let app = gtk::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE).build();
    app.connect_startup(|app| {
        theme::install();
        nixe_core::control::serve_async(app, control::tools(), control::handle);
        start_reminders();
    });
    app.connect_command_line(|app, cl| {
        let argv: Vec<String> = cl.arguments().iter().map(|a| a.to_string_lossy().to_string()).collect();
        let has = |flag: &str| argv.iter().any(|a| a == flag);
        let value = |flag: &str| argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1)).cloned();
        if has("--toggle")
            && let Some(w) = window::window()
            && w.is_visible()
        {
            w.close();
            return glib::ExitCode::SUCCESS;
        }
        window::present(app, value("--section").as_deref());
        if let Some(day) = value("--date").and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()) {
            window::go_to(day);
        }
        // Files to import, from the command line or "Open with Calendar". Wait a
        // moment for the calendars to load so the import dialog can list them.
        let cwd = cl.cwd();
        let files: Vec<std::path::PathBuf> = argv
            .iter()
            .skip(1)
            .filter(|a| {
                !a.starts_with("--")
                    && Some(a.as_str()) != value("--section").as_deref()
                    && Some(a.as_str()) != value("--date").as_deref()
            })
            .map(|a| {
                let path = std::path::PathBuf::from(a.strip_prefix("file://").unwrap_or(a));
                match &cwd {
                    Some(dir) if path.is_relative() => dir.join(path),
                    _ => path,
                }
            })
            .collect();
        for file in files {
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || actions::import_file(&file));
        }
        glib::ExitCode::SUCCESS
    });
    app.connect_shutdown(|_| prefs::flush());
    app.run()
}
