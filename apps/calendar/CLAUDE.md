# Calendar — notes for working on this app

- GTK4 (`gtk4` 0.11, `v4_18`) + Rust, no libadwaita. The window follows Music: a flat
  sidebar (`hexpand` false, 250 px, icons under 980 px), the page, and a status bar.
  Settings is `nixe_ui::settings_card`. Colors are `@theme_*` tokens; event colors are
  the calendars' own, from EDS.
- Events, calendars and which calendars are shown belong to Evolution Data Server.
  `worker.rs` owns `nixe_eds::Registry` and the open `Calendar`s on one thread with its
  own GLib main context (EDS signals arrive there) and sends `worker::Update` values to
  `window::apply` on the GTK thread. Never call EDS from the GTK thread.
- Accounts are GOA's: "Add account" and "Sign in" open Settings → Online Accounts
  (`nixe-settings --section online-accounts`). The app stores no credentials.
- `crates/nixe-calendar-core` turns EDS items into local-time `Event`s and lays out a
  week (`WeekLayout`); its tests cover overlaps, midnight and all-day lanes.
- Offline: EDS serves its saved copy; calendars report `online()` and the window shows
  an Offline banner and readout.
- Checks: `cargo test -p nixe-calendar-core -p nixe-calendar` and
  `cargo clippy --workspace --all-targets -- -D warnings`. Layout:
  `CALENDAR_SNAPSHOT=/tmp/cal.png CALENDAR_SNAPSHOT_W=800 nixe-calendar` under scratch
  `XDG_CONFIG_HOME`/`XDG_STATE_HOME` (copy Omarchy's colors.toml in for the real theme).
  It prints the sidebar width and whether the nav is compact.
- Until nixe-core's nixe-eds is published, Cargo.toml points at the sibling checkout
  with a `file://` git URL; CI refuses that on purpose.
