# Calendar schema

The calendar domain preserves editable local events, imported source fidelity, recurrence identity, notification state, project scheduling links, and Pomodoro history without making any UI projection authoritative. Exact tables live in `crates/ganbaru-db/migrations/`.

## Calendars and events

Every event belongs to a calendar. The built-in local calendar has a stable identity and cannot be removed. Imported calendars get their own identity so an import can be reviewed, removed, or repeated without mixing source ownership into the local calendar.

Event rows hold the editable canonical projection. Archive rows preserve events that cannot be hard-deleted because history, project links, imported identity, or another durable relationship refers to them. Archive is a data-preservation state, not a hidden UI filter.

Timed events store instants plus an IANA home zone; all-day events use floating inclusive dates. Callers must not infer elapsed duration by subtracting wall-clock labels across a timezone transition.

Calendar services validate ranges, ownership, recurrence consistency, and protected relationships before writing. Multi-row edits commit in one transaction.

## Recurrence identity

A recurring series has three relevant identities:

- The template (original event).
- The civil recurrence date selected from the rule.
- A concrete override or generated occurrence.

Pomodoro and project history keep both concrete event identity and original-series identity. Editing an occurrence must not make earlier history point to a newly generated ID.

Normalized recurrence data stores the rule, exception dates, additional dates, and overrides. Raw iCalendar data is preserved separately for source fidelity; the normalized form drives behavior, and preservation rows allow export without pretending unsupported properties were applied. Expansion rules are in [Recurrence expansion](../../algorithms/calendar/recurrence-expansion.md).

Scoped edits are validated as one complete plan before a single transaction applies it.

## Edit receipts

Semantic Calendar commands (`calendar_edit_receipts`) store a receipt in the vault in the same transaction as the Calendar and Focus changes. A receipt binds a command identity to its complete intent and accepted result, so a retry returns that result even if the source later changed or was removed. Receipts are kept indefinitely so retry survives restart; cleanup is not implemented.

## Import preservation

iCalendar import is staged: parsing and validation produce a bounded plan before rows are committed. Calendar, component, property, parameter, timezone, alarm, organizer, attendee, attachment, and unknown-property data stay associated with stable imported identities.

Preservation data is not a second editable event model. Editing an imported event updates the canonical projection while keeping enough provenance to export source values according to the interoperability policy.

Import deduplication uses source identities and explicit replacement behavior; titles, timestamps, or display order are not identity. A failed import leaves the previous calendar intact.

## Notifications

Notification definitions are portable event data. Scheduled native alarms and delivery state are platform-specific projections, reconciled against canonical intent and recreated where safe. Delivery callbacks must be idempotent. Android exact-alarm and permission state are device-local facts.

## Projects and Pomodoro

Task scheduling links reference exact task and event identities and record link meaning. Moving or archiving an event does not erase the task, and deleting a task does not erase a protected event.

Pomodoro configuration attaches to the event, but each run snapshots the rhythm and event context it needs for history. Later calendar edits never rewrite elapsed runs or segments. Run references to Calendar events are nullable current aliases; original run identity and segment facts never change. See invariants 6 and 7 in [Data invariants](../invariants.md).

## Deletion and repair

Hard deletion is allowed only when no protected relationship or preservation requirement remains. Domain services choose among hard delete, archive, recurrence detach, and relationship removal; generic callers do not bypass that decision.

Archives keep the complete event: normalized children, Focus configuration, task-link meaning and timestamps, and Music assignments with their original versions. These do not cascade away when the referenced task or Music object is later removed. Each archive records its original occurrence identity and, for scoped operations, its recurrence date. A duplicate archive key fails instead of replacing an earlier snapshot.

Each archive owns an independent copy of its imported data, with explicit archive-to-object ownership so cleanup after restoration never deletes data still used by another live or archived event. Removing a calendar moves archive-owned imported data to the built-in local calendar's storage custody while the archive keeps its original calendar identity. Archive-only imported data is not published in Local exports.

Deletion updates nullable Calendar references in the same transaction as any authorized Focus stop, recurrence writes, and the receipt. Short-lived Undo keeps its preimage only in process memory, so an accepted Undo receipt does not retain an indefinite copy of hard-deleted data. Permanent archive restoration validates the source calendar, recurrence, and task relationships first. See [Deletion and undo](../../features/calendar/deletion-and-undo.md).

Repair routines may rebuild derived indexes and native schedules. They must not synthesize event identity from mutable presentation fields or discard unknown imported data because the UI does not expose it.
