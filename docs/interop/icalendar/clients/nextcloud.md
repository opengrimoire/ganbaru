# Nextcloud

Nextcloud Calendar is a practical compatibility target and a CalDAV-adjacent source of standards-oriented iCalendar files. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Nextcloud's Calendar app supports RFC 5545 `.ics` files and imports one or more files through Calendar settings.
- Event data can be exported from the event editor unless an administrator disabled it.
- Administration docs describe CalDAV-backed calendar behavior and export controls.

Sources:

- <https://docs.nextcloud.com/server/stable/user_manual/en/groupware/calendar.html>
- <https://docs.nextcloud.com/server/latest/admin_manual/groupware/calendar.html>

## Client-specific priorities

- Record the server, Calendar app, and Tasks app versions.
- Include CalDAV-style recurrence and overrides, `VTODO` when the Tasks app is available, shared calendar export, and timezone definitions from a self-hosted server.

## Behavior to verify

- Whether Nextcloud accepts mixed `VEVENT` and `VTODO` calendars.
- Differences between hosted Nextcloud versions.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
