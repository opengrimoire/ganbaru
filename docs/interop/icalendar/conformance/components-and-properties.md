# Components and properties

Component and property coverage for the [conformance baseline](./README.md#audit-baseline). "Linked" means an event, override, or alarm whose projected row references its preserved source component. Generated-owned fields are listed in the [edit merge policy](../preservation-and-export.md#edit-merge-policy).

## Components

| Component | Projected | Preserved | Exported | Editable | Tested | Notes and gaps |
| --- | --- | --- | --- | --- | --- | --- |
| `VCALENDAR` | not-applicable | yes | partial | no | partial | Root, metadata, diagnostics, and children are stored. Export generates the object level; see object-level properties below. Original component order is not reproduced. |
| `VEVENT` | partial | yes | partial | partial | yes for projected subset | Parser, serializer, round-trip, fixture, and Rust import tests cover projection, preservation, links, and unsupported data surviving edits. Not every legal property or value shape has a fixture or an editable field. |
| `VTODO` | no | yes | preserve-only | no | yes for passthrough | No task projection or UI. |
| `VJOURNAL` | no | yes | preserve-only | no | partial | Covered through the shared non-event passthrough path. |
| `VFREEBUSY` | no | yes | preserve-only | no | yes for passthrough | No availability projection or UI. |
| `VTIMEZONE` | partial (via `TZID` mapping) | yes | partial | no | partial | `STANDARD` and `DAYLIGHT` are preserved and emitted before generated stubs. Expansion uses IANA zones, not the imported transition rules. |
| `VALARM` | partial | yes | partial | partial | partial | `ACTION`, `TRIGGER`, and `DESCRIPTION` project to alarm rows. Repeat, duration, email, audio, attendee, attachment, and extension semantics are not app behavior. |
| Nested, custom, future | no | yes | partial | no | partial | Top-level non-event components pass through; non-alarm nested components inside linked events are retained. Preserved `VEVENT`s without a projection, including orphan overrides, are not exported. |

## Object-level properties

| Property | Status |
| --- | --- |
| `PRODID` | Stored. Export generates Ganbaru AI's own. |
| `VERSION` | Stored. Export emits `2.0`. |
| `CALSCALE` | Stored. Export emits `GREGORIAN`. |
| `METHOD` | Stored. Export reuses one valid distinct method, otherwise `PUBLISH`. |
| `X-WR-CALNAME` | Preserved. Export generates it from the current calendar name. |
| `X-WR-TIMEZONE`, RFC 7986 (`NAME`, `DESCRIPTION`, `COLOR`, `IMAGE`, `REFRESH-INTERVAL`, `SOURCE`), unknown `X-*` and registered extensions | Preserved, not merged into export. |

The target is intentional field-by-field merging. Blind passthrough of conflicting object metadata is unsafe.

## Event properties

| Group | Property | Status |
| --- | --- | --- |
| Identity | `UID` | Projected as source UID, exported, tested. |
| | `SEQUENCE` | Projected, exported, used for duplicate selection and older-revision rejection, tested. |
| | `DTSTAMP` | Preserved; export generates a current value. |
| | `CREATED`, `LAST-MODIFIED` | Preserved and retained for linked events; not projected. App `created_at` and `updated_at` are app timestamps, not import provenance. |
| Time | `DTSTART`, `DTEND` | Projected and exported for UTC, `TZID`, floating, and all-day forms within the documented subset. All-day exclusive end is tested. |
| | `DURATION` | Projected into an end time. Linked export keeps the `DURATION` shape with a regenerated value. Generated events use `DTEND`. |
| Text | `SUMMARY`, `DESCRIPTION`, `LOCATION`, `URL` | Projected, exported, tested. Descriptions are sanitized before persistence. |
| | `COMMENT`, `RESOURCES`, `CONTACT` | Preserve-only for linked events. |
| Recurrence | `RRULE` | Projected for the supported subset and regenerated on export. Unrepresented parts are lost on export; see [recurrence and timezones](../recurrence-and-timezones.md#recurrence-properties). |
| | `RDATE` | Projected and exported. Date-time forms have stronger coverage than date-only and period forms. |
| | `EXDATE` | Projected as local date keys, exported at the event start time. |
| | `RECURRENCE-ID` | Projected as overrides; exported for UTC, zoned, and all-day cases. |
| | `RANGE=THISANDFUTURE` | Retained on linked overrides; applied for cancelled overrides; not creatable in the app. |
| Status | `STATUS` | `CONFIRMED`, `TENTATIVE`, `CANCELLED` projected, exported, tested. |
| | `TRANSP` | `OPAQUE`, `TRANSPARENT` projected, exported, tested. |
| | `CLASS` | `PUBLIC`, `PRIVATE` projected and exported; `CONFIDENTIAL` projects as `PRIVATE`. |
| | `PRIORITY`, `CATEGORIES`, `GEO` | Projected, exported, tested. |
| | `RELATED-TO`, `REQUEST-STATUS` | Preserve-only for linked events. |
| People | `ORGANIZER` | `CN` and address projected, exported, tested. |
| | `ATTENDEE` | `CN`, `ROLE`, `PARTSTAT`, `RSVP`, and address projected, exported, tested. |
| | Other people parameters | `SENT-BY`, `DIR`, `DELEGATED-FROM`, `DELEGATED-TO`, `MEMBER`, `CUTYPE`, multiple values, and unknown parameters are preserved and merged while their property remains. |
| Attachments | URI `ATTACH` | Preserved as inert data, retained for linked events. |
| | Binary `ATTACH` | Preserved within the inline limit; never decoded or opened. |
| Extensions | Event `X-*` | Projected as value-only extended properties and exported. |
| | Google guest-permission `X-*` | Projected as booleans and exported. |
| | Unknown registered properties | Preserve-only, so parameters, value types, and multiplicity are not narrowed into editable strings. |

Imported participation state is metadata, not proof that a reply was sent. See [scheduling boundary](../scheduling-boundary.md).

## Non-event components

`VTODO`, `VJOURNAL`, and `VFREEBUSY` fields (identity, dates, status, recurrence, people, periods, attachments, and extensions) are preserved and pass through as part of their top-level component. Task, journal, and free/busy projections are Planned.

## Timezone and alarm properties

- `VTIMEZONE`: `TZID` maps to an IANA zone for projection when possible. `LAST-MODIFIED`, `TZURL`, `DTSTART`, `TZOFFSETFROM`, `TZOFFSETTO`, `TZNAME`, `RRULE`, `RDATE`, `COMMENT`, and extensions are preserved and emitted with the definition.
- `VALARM`: `ACTION`, `TRIGGER`, and `DESCRIPTION` are projected, exported, and tested. `SUMMARY`, `ATTENDEE`, `DURATION`, `REPEAT`, `ATTACH`, `ACKNOWLEDGED`, `PROXIMITY`, and extensions are preserved and retained for linked alarms where `ical.js` supports them. Import warns that `REPEAT` and `DURATION` are not represented in the projection.
