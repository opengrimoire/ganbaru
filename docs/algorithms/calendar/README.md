# Calendar algorithms

Calendar algorithms turn recurrence rules and overlapping schedules into deterministic concrete events and one Pomodoro ownership decision. They operate on normalized domain values and leave persistence to calendar services.

- [Recurrence expansion](recurrence-expansion.md) defines bounded native generation of occurrences, exception and override handling, identities, windows, timezone behavior, and how scoped edits partition a series.
- [Time conflict detection](time-conflict-detection.md) defines which overlapping event owns Focus at a given instant.

Interoperability scope and fixtures live under [iCalendar interoperability](../../interop/icalendar/README.md). Feature behavior lives in the [Calendar feature docs](../../features/calendar/README.md). Durable event identity and archive rules are in the [Calendar schema](../../data/schema/calendar.md).

## Shared requirements

- Use explicit inclusive or exclusive boundary semantics.
- Preserve stable event and recurrence identity.
- Resolve equal candidates deterministically.
- Bound expansion and scanning before processing untrusted imports.
- Keep local civil recurrence intent separate from elapsed instant arithmetic.
- Apply the same ownership policy to automation and visual projections.
