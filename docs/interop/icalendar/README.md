# iCalendar compatibility

Status: Partial. Offline `.ics` import, structured preservation, `VEVENT` projection, and merged export are implemented. Non-event projections, full recurrence and timezone semantics, and manual client runs remain incomplete. Exact status lives in the [conformance audit](./conformance/README.md).

Ganbaru AI accepts, preserves, edits where possible, and exports iCalendar files without depending on Google, Outlook, CalDAV, email, or any hosted service. The target is broad offline round-trip compatibility: import RFC 5545 objects, retain unsupported structured data instead of dropping it, project supported `VEVENT` data into the calendar, and merge current projected fields over the preserved source on export.

Preservation is semantic, not byte-for-byte. Legal changes to property order, case, escaping, quoting, and line folding are acceptable when component meaning and unsupported structured data survive within the documented parser limits.

## Standards

- **RFC 5545 (iCalendar):** the core format and data model for components, properties, parameters, value types, recurrence, `VTIMEZONE`, and line serialization. This is the primary target.
- **RFC 6868 (parameter value encoding):** caret escaping for parameter values.
- **RFC 5546 (iTIP scheduling):** invitation and reply semantics. Ganbaru AI preserves this metadata offline but does not act on it. See [scheduling boundary](./scheduling-boundary.md).
- **RFC 7265 (jCal):** the in-memory parser and serializer shape. SQLite stores the same model relationally.
- **RFC 7986 (property extensions):** calendar names, colors, images, and conference properties. Tracked; currently preserved but not merged into object-level export.
- **IANA iCalendar registries:** the live list of registered components, properties, parameters, values, and methods.

Google Calendar, Outlook, Apple Calendar, Thunderbird, Nextcloud, Proton Calendar, and Fastmail are compatibility targets and fixture sources. Their quirks do not define correct behavior. See [clients](./clients/README.md).

## Compatibility levels

1. **Offline round trip (main goal):** accept legal input within safety limits, record unsupported structured data, and later export a legal object with equivalent meaning. No accounts, tokens, or network access.
2. **Semantic app support (incremental):** understand data well enough to render, edit, search, notify, expand recurrence, and connect it to Focus. This applies to `VEVENT` first; `VTODO`, `VJOURNAL`, and `VFREEBUSY` may follow when matching app surfaces exist. Unsupported semantics must still be preserved.
3. **Scheduling workflow (planned, transport-dependent):** send invitations, replies, and cancellations, or sync with CalDAV or a provider API. This needs identity and transport and is not required for levels 1 and 2.

## Design summary

Two layers keep compatibility from becoming always-loaded app state:

1. **Structured preservation layer:** imported objects and components stored relationally, including properties, parameters, value types, nested components, extensions, and timezone definitions.
2. **App projection layer:** the normalized calendar rows used for rendering, editing, Focus, search, and visible-window queries.

Calendar startup and visible-window queries use only projected rows. They never parse raw `.ics` text, load preserved component trees, or expand unbounded recurrence. Details live in [preservation and export](./preservation-and-export.md).

## Preservation rule

Unsupported standard data must be preserved unless it is invalid, unsafe, exceeds configured limits, or the user explicitly discards it. When the app cannot project or edit a field, it retains it for export and diagnostics.

## Non-goals

- Offline file compatibility is not scheduling automation. Sending replies, cancellations, invitations, email alarms, or remote updates requires an optional user-configured transport.
- File compatibility is not the same as showing every component in the UI. `VTODO`, `VJOURNAL`, and `VFREEBUSY` are preserved before the app has surfaces for them.

## Completion principle

A feature counts as compatible only when it has a standards interpretation, a preservation rule, import and export rules, an edit policy, automated fixtures where practical, and manual client coverage for major clients when behavior is client-sensitive.

## Documents

- [Preservation and export](./preservation-and-export.md): import and export flow, storage model, edit merge policy, deletion, and limits.
- [Recurrence and timezones](./recurrence-and-timezones.md): value types, all-day dates, floating times, `VTIMEZONE`, recurrence, and DST.
- [Scheduling boundary](./scheduling-boundary.md): offline scheduling metadata versus transport-backed actions.
- [Conformance](./conformance/README.md): audited status, gaps, component and property coverage, serialization, and fixtures.
- [Clients](./clients/README.md): shared manual procedure and dated client observations.
- [Decisions](./decisions.md): dated architecture decision log.

## References

- RFC 5545: <https://www.rfc-editor.org/rfc/rfc5545>
- RFC 5546: <https://www.rfc-editor.org/rfc/rfc5546>
- RFC 6868: <https://www.rfc-editor.org/rfc/rfc6868>
- RFC 7265: <https://www.rfc-editor.org/rfc/rfc7265>
- RFC 7986: <https://www.rfc-editor.org/rfc/rfc7986>
- IANA iCalendar registries: <https://www.iana.org/assignments/icalendar/icalendar.xhtml>
