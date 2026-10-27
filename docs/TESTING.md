# Calendar testing

## Unit

- Week, month and agenda layout: overlapping events, all-day spans, events crossing midnight.
- `nyx-when` phrases used by quick create (table tests live in nyx-core).
- Scheduling: free slots against working hours and busy ranges.

## EDS (private session)

Run under `dbus-run-session` with a scratch `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and
`XDG_CACHE_HOME`, using EDS's local calendar backend:

- create, modify, remove; recurrence with EXDATE and RECURRENCE-ID
- daylight-saving transitions (US and EU), all-day events across time zones
- edit this / this and following / all
- `.ics` import and export round trip
- registry missing or slow → error state, no crash

## Live (only with Ken's go-ahead)

Read-only against the connected Google account: events match EDS; offline with the
network down; Calendar service switched off and on in Settings. Writes only to a
scratch calendar.

## UI

Screenshots of the real window at 1120 and 800 px, with the Omarchy theme copied
into a scratch `XDG_STATE_HOME`.

```sh
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```
