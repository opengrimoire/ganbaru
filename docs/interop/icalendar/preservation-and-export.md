# Preservation and export

Status: Implemented for import, relational preservation, `VEVENT` projection, and merged export. Edit-risk status transitions, export warnings, and orphan cleanup are Planned.

This document owns the two-layer storage model, the import and export flow, the edit merge policy, deletion behavior, and the resource limits. Source of truth for exact schema is `crates/ganbaru-db/migrations/`; the parser and serializer live in `apps/client/src/lib/calendar/ics/`; native import, export snapshots, and preservation copying live in `crates/ganbaru-calendar/src/import*`, `reads/`, and `events/metadata/`.

## Storage model

### Projection layer

The normalized calendar tables (`calendar_events` and its attendee, alarm, override, exdate, rdate, category, and extended-property children) are the app-facing model. They are optimized for visible-window queries, recurrence expansion, editing, Focus, and notifications. The projection may be lossy compared with iCalendar, but every projected row that came from an imported component keeps a link to it.

### Preservation layer

Preservation stores accepted iCalendar structure relationally, without one column per RFC property and without JSON columns:

- `icalendar_objects`: one row per imported object, with owning calendar, `source_kind` (`import-file`, `import-zip-entry`, `local-export-base`, `subscription`), source name, source fingerprint, and the object-level `PRODID`, `VERSION`, `METHOD`, and `CALSCALE`.
- `icalendar_components`: one row per component, including the `VCALENDAR` root and nested components, with parent, type, `UID`, recurrence identity, `SEQUENCE`, projection link, preservation status, and order.
- `icalendar_component_properties`, `icalendar_property_parameters`, `icalendar_value_nodes`: ordered properties, parameters, and recursive value trees (arrays, objects, and scalars), which retain value types, multiplicity, and extension data.
- `icalendar_object_diagnostics`, `icalendar_component_projection_warnings`: ordered parser diagnostics and projection warnings.

Timezone definitions are ordinary preserved components. A dedicated timezone lookup table or a component link join table should be added only if measurements or new projections require them.

### Links between layers

- `calendar_events`, `calendar_event_overrides`, `calendar_event_attendees`, and `calendar_event_alarms` carry a nullable `icalendar_component_id`. Attendees also store `icalendar_property_index`, because `ATTENDEE` is a property on a `VEVENT`, not a component.
- Where one component maps to one row (master events, overrides, alarms), `icalendar_components.projected_kind` and `projected_id` provide the reverse link. Attendees rely only on the forward link so several attendees can reference the same `VEVENT`.
- `VTODO`, `VJOURNAL`, `VFREEBUSY`, and custom components are preservation-only until matching app surfaces exist.

### Preservation status

The schema accepts `lossless`, `partial`, `unsupported`, `needs-review`, `regenerated`, and `invalid`. Import assigns a status from the component type. An imported event with a source `UID` but no linked component reads as `regenerated`, so diagnostics make clear that no original is available. Status diagnostics must never block rendering of valid projected data.

## Import flow

1. Read the `.ics` file or `.ics.zip` entries through the native size and path checks.
2. Parse with `ical.js` into jCal, enforcing the parser limits below.
3. Store the accepted object and component tree in preservation tables.
4. Project supported `VEVENT` components into calendar rows and link each row to its component.
5. Preserve unsupported components without projecting them.
6. Emit warnings for narrowed projections, unsupported semantics, and repaired input.

Re-import identity:

- Within one file, duplicate master `VEVENT`s with the same `UID` resolve to the highest `SEQUENCE`, then the newest `LAST-MODIFIED`, `DTSTAMP`, or `CREATED`.
- Against stored rows, import matches `calendar_id` plus `source_uid`. A lower incoming `SEQUENCE` is skipped; an equal or higher one updates the row.
- Preservation objects are replaced by calendar, `source_kind`, and source name only when the import contains no older event revision.
- Overrides stay children of their selected master. Component type and `RECURRENCE-ID` are not independent upsert keys, and `source_fingerprint` is provenance, not an upsert key. A broader key (source, component type, `UID`, recurrence identity, sequence) may be needed if non-event components gain independent projections.

## Export flow

1. Rust reads one export snapshot for the target calendar in a single SQLite read transaction: the calendar header, full projected events and children, export metadata, and the preserved components it needs. Every value in one export comes from the same snapshot even if writes commit concurrently.
2. Preserved trees are reconstructed on a native blocking worker after the transaction is released. The snapshot includes live event and override components, timezone definitions, and top-level non-event components.
3. The frontend validates the typed snapshot and runs the `ical.js`-based serializer.
4. Linked events: the preserved component is reconstructed and generated-owned fields are overlaid from current projection data.
5. Local events without a preserved component are generated cleanly from projection data.
6. Preserved `VTIMEZONE` definitions are emitted before generated timezone stubs. Preserved top-level `VTODO`, `VJOURNAL`, `VFREEBUSY`, and custom components pass through unchanged.
7. Output uses CRLF line endings, 75-octet UTF-8-safe folding, TEXT escaping, and RFC 6868 parameter escaping, then goes through the native atomic writer.

Rules:

- Export never splices stale raw text with edited fields. It merges structured data. See [decisions](./decisions.md).
- Malformed or oversized snapshots fail explicitly. There is no partial export fallback.
- Top-level preserved `VEVENT`s are never passed through on their own. An event appears in export only through a live projected event or override.
- The object level is generated: `PRODID`, `VERSION:2.0`, `CALSCALE:GREGORIAN`, and `X-WR-CALNAME` from the current calendar name. A preserved `METHOD` is reused only when the calendar has exactly one valid distinct method; otherwise export emits `METHOD:PUBLISH`. Other preserved object-level properties are not merged, because blindly passing through conflicting object metadata from several imports is unsafe.

## Edit merge policy

Supported edits update projection rows only. Preserved components remain import provenance and are not rewritten at edit time; export reconstructs them and overlays the current projection.

Generated-owned `VEVENT` fields, replaced from projection on export:

- Title, description (sanitized before persistence), and location map to `SUMMARY`, `DESCRIPTION`, and `LOCATION`; the event link (also used for promoted conference URLs) maps to `URL`.
- Start, end, and all-day state map to `DTSTART` and `DTEND`, or to an imported `DURATION` shape when the source used one. Floating source times keep their floating shape.
- Recurrence maps to `RRULE`, `RDATE`, `EXDATE`, and override `RECURRENCE-ID`. Because `RRULE` is regenerated, rule parts the projection cannot represent are lost on export even though they remain in storage. See [recurrence and timezones](./recurrence-and-timezones.md).
- `STATUS`, `TRANSP`, `CLASS` (`PUBLIC` or `PRIVATE`; imported `CONFIDENTIAL` projects as `PRIVATE`), `PRIORITY`, `SEQUENCE`, `CATEGORIES`, `GEO`, and the Google guest-permission extensions.
- `ORGANIZER` and `ATTENDEE`: `CN`, `ROLE`, `PARTSTAT`, `RSVP`, and address are generated; other parameters such as `SENT-BY`, `DELEGATED-FROM`, `MEMBER`, and `CUTYPE` are merged back from preservation.
- `VALARM`: `ACTION`, `TRIGGER`, and `DESCRIPTION` are generated; other alarm fields are retained.
- `DTSTAMP` is generated at export time.

Everything else on a linked event stays from the preserved component: `COMMENT`, `RESOURCES`, `CONTACT`, `RELATED-TO`, `REQUEST-STATUS`, `CREATED`, `LAST-MODIFIED`, attachments, unknown registered properties, non-alarm nested components, and `RANGE=THISANDFUTURE` on overrides.

Detach and split copy the master's preservation into an independent object and component graph, including the envelope and timezone definitions, with attendee and alarm references pointing to the copy. Unrelated sibling `VEVENT`s are excluded. Copies survive deletion of the original import and move with the event to another calendar. Export replaces `UID` and recurrence identity from the current projection.

### Structural edit risks (planned safeguards)

Some edits make the preserved source questionable: converting a timed recurring event to all-day, changing recurrence frequency with complex overrides, changing timezone while a custom `VTIMEZONE` remains, changing organizer or attendees on a scheduling request, editing an event with `METHOD:REQUEST` or `METHOD:CANCEL`, or editing a component with unrepresented recurrence parts.

Planned behavior, not implemented:

- Status transitions: `lossless` to `partial` when projection narrows semantics; `partial` to `needs-review` when a structural edit makes the merge uncertain; `needs-review` to `regenerated` when the user accepts app-generated output; any status to `invalid` on unrecoverable structure.
- Export warnings naming the component `UID` when a component is `needs-review` or `invalid`, a regeneration dropped fields, a scheduling component was edited offline, a custom timezone could not be interpreted, or recurrence expansion was capped.

Repair is explicit. Re-importing the original file is the preferred repair path when preservation is missing or incomplete. Unsupported values that were never stored must not be invented from the projection.

## Deletion and retention

- Deleting a calendar removes its projected rows; foreign-key cascades then remove its preserved objects, components, and diagnostics.
- Deleting or archiving one event removes it from the projection but keeps the linked preservation rows. Archives hold their own references to the import objects they need so Undo can restore the event. Because export never passes through top-level preserved `VEVENT`s, the deleted event does not reappear.
- Deleting one occurrence of a series adds an `EXDATE` or override according to the [Calendar recurrence rules](../../features/calendar/recurrence.md).
- Planned: a tombstone or cleanup policy for preservation rows orphaned by single-event deletion, for storage and provenance clarity.

## Lazy loading and performance

Always loaded for rendering: projected rows in the requested window and slim recurrence and override data needed for expansion.

Loaded on demand: preserved components, full attendee and organizer parameters, full alarms, unsupported components, diagnostics, and export metadata.

Never required for startup: raw `.ics` text, every preserved component of a calendar, or recurrence instances for all time. Visible-window queries do not join preservation tables. Import and export may be slower than rendering but must stay bounded and report diagnostics.

## Limits

| Boundary | Limit |
| --- | --- |
| Plain `.ics` file and each zip entry | 25 MiB |
| Zip archive | 1,024 entries, 250 MiB total uncompressed; unsafe paths, encrypted entries, and wrong extensions are rejected |
| Parser | 2 MiB per unfolded line, 50,000 components, 500,000 properties, nesting depth 32, 1 MiB per inline binary `ATTACH` |
| Export snapshot | 500,000 records and 64 MiB of text, preflighted before allocation; preserved trees at most 32 levels; reconstructed and serialized output also capped at 64 MiB |
| Export output file | 25 MiB, enforced by the native writer |
| Export concurrency | One export prepares at a time; overlapping requests are rejected, not queued |
| Recurrence expansion | See [recurrence and timezones](./recurrence-and-timezones.md#expansion) |

Export limits apply to the whole calendar, so several individually accepted imports can make a calendar too large to export in one snapshot. Limit violations reject the operation without truncation. Progress reporting and streaming for large imports and exports are Planned.

## Security posture

Calendar files are untrusted input. The app keeps file and zip limits, caps component, property, and recurrence work, never fetches external URLs, treats attachments and `URI` values as inert data until the user explicitly opens them, and sanitizes descriptions before rendering.
