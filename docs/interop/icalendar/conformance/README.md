# Conformance

Status: Reference. This audit tracks implementation coverage against the [iCalendar standards scope](../README.md#standards). It separates projection, preservation, export, editing, and test evidence. Compatibility means semantic preservation within documented parser limits, not byte-for-byte reproduction.

## Status model

Each entry considers:

- **Projected:** mapped into normalized calendar rows.
- **Preserved:** retained in relational iCalendar storage.
- **Exported:** emitted in valid output with equivalent semantics.
- **Editable:** editable in the app without silently discarding preserved data.
- **Tested:** covered by an automated fixture or a recorded manual client run.

Values: `yes` (implemented with direct test evidence), `partial` (implemented for a documented subset), `no`, `preserve-only` (retained and exported, but not projected or editable), `planned`, and `not-applicable`.

## Audit baseline

Audit date: 2026-08-30. Recurrence entries were rechecked against the native recurrence engine on 2026-10-04.

Evidence:

- Parser, serializer, types, and tests: `apps/client/src/lib/calendar/ics/`
- Rule conversion: `apps/client/src/lib/calendar/rrule.ts`
- Native recurrence engine: `crates/ganbaru-calendar/src/recurrence/`
- Import and export stores: `apps/client/src/lib/stores/calendar/bulk-import.ts`, `import-export.ts`, `export-snapshot.ts`
- Native import and preservation: `crates/ganbaru-calendar/src/import.rs`, `import/`
- Native export snapshots: `crates/ganbaru-calendar/src/reads/`
- Schema: `crates/ganbaru-db/migrations/`
- Fixtures: `apps/client/test-fixtures/ics/`

This is a source and test audit, not a claim that every legal RFC 5545 form has a fixture. The storage, import, and export behavior being audited is described in [preservation and export](../preservation-and-export.md).

## Component summary

| Component | Projected | Preserved | Exported | Editable | Automated evidence |
| --- | --- | --- | --- | --- | --- |
| `VCALENDAR` | not-applicable | yes within limits | partial | no | partial |
| `VEVENT` | partial | yes within limits | partial | partial | yes for projected subset |
| `VTODO` | no | yes | preserve-only | no | yes for passthrough |
| `VJOURNAL` | no | yes | preserve-only | no | partial |
| `VFREEBUSY` | no | yes | preserve-only | no | yes for passthrough |
| `VTIMEZONE` | partial | yes | partial | no | partial |
| `VALARM` | partial | yes | partial | partial | partial |
| Custom or future components | no | yes when accepted | partial | no | partial |

Details: [components and properties](./components-and-properties.md), and [serialization and fixtures](./serialization-and-fixtures.md) for parameters, value types, serializer rules, and fixture evidence.

## Critical compatibility gaps

- Sub-daily `FREQ` values and `BYSECOND`, `BYMINUTE`, and `BYHOUR` are narrowed silently on import and lost on export, because `RRULE` is regenerated from the projection. They survive only in preservation storage. This contradicts the preservation rule.
- Object-level metadata beyond a single safe `METHOD` is not merged into export.
- Multiple or invalid object-level methods normalize to `METHOD:PUBLISH`.
- Recurrence math does not evaluate custom `VTIMEZONE` transition rules.
- `RDATE` and `EXDATE` value shapes are narrowed by projection; an `RDATE` with a different local time is preservation-only.
- Attendee and organizer parameters beyond the projected subset are preserve-only.
- Floating timed events project through the device zone, although linked export keeps the floating shape.
- `RANGE=THISANDFUTURE` is applied only for imported cancelled overrides and cannot be created by the app.
- Generated events always use `DTEND`; `DURATION` is kept only when the source used it.
- Re-import upserts by calendar and `source_uid`, not by component type and recurrence identity.
- Edit-risk status transitions, export warnings, and orphan preservation cleanup are planned.
- Manual client runs are absent for every tracked client except a partial Google Calendar run from May 2026.
