# Fastmail

Fastmail is a practical compatibility target and a useful provider because it supports standard calendar import and export and CalDAV. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Fastmail documents importing events from `.ics` files and exporting calendars as `.ics`.
- Fastmail states that alarms and notification preferences are not imported or exported. Missing alarms in Fastmail exports are a client observation, not a Ganbaru AI rule.
- Fastmail replaces existing event details when an uploaded file contains an event that already exists.

Source: <https://www.fastmail.help/hc/en-us/articles/360060590773-Import-export-your-calendars>

## Client-specific priorities

- An alarm fixture to confirm the documented alarm loss.
- A duplicate `UID` upload to confirm replacement behavior.

## Behavior to verify

- Whether duplicate `UID` import replaces details as documented.
- Whether attendee participation fields round-trip.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
