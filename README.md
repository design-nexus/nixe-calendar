# Calendar

A calendar for Omarchy: the calendars from your Google, Microsoft 365, Outlook.com,
Nextcloud and other online accounts, and calendars kept on this computer, in one week view. Part of the
[Nixe](https://github.com/design-nexus/nixe) apps.

- Day, week, month and agenda views, with a month in the sidebar
- Type “Lunch with Sam tomorrow at 1” to add an event; a full editor for repeats, alerts, places and notes
- Repeating events, with changes to one, the rest, or all of them
- Answer invitations; see the organizer and who's coming
- Reminders as desktop notifications, with Snooze
- Import and export .ics files; search a year of events
- Calendars grouped by account, with their colors; click one to show or hide it
- Works offline from the saved copy and catches up when you're back online
- Follows your Omarchy theme; works from the keyboard and tiled at half-screen

Accounts are added in **Settings → Online Accounts**. Calendar data is kept by
Evolution Data Server, so it stays on this computer and syncs with your accounts. Microsoft 365
and Outlook.com calendars need the `evolution-ews` package.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nixe-calendar/main/install.sh | bash
```

## Keys

| Key | Action |
| --- | --- |
| N | New event |
| T | Today |
| D / W / M / A | Day, week, month, agenda |
| J / K | Next / previous week |
| R | Refresh |
| Ctrl+F | Find events this week |
| F1 | All shortcuts |
