# Calendar

A calendar for Omarchy: the calendars from your Google, Nextcloud and other online
accounts, and calendars kept on this computer, in one week view. Part of the
[Nixe](https://github.com/design-nexus/nixe) apps.

- Week view with all-day events, overlapping events side by side and a line at the current time
- Calendars grouped by account, with their colors; click one to show or hide it
- Works offline from the saved copy and catches up when you're back online
- Follows your Omarchy theme; works from the keyboard and tiled at half-screen

Accounts are added in **Settings → Online Accounts**. Calendar data is kept by
Evolution Data Server, so it stays on this computer and syncs with your accounts.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/design-nexus/nixe-calendar/main/install.sh | bash
```

## Keys

| Key | Action |
| --- | --- |
| T | Today |
| J / K | Next / previous week |
| R | Refresh |
| Ctrl+F | Find events this week |
| F1 | All shortcuts |
