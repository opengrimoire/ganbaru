# Client compatibility

Client notes record practical import and export behavior. They are dated observations, not standards rules. The [iCalendar overview](../README.md) and [conformance audit](../conformance/README.md) define Ganbaru AI's compatibility contract.

## Manual test status

| Client | Status | Last run | Notes |
| --- | --- | --- | --- |
| [Google Calendar](./google-calendar.md) | partial | 2026-05 | Ordinary events and capped recurrence were exercised; stale unbounded recurrence behavior was observed. |
| [Apple Calendar](./apple-calendar.md) | not run | none | Fixture priorities and official source notes only. |
| [Outlook](./outlook.md) | not run | none | Test web and desktop variants separately. |
| [Thunderbird](./thunderbird.md) | not run | none | Include mixed `VEVENT` and `VTODO` coverage. |
| [Nextcloud](./nextcloud.md) | not run | none | Record server and Calendar app versions. |
| [Proton Calendar](./proton-calendar.md) | not run | none | Record product surface and plan when relevant. |
| [Fastmail](./fastmail.md) | not run | none | Verify documented alarm and duplicate-UID behavior. |

`Not run` means no dated Ganbaru AI round trip is recorded. It does not mean the client is incompatible.

## Shared procedure

Use disposable calendars and test accounts. Never begin with a user's primary calendar.

1. Record the client, platform, visible version, test date, timezone, and locale.
2. Create a disposable calendar.
3. Import the relevant Ganbaru AI fixtures from `apps/client/test-fixtures/ics/rfc5545/` and any client-specific raw fixtures.
4. Inspect all-day spans, timed values, recurrence, overrides, alarms, attendees, and unsupported components relevant to that client.
5. Export the same calendar back to `.ics` when the client supports it.
6. Keep the raw client export unchanged as a dated fixture when licensing and privacy allow.
7. Re-import the result into Ganbaru AI.
8. Compare semantic results, parser warnings, preserved structured fields, and bounded recurrence occurrence sets.
9. Record the result in the client document and update the status matrix above.

## Result record

Each manual run should state:

- Client and platform
- App, web, or server version when visible
- Account or plan type only when it affects behavior
- Test date
- Timezone and locale
- Input fixture names
- Import result in the external client
- Export and re-import result in Ganbaru AI
- Semantic differences, dropped fields, warnings, and UI surprises
- Raw fixture path and screenshots when retained

Do not report byte-level differences as compatibility failures by themselves. Legal serializers can normalize ordering, case, escaping, and line folding.

## Import expectations

Files from every client go through the same standards-based import. Client documents list only deviations from this baseline.

- Supported `VEVENT` data is projected; all-day `DTEND` is treated as exclusive.
- Everything else accepted is preserved under the [preservation rule](../README.md#preservation-rule): client `X-*` fields, unsupported components such as `VTODO`, custom `VTIMEZONE` definitions, alarm fields beyond the basic projection, and organizer and attendee parameters.
- Scheduling metadata and attendee responses stay inert and read-only. See [scheduling boundary](../scheduling-boundary.md).
- Attachments and URIs stay inert data until the user explicitly opens them.
- Data a client omits from its export, such as alarms, is recorded as a client observation. It never becomes a Ganbaru AI rule.

## Shared priority set

Every client should eventually receive at least:

- Single-day and multi-day all-day events
- Zoned, UTC, and floating timed events
- Recurring events with exclusions and moved instances
- Attendees, organizer, and inert scheduling metadata
- Basic alarms
- Non-ASCII and escaped text
- Unknown extensions
- Custom timezone definitions
- Mixed components where the client claims support

## Shared questions

Every client run should answer:

- Does the client keep unknown `X-*` properties through import and export?
- Does it rewrite timezones or custom `VTIMEZONE` definitions?
- Does it accept or drop `VTODO`, `VJOURNAL`, and `VFREEBUSY`?
- Does it keep `VALARM` repeat and duration?
- Does it handle `RANGE=THISANDFUTURE`?

Client documents add only client-specific priorities, quirks, questions, and observations.
