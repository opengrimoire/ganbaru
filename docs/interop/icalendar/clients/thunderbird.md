# Thunderbird

Thunderbird is both a practical compatibility target and a useful reference, because its calendar model is built around iCalendar concepts. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Thunderbird's calendar item model leans heavily on iCalendar. It implements `VEVENT` events and `VTODO` tasks, but not `VJOURNAL`.
- Thunderbird uses `ical.js`, the same library Ganbaru AI uses for parsing.
- Thunderbird exports calendars as `.ics`.

Sources:

- <https://source-docs.thunderbird.net/en/latest/calendar/item_model.html>
- <https://support.mozilla.org/gu-IN/kb/exporting-and-sharing-a-calendar>

## Client-specific priorities

- Record the Thunderbird version, operating system, and calendar storage type.
- Include `VTODO` export, recurring tasks, task alarms, a mixed event and task calendar, and Thunderbird custom properties. Tasks are preserved for future task integration.

## Behavior to verify

- The exact `VTODO` shape Thunderbird exports.
- Whether Thunderbird accepts Ganbaru AI-generated `VTODO` once task support exists.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
