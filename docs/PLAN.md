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
- [x] Year view (twelve months, days with events marked; Y)
- [x] Natural-language quick create (`nixe-when`), other readings offered before saving (done with the MVP)
- [x] Calendar Sets: named groups of calendars in the sidebar (shown through EDS visibility)
- [x] Scheduling assistant: working hours, Find a time in the editor, `calendar.suggest_times` (own calendars; others' free/busy later)
- [x] Smart scheduling: the editor names clashes and suggests free times; nothing moves without the user
- [x] Meeting links (Meet, Zoom, Teams, Jitsi, Whereby, Webex): Join in the popover (J) and on alerts
- [x] Locations: open on OpenStreetMap (until Nixe Maps exists)
- [x] Optional reminders overlay from EDS task lists (Settings → Working hours)
- [x] Microsoft 365 and Outlook.com through `evolution-ews` (needs the package; "this and following" is hidden where the backend can't do it)

## Out of scope

Hosted backends; editing EDS source files directly; a second calendar store.
