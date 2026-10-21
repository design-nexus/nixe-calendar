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

### 1. Vertical slice
- [ ] Sidebar (Music construction): calendars from `nixe-eds`, grouped by account, with color and visibility
- [ ] Read-only week view built from `ECalClient` instances
- [ ] Status bar: sync state, offline readout, gear
- [ ] Banners: EDS missing or not answering, GOA missing, account needs sign-in (opens Settings → Online Accounts)
- [ ] Live check against the connected Google account; offline check with the network down

### 2. MVP
- [ ] Day, month and agenda views; mini-calendar
- [ ] Create and edit (popover quick edit, full editor in a card); all-day; time zones
- [ ] Recurrence editor; edit this / this and following / all
- [ ] Invitations: attendees, RSVP
- [ ] `nixe-calendard`: reminders through `EReminderWatcher`, notifications, `agenda.sqlite`; stays silent while `evolution-alarm-notify` runs
- [ ] Search; `.ics` import and export; `text/calendar` handler
- [ ] Control tools for Assistant

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
