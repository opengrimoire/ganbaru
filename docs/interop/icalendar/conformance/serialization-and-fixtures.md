# Serialization and fixtures

Parameter, value-type, serialization, and fixture coverage for the [conformance baseline](./README.md#audit-baseline).

## Parameters

| Parameter | Status |
| --- | --- |
| `VALUE` | Honored for supported dates and date-times; preserved for linked export. |
| `TZID` | Projected and mapped when possible; the source parameter and related `VTIMEZONE` stay preserved. |
| `CN` | Projected for organizer and attendee, exported, tested. |
| `ROLE`, `PARTSTAT`, `RSVP` | Projected for attendee, exported, tested. |
| `CUTYPE`, `DELEGATED-FROM`, `DELEGATED-TO`, `DIR`, `MEMBER`, `SENT-BY` | Preserve-only; merged for linked people properties. |
| `LANGUAGE`, `ALTREP`, `RELTYPE`, `FMTTYPE`, `RELATED` | Preserve-only for linked properties. |
| `RANGE` | Retained for linked `RECURRENCE-ID`. |
| `FBTYPE` | Preserved in `VFREEBUSY` passthrough. |
| `ENCODING` | Preserved for inert binary values. |
| Unknown `X-*` and registered parameters | Retained and merged while their linked property remains. |

The relational model keeps decoded values and multiplicity. Serialization emits valid quoting and RFC 6868 caret encoding, not the source's exact quote placement or caret spelling.

## Value types

| Type | Status |
| --- | --- |
| `BINARY` | Preserved as inert data within the inline limit; never decoded. |
| `BOOLEAN` | Preserved; Google guest-permission extensions project as booleans. |
| `CAL-ADDRESS` | Projected for organizer and attendee. |
| `DATE`, `DATE-TIME` | Projected for event times and selected recurrence values. |
| `DURATION` | Projected for event end and alarm triggers; linked export keeps event `DURATION` shape. |
| `FLOAT` | Projected for `GEO`. |
| `INTEGER` | Projected for `PRIORITY` and `SEQUENCE`. |
| `PERIOD` | Preserved, including `VFREEBUSY` passthrough; not projected. |
| `RECUR` | Projected for the supported rule subset. |
| `TEXT` | Projected for common fields; escaped on export. |
| `TIME` | Preserved when accepted by `ical.js`; not projected. |
| `URI` | Projected for `URL`; other URI values are inert preservation data. |
| `UTC-OFFSET` | Preserved in timezone definitions. |

## Serialization rules

| Rule | Status | Notes |
| --- | --- | --- |
| CRLF line endings | yes | |
| Folding at 75 octets | yes | Tested with long values. |
| UTF-8-safe folding | yes | Never splits an encoded character. |
| TEXT escaping | yes | Backslash, semicolon, comma, newline, carriage return. |
| RFC 6868 parameter escaping | partial | `CN` and selected cases; not every parameter shape. |
| Exclusive date-only `DTEND` | yes | |
| `RECURRENCE-ID` type matching | partial | UTC, zoned, all-day, and linked range preservation. |
| `EXDATE` and `RDATE` type matching | partial | Date-time stronger than date and period forms. |
| Unknown properties and parameters in storage | yes | Stored as component, property, parameter, and value rows. |
| Unknown linked event data on export | partial | Covered merge paths retain it; object-level merge and orphan event passthrough are gaps. |
| Stable source bytes | not-applicable | Semantic equivalence is the contract. |

## Automated fixtures

The standards fixture pack lives in `apps/client/test-fixtures/ics/rfc5545/` and is exercised by `apps/client/src/lib/calendar/ics/fixture-suite.test.ts`, which links preserved jCal back to projected events so it runs the same overlay path as real imports.

| File | Covers |
| --- | --- |
| `core-events.ics` | Minimal UTC event, single and multi-day all-day events, `DURATION`, floating time, escaped text, categories, geo |
| `recurrence-timezones.ics` | Custom `VTIMEZONE`, DST-adjacent recurrence, `EXDATE`, `RDATE`, yearly all-day recurrence, `RECURRENCE-ID;RANGE=THISANDFUTURE` |
| `scheduling.ics` | `METHOD:REQUEST`, organizer and attendee parameters, delegation, members, `REQUEST-STATUS` |
| `components.ics` | Mixed `VEVENT`, `VTODO`, `VJOURNAL`, `VFREEBUSY`, `VTIMEZONE`, nested `VALARM` |
| `attachments-extensions.ics` | URI and binary attachments, RFC 7986 properties, object and component `X-*` |

Client-oriented samples (`google-calendar-sample.ics`, `outlook-sample.ics`, `edge-cases.ics`) live in `apps/client/test-fixtures/ics/`. Parser, serializer, and round-trip tests add inline cases, including malformed input and parser limits. Zip path, entry, and aggregate-size protections are covered by native import tests.

No fixture implies complete coverage of every property allowed on its component.

## Fixture backlog

- Recurrence: explicit `BYSECOND`, `BYMINUTE`, and `BYHOUR` cases and sub-daily frequencies; broader `BY*` combinations; date-only and period `RDATE`; parameter-rich `EXDATE`; custom `VTIMEZONE` evaluation across both DST transitions.
- Components: full `VTODO` with recurrence and alarms; broader `VJOURNAL`; multiple `VFREEBUSY` periods and `FBTYPE` variants; orphan overrides and other unprojected `VEVENT`s.
- Scheduling: `METHOD:CANCEL`, `METHOD:REPLY`, multi-value and quoted parameters, broader RFC 6868 cases, edited offline invitations.
- Alarms: absolute triggers, audio with attachment, email with attendees, full `REPEAT` and `DURATION`.
- Extensions: object-level RFC 7986 and custom-property merging, property groups, registered extension properties, more binary encodings.
- Security: malformed folding, oversized property, oversized recurrence count, nesting stress, external URIs, hostile description HTML, zip path traversal, decompression bomb shape.
- Clients: manual round trips for every tracked client.

## Fixture conventions

- Keep small readable fixtures for unit tests and at least one large synthetic fixture for performance.
- Keep raw client exports unchanged and include the date in the name when behavior may change, for example `google-2026-05-14-yearly-all-day.ics`. Standards fixtures use names such as `rfc5545-recur-exdate-zoned.ics`.
- Each fixture asserts expected diagnostics, preserved structure, projected rows where applicable, export validity, and semantic equivalence after reparse, including unsupported data surviving a supported edit.
- Compare semantic structure and bounded occurrence sets. Ordering, case, equivalent escaping, and legal folding changes are not failures unless they change meaning or drop unsupported data.
