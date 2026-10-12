# Proton Calendar

Proton Calendar is a practical compatibility target. Its encryption and product model may affect what it imports, exports, or rewrites, so behavior must be tested from real fixtures. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Proton documents importing calendars from other services and exporting a calendar as `.ics`.
- Proton's import and export flow is account-based, but Ganbaru AI's `.ics` compatibility never depends on Proton account access.

Sources:

- <https://proton.me/support/protoncalendar-calendars>
- <https://proton.me/support/easy-switch-calendars>

## Client-specific priorities

- Record the Proton surface, and the plan only when it affects behavior.
- Include a calendar imported into Proton and exported again, reminders, and attendees if Proton exports them.

## Behavior to verify

- Whether Proton imports multi-event `.ics` files reliably on web and desktop.
- Whether Proton exports attendees and alarms.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
