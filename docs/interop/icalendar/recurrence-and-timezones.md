# Recurrence and timezones

Status: Partial. Common value types, all-day dates, zoned and UTC recurrence identities, and the supported `RRULE` subset are implemented. Custom `VTIMEZONE` evaluation, first-class floating events, and sub-daily or time-of-day rule parts are not.

Recurrence and timezones are the highest-risk part of iCalendar compatibility. The rule is: preserve accepted source semantics, and project only what the app can expand correctly. Occurrence expansion itself is specified in [Recurrence expansion](../../algorithms/calendar/recurrence-expansion.md); user-facing recurrence behavior is in [Calendar recurrence](../../features/calendar/recurrence.md).

## Time value categories

The app must distinguish these forms. Projection may convert them to row fields, but preservation keeps the value type and parameters:

- Date-only: `DTSTART;VALUE=DATE:20260513`
- UTC date-time: `DTSTART:20260513T150000Z`
- Zoned date-time: `DTSTART;TZID=America/New_York:20260513T090000`
- Floating date-time: `DTSTART:20260513T090000`
- Duration: `DURATION:PT1H`
- Period: `FREEBUSY:20260513T090000Z/20260513T100000Z`

## All-day events

iCalendar `DTEND;VALUE=DATE` is exclusive; the app's visible all-day end is inclusive.

- Import `DTSTART:20260513`, `DTEND:20260514` as May 13 only, and export it back the same way.
- A missing `DTEND` means one day unless `DURATION` says otherwise.
- `RECURRENCE-ID`, `EXDATE`, and `RDATE` stay date-only for all-day series.

## Floating timed events

Floating date-times have no `Z` and no `TZID`; they are not device-zone events.

- Implemented: the floating shape is preserved, the event is projected in the current render zone, and linked export keeps it floating.
- Planned: a first-class floating projection, if fixtures show floating events are common. Until then, an edit that would pin a floating event to a zone narrows its meaning.

## Timezones

- An IANA `TZID` is used directly for projection and recurrence.
- A known Windows `TZID` (common in Outlook) is mapped to IANA for projection; the original stays in preservation.
- A custom `VTIMEZONE` is preserved in full and exported before generated stubs. The app does not evaluate its transition rules; projection relies on the mapped IANA zone.
- Planned: warn before projection or export when a custom definition cannot be matched to an IANA zone.

Foreign `VTIMEZONE` definitions must never be discarded just because projection uses IANA zones.

## Recurrence properties

All `RRULE` parts defined by RFC 5545 (`FREQ`, `UNTIL`, `COUNT`, `INTERVAL`, `BYSECOND`, `BYMINUTE`, `BYHOUR`, `BYDAY`, `BYMONTHDAY`, `BYYEARDAY`, `BYWEEKNO`, `BYMONTH`, `BYSETPOS`, `WKST`), plus `RDATE`, `EXDATE`, `RECURRENCE-ID`, and `RANGE=THISANDFUTURE`, survive in structured preservation.

Projection represents `FREQ` (`DAILY`, `WEEKLY`, `MONTHLY`, `YEARLY`), `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (including ordinals), `BYMONTHDAY`, `BYMONTH`, `BYSETPOS`, `BYYEARDAY`, `BYWEEKNO`, and `WKST`. The native engine validates rule combinations and rejects anything else instead of guessing.

Known narrowing: on import, the parser canonicalizes `RRULE` into the projected subset. `BYSECOND`, `BYMINUTE`, and `BYHOUR` are dropped from the projection, and `SECONDLY`, `MINUTELY`, and `HOURLY` frequencies project as `DAILY`, without a warning. Because export regenerates `RRULE` from the projection, these parts are not restored on export. This conflicts with the preservation rule and is tracked as a [conformance gap](./conformance/README.md#critical-compatibility-gaps).

### RDATE and EXDATE

- Preserve the value type and timezone parameters of each property.
- Projected exclusions are stored as local date keys and exported at the master's original start time, never at midnight unless the master starts at midnight.
- An `RDATE` whose local time differs from the series time is preservation-only until date-time recurrence identities are supported.

### RECURRENCE-ID

- Its value type matches the master `DTSTART`: zoned for zoned masters, UTC for UTC masters, date-only for all-day masters.
- `RANGE=THISANDFUTURE` is always preserved. A cancelled override with that range suppresses its occurrence and all later ones during expansion. A non-cancelled one is preservation-only, and the app cannot create the range itself; "this and following" edits are exported as capped series.

## Expansion

Expansion is window-bounded and never runs over preserved-only data at startup. The native engine rejects requests whose windows or generated occurrence counts exceed fixed caps rather than truncating silently. For user-facing recurrence, wall-clock intent wins: a 9 AM daily zoned event stays at 9 AM local time across DST.

## Test expectations

DST coverage must include daily recurrence through spring-forward gaps and fall-back repeated hours, weekly and all-day recurrence across transitions, custom `VTIMEZONE` transitions, and local times that exist in one zone but not another.

Recurrence fixtures compare occurrence sets in bounded windows, not serialized strings: parse, preserve, project, expand a window, export, reparse, expand the same window, and compare start and end pairs and value types. Unsupported recurrence data can pass preservation tests before it passes projection tests.
