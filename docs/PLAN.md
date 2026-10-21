# Calendar plan

Nixe Calendar: day, week, month, year and agenda views over the calendars in Evolution
Data Server, for every account connected in Settings → Online Accounts and for calendars
kept on this computer. Shared foundation: [nixe-core docs/pim](https://github.com/design-nexus/nixe-core/blob/main/docs/pim/ARCHITECTURE.md).

| | |
| --- | --- |
| Packages | `nixe-calendar` (UI), `nixe-calendard` (reminders, agenda index), `nixe-calendar-glimpse` |
| App id | `io.github.design_nexus.nixe.Calendar` |
| Data | EDS (events); `~/.local/share/nixe-calendar/calendar.sqlite` (sets, working hours), `agenda.sqlite` (read-only snapshot for Glimpse) |
| Config | `~/.config/nixe-calendar/settings.toml` |
| MIME | `text/calendar`, `x-scheme-handler/webcal` |

## Phases

### 1. Vertical slice (done)
- [x] Sidebar (Music construction): calendars from `nixe-eds`, grouped by account, with color and visibility
- [x] Week view built from `ECalClient` instances
- [x] Status bar: sync state, offline readout, gear
- [x] Banners: EDS missing or not answering, account needs sign-in, offline
- [x] Live check against the connected Google account
- [ ] Offline check with the network down (needs Ken)

### 2. MVP (done)
- [x] Day, month and agenda views; mini-calendar
- [x] Create (quick create with natural language, and a full editor); all-day; edit
- [x] Repeat presets and endings; edit this / this and following / all
- [x] Invitations: organizer, guests and answers; Going / Maybe / Not going
- [x] `nixe-calendard`: reminders through `EReminderWatcher`, notifications with Snooze and Dismiss, `agenda.sqlite`; quiet while `evolution-alarm-notify` runs
- [x] Search (this view as you type; Enter searches a year either side)
- [x] `.ics` import (file picker, command line, Open with) and export
- [x] Control tools for Assistant
- [ ] Time zone choice per event (events are written in the local zone)
- [ ] Moving an event to another calendar

### 3. Advanced
- [ ] Year view
- [ ] Natural-language quick create (`nixe-when`), preview before saving when ambiguous
- [ ] Calendar Sets (Work, Personal, Combined, custom)
- [ ] Scheduling assistant: free/busy, working hours, suggested times
- [ ] Smart scheduling: propose moves; never change an existing event without approval
- [ ] Meeting links (Google Meet, Zoom, Teams): detect, join with one click
- [ ] Locations: open in Maps (OSM link until Nixe Maps exists)
- [ ] Optional reminders overlay from EDS task lists
- [ ] Microsoft 365 through `evolution-ews`

## Out of scope

Hosted backends; editing EDS source files directly; a second calendar store.
