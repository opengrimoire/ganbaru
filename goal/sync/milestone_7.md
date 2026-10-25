# Milestone 7: Calendar, Focus history, and distraction rules and usage

**Status: Partial.** Implemented in source on `feat/sync`; physical acceptance (step 8) is pending. Part of the [concurrent sync goal](README.md). This file is the implementation plan for converting Calendar with its iCalendar preservation graph and archives, closed Focus history with per-device activity, and distraction rules, daily usage, and block history. The owning contracts are [Device linking and synchronization](../../docs/data/sync.md), [Sync merge rules](../../docs/algorithms/sync/README.md), [Sync schema](../../docs/data/schema/sync.md), [Calendar schema](../../docs/data/schema/calendar.md), [Pomodoro schema](../../docs/data/schema/pomodoro.md), [Focus authority](../../docs/algorithms/pomodoro/focus-authority.md), and the distraction section of [Supporting domains](../../docs/data/schema/supporting-domains.md). Each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- Calendars, events, recurrence exceptions, reminders, attendees, Focus configuration, imported iCalendar data, deleted-event archives, and task links are editable on every linked device offline and converge.
- Concurrent edits to different parts of an event (title on one device, color or reminders on another) both survive. Concurrent edits to the same title, description, or timing show a conflict; while an event's timing is in conflict, desktop automatic Focus admission skips its occurrences.
- Importing the same `.ics` file on two devices produces one copy of each event. Deleting an event and pressing Undo keeps its id and its Focus history links.
- A closed Focus run, with its segments, pauses, rhythm, and lifecycle events, appears on every device. An open run never leaves its device. Each device shows what the others are focusing on, and starting a run while another device is focusing asks first.
- Distraction rules, categories, and usage limits are shared by every device and live in SQLite, not `config.json`.
- Daily usage per source is recorded per device and summed across devices for limits, with no owner and no coordinator spool exchange.
- Block events replicate with their run. The native messaging host never opens the vault database.

## Scope

In scope:

- Calendar unit (table ids 160 to 223): `calendars`, `calendar_events` and its owned children, the seven `icalendar_*` tables, the archive tables, and `project_task_event_links`.
- Focus (224 to 271): `pomodoro_runs`, `pomodoro_segments`, `pomodoro_run_events`, the owned rhythm and pause rows, and a new `pomodoro_device_activity` table. Every other Pomodoro table becomes Local.
- Distractions (272 to 287): new rule tables replacing the `distractions` key of `config.json`, a new daily usage table, and `distractions_block_events`.
- Post-apply hooks: Calendar notification schedules, the iCalendar projection index, timing conflict suspension, Focus projection refresh, and distraction runtime projections.
- Removal of the owner usage drain, the coordinator and Android usage exchange, and the host's vault writes.
- No engine changes: every group shape this milestone uses comes from milestone 2, and no step edits shared engine files.

Out of scope:

- Remote control of Focus on another device, controller ownership epochs, and live companion status (Focus feature work, not this goal).
- Replicating or rebuilding the adaptive Focus model across devices (Deferred).
- Periodic subscription refresh of remote calendars; remote import stays an explicit user action.
- Preferences in `config.json` other than distractions (milestone 4), Music tables including `music_context_assignments` (milestone 5), Projects tables (milestone 6).
- Handoff retirement and compaction (milestone 10).

## Prerequisites

- Milestone 2: adapter registry, the per-domain replica write flag, Local tables and columns, reference cycles under deferred foreign keys, derived keys with an existence register, composite row keys through `sync_key` and the `derived_key` helper in `ganbaru-sync-contracts`, cascade hiding with repair tombstones, publication predicates and parent publication, deletion holds for Undo leases, post-apply hooks, dependency-ordered apply, generic and nested owned children, coupled groups with owned tables and reference columns, hook-maintained derived columns, conflict and recovery presentation, winner elision, unchanged-group elision, and guard triggers that skip while `sync_apply_state.applying = 1`.
- Milestone 5 (Calendar and Focus steps only): `music_context_assignments` is Replicated with the composite key `(owner_kind, owner_id, phase)` and its `sync_key`.
- Milestone 6 (Calendar and Focus steps only): `projects` and `project_tasks` are Replicated, so `calendar_events.project_id` and `project_task_event_links` may reference them, and the Projects scheduling rows written inside Calendar commits are captured.
- Milestone 4 is not required. The distraction steps need only milestone 2 and can run in parallel with the Calendar and Focus steps.

## Current state

Baseline lines refer to `crates/ganbaru-db/migrations/20261004220000_baseline_schema.sql` (B).

- Calendar and iCalendar: 33 Calendar tables and 7 iCalendar tables. `calendars` B:616 with the `'local'` seed at B:6464; `visible` B:621; `last_synced` B:624 is read (`crates/ganbaru-calendar/src/calendars.rs:63,78`, `apps/client/src/lib/stores/calendars.svelte.ts:13`) but never written. `calendar_events` B:722 with `calendar_id` `ON DELETE NO ACTION` B:728, the geo CHECK B:755, and UNIQUE `(calendar_id, source_uid)` B:771. Children B:774 to 907, with UNIQUE exdates B:816, rdates B:847, overrides `(parent_event_id, recurrence_id)` B:869, and the three event Pomodoro configuration tables B:880 to 907.
- Event ids: created events use `calendar-create-{sha256}` (`crates/ganbaru-calendar/src/events/create.rs:58`); splits and task scheduling derive ids at `events/metadata/plan.rs:311` and `events/task_schedule.rs:136`; imports use a frontend `randomUUID` candidate id.
- Edits rewrite the whole event row and delete and reinsert every child (`events/metadata/write.rs:51-174`, `events/metadata/rows.rs:11`). Recurrence removal adds exdates with random ids (`events/archive.rs:342`).
- The iCalendar graph (B:629 to 720) is written at import with ids derived from the object id; `projected_kind` and `projected_id` are set by a later UPDATE in the same transaction (`import/preservation.rs:388-410`) and recomputed when splits copy components (`events/metadata/copy.rs:200-253`). Removing a calendar moves its archived iCalendar objects to `'local'` (`calendars.rs:196-221`); objects and components cascade from `calendars` (B:631, B:658).
- Archives (B:909 to 1127): one root with 36 columns and 16 child tables, written once (`events/metadata/archive.rs:132-201`) and deleted only by Undo (`events/restore.rs:89`). No FK to live events; the only incoming FK is `pomodoro_runs.calendar_archive_id` (B:1195, `SET NULL`). `calendar_event_archive_import_objects` references `icalendar_objects` with `CASCADE` (B:1015).
- `calendar_edit_receipts` B:1128 are command receipts.
- Calendar writes Focus rows: closed-run retargeting (`events/commit.rs:664`), deletion archive links (`events/deletion/write.rs:133-148`), reference clearing (`events/archive.rs:371-410`), restore relinking (`events/restore.rs:417-490`), and Undo (`events/deletion/undo.rs:253-263`), all under a 5 s process lease (`apps/client/src-tauri/app/src/pomodoro/native_runtime/calendar_edit.rs:6`). Projects writes `calendar_events.project_id` (`crates/ganbaru-projects/src/relationship_commands.rs:205-228`); Calendar rewrites event-owned `music_context_assignments` rows, which have no FK (B:2045).
- Desktop automatic admission reads Calendar occurrences and the event Focus configuration (`crates/ganbaru-calendar/src/reads/focus_context.rs:41-132`). Android Calendar alarms live in SharedPreferences and are reminders only.
- Focus: `pomodoro_execution_state` B:1138 (singleton) and `pomodoro_execution_receipts` B:1148. `pomodoro_runs` B:1162 with random ids (`crates/ganbaru-pomodoro/src/execution/persistence.rs:129-136`), `last_heartbeat` rewritten every 15 s (`execution/observations.rs:162`), the global single-open UNIQUE B:1204, `event_id` `SET NULL` to events, a self FK `inherited_from_run_id`, and `calendar_archive_id`. Rhythm tables B:1224 and B:1234. `pomodoro_segments` B:1243 with the global single-active UNIQUE B:1263. `pomodoro_pauses` B:1265 replaced wholesale with new ids while open (`writes/run_events.rs:33-60`). `pomodoro_run_events` B:1278 reference segments with `SET NULL`. Desktop recovery closes an open run at its last heartbeat; Android resumes it (`execution/observations.rs:467-499`).
- Adaptive Focus: planned blocks B:1206 and 16 adaptive tables B:1311 to 1581 with a static policy id (`adaptive/decision.rs:22`), static experiment ids, and blended REAL context states updated by upsert. `pomodoro_run_adaptive_snapshots` B:1507 references adaptive tables. The schema doc calls adaptive local and opt-in.
- Distractions: rules and categories live in `config.json` under `distractions` (`apps/client/src/lib/distractions/rules.ts:259-346`), read by `apps/client/src-tauri/app/src/distractions/runtime/policy.rs:28`, `distractions/android/projection.rs:248`, and `crates/ganbaru-distractions/src/limits.rs:102-160`. SQLite has `distractions_block_events` B:1590 (`run_id` and `segment_id` `SET NULL`), rule snapshots B:1614, and `distractions_usage_samples` B:1637 (400-day retention and one-day future tolerance at `usage.rs:12-17`; desktop ids at `accounting.rs:128-132`). Limits sum samples by `(source_type, source_key, local_date)` (`limits/store.rs:7-58`).
- Usage spool: `app_config_dir/distractions-device-spool.sqlite` (`distractions/linked.rs:11-123`) with the owner drain (`linked.rs:368-395`), linked import (`linked.rs:814-870`), combined aggregation (`linked.rs:872-927`), guardian acknowledgements (`linked.rs:552-700`), the coordinator exchange (`vault/handoff/coordinator.rs:474-544`), and the Android exchange (`distractions/android.rs:480-531`).
- The native messaging host inserts block events directly into the vault (`crates/ganbaru-native-messaging/src/events.rs:15-100`) when `native_vault_is_writable` reads ownership from `vault-ownership.json` (`linked_usage.rs:190-220`); host usage goes to the spool (`linked_usage.rs:37-110`). The host reads rules from device runtime files written by the app, not from the vault.
- Engine: `OwnedTable { table, parent_column, order_column, columns }` is one flat child table per group (`crates/ganbaru-sync/src/manifest.rs`); `Engine::vault()` hardcodes the Quick notes adapter (`lib.rs:123-128`); references must point earlier in the manifest (`crates/ganbaru-sync/src/tests/conformance.rs:499-560`, `assert_well_formed`).

## Design

### Classification rule

A table is a top-level Replicated row when any FK from outside its owner's subtree points into it or a `Reference` resolves to it; otherwise it is an owned child encoded inside its parent's group. Owned groups materialize by delete and reinsert, so no table with an incoming `SET NULL` or `CASCADE` FK from elsewhere is ever owned. Device state that must commit atomically with domain writes is Local. This rule produces:

| Class | Tables |
| --- | --- |
| Replicated | `calendars`, `icalendar_objects`, `icalendar_components`, `calendar_events`, `calendar_event_archives`, `calendar_event_archive_import_objects`, `project_task_event_links`, `pomodoro_runs`, `pomodoro_segments`, `pomodoro_run_events`, `pomodoro_device_activity` (new), the six distraction rule tables (new), `distractions_usage_totals` (new), `distractions_block_events` |
| Owned, generic key `(owner, ordinal)` | Event alarms, attendees, organizers, categories, notifications, extended properties, exdates, rdates, overrides, and Focus configuration. iCalendar object diagnostics, component warnings, and component properties. The archive copies of those event tables plus archive music assignments and task links. Run count rhythms, run sequence steps, pauses, block event rule snapshots, usage limit entries |
| Owned, nested key `(parent key, ordinal)` | Override extended properties under overrides, count rhythms and sequence steps under Focus configuration (event and archive), parameters and value nodes under properties |
| Local | `calendar_edit_receipts`, `pomodoro_execution_state`, `pomodoro_execution_receipts`, `pomodoro_run_adaptive_snapshots`, `pomodoro_adaptive_planned_blocks`, every `pomodoro_adaptive_*` table, `distractions_usage_samples` |
| Local columns | `calendars.visible`, `calendars.last_synced` |

Adaptive tables reference runs and segments with `SET NULL` and `pomodoro_run_adaptive_snapshots` references runs with `CASCADE`, which the FK rule for Local tables allows. Nothing Replicated references an adaptive table.

Owned key shape (milestone 2 step 7). The step that converts a table rebuilds every owned table it owns: the primary key becomes the owner key column plus `ordinal INTEGER NOT NULL`, and a nested table's primary key is its parent row's full key plus its own `ordinal`, with a composite foreign key to that parent row `ON DELETE CASCADE`. Ordinals follow each reader's current order. Former primary keys and unique indexes (`event_id` on organizers and Focus configuration, `(event_id, step_index)`, `(archive_event_id, owner_kind, phase)`, `(archive_event_id, task_id)`, `run_id` on run count rhythms, `block_event_id` on rule snapshots, and the exdate, rdate, override, and extended property indexes) stay as UNIQUE constraints over data columns.

- Every owned table that has an `id` keeps it as a UNIQUE non-key data column carried in the row value, because Calendar patches, copies, archives, and export address rows by id and nested tables link by it: event and archive alarms, attendees, categories, notifications, extended properties, exdates, rdates, overrides, and override extended properties, the five iCalendar child tables, and pauses. Nested tables keep their parent id column (`override_id`, `archive_override_id`, `property_id`) as data with its foreign key.
- `icalendar_value_nodes` nests under properties only: `property_id` becomes `NOT NULL` (parameter values carry their property), `parameter_id` stays a data column without a foreign key, and `parent_node_id` keeps its self foreign key.
- Ids carried in owned values may stay random: merge replaces an owner's rows wholesale and nothing outside the owner references them. A writer that rewrites an owner's rows keeps the ids of unchanged rows, so unchanged-group elision (milestone 2 step 10) seals nothing for an unchanged rewrite.
- No foreign key from outside an owner reaches any of these ids (checked against the baseline and later migrations), so none of them is Replicated.
- No replicated column is `REAL` (milestone 2 owned columns rule). `geo_lat` and `geo_lng` on events and archives become `INTEGER` millionths of a degree, the precision RFC 5545 `GEO` allows: import rounds, commands refuse more than six decimals, and readers and export format them back. `icalendar_value_nodes.number_value` becomes `TEXT` holding the JSON number literal as parsed, so preservation stays lossless; `reads/icalendar.rs:244`, `reads/export_preservation.rs:392`, and `events/metadata/preservation.rs:142` parse it. Local adaptive tables keep `REAL`.

### Engine shapes used

The milestone needs these shapes. Milestone 2 delivers all of them (README decision on engine shapes); this milestone edits no shared engine file, and a missing shape found during implementation goes back to the engine track as its own step.

- Nested owned tables (milestone 2 step 7): an owned child whose parent is another owned table in the same group (overrides with override extended properties, properties with parameters and value nodes, archive overrides, Focus configuration with rhythms), keyed by the parent row's full key plus `ordinal` and materialized parent first.
- A `Coupled` group whose value includes owned tables and declared reference columns resolved like `Reference` (milestone 2 step 7; timing, run calendar link).
- Write-once references: a table row below reading "Reference to X, written once" is an `Immutable` group holding that column as a milestone 2 reference column (`ReferenceColumn` with the stated `on_delete`), resolved and hidden like a `Reference` group; no writer changes it after insert.
- Hook-maintained derived columns (milestone 2 step 7): never captured, recomputed by the domain's post-apply hook, and excluded from the summary fixture (`icalendar_components.projected_kind` and `projected_id`).
- Parent publication (milestone 2 step 6): a table declares `published_with` its parent. While the parent row fails its publication predicate, child writes are not captured; the seal that publishes the parent captures the current state of every child row pointing at it, and later child writes capture normally. Segments and run events publish with their run; block events with a non-null `run_id` do too.
- Composite row keys (milestone 2 step 4): `calendar_event_archive_import_objects` and `project_task_event_links` gain `sync_key`.
- Cascade hiding (milestone 2 step 5) for every `CASCADE` parent, including `calendar_events.calendar_id` once this milestone rebuilds it from `NO ACTION` to `CASCADE`.
- Self references (`icalendar_components.parent_component_id`, `pomodoro_runs.inherited_from_run_id`) under milestone 2 step 3's cycle rule.
- Unchanged-group elision at seal (milestone 2 step 10): a captured group whose encoded value equals the current winner is not sealed. Calendar edits rewrite every child, and without elision every edit would conflict with every concurrent edit.

### Calendar mapping

Every Calendar table bumps no `revision`; `updated_at` is a `Max` group. Group ids are listed so parallel agents and conformance fixtures agree.

`calendars` (160). Calendar removal is a tombstone without redirect; the domain already archives events and moves archived objects to `'local'` in the same transaction.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at` | Immutable | No | No |
| name | 1 | `name` | Register | No | Yes |
| color | 2 | `color` | Register | No | Yes |
| source | 3 | `source`, `read_only`, `source_url` | Coupled | No | Yes |
| updated | 4 | `updated_at` | Max | No | No |

`visible` is Local: hiding a calendar is a per-device view choice ("viewport layout" in the storage ownership table), and a new calendar materializes visible. `last_synced` is Local because nothing writes it.

`icalendar_objects` (161) and `icalendar_components` (162). Preserved source data is written once. Components are top-level because events and overrides reference them; one component with its property graph is one immutable value, which keeps every value far below the blob bound even for large files.

`icalendar_objects` (161):

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| source | 0 | `source_kind`, `source_name`, `source_fingerprint`, `prodid`, `version`, `method`, `calendar_scale`, `created_at` | Immutable | No | No |
| diagnostics | 1 | Owned `icalendar_object_diagnostics` | Immutable | No | No |
| calendar | 2 | `calendar_id` | Reference to 160, hiding parent | No | Yes |
| updated | 3 | `updated_at` | Max | No | No |

`icalendar_components` (162):

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| object | 0 | `object_id` | Reference to 161, hiding parent, written once | No | No |
| parent | 1 | `parent_component_id` | Reference to 162, hiding parent, written once | No | No |
| component | 2 | `component_type`, `uid`, `recurrence_id`, `recurrence_id_value_type`, `sequence`, `dtstart_key`, `preservation_status`, `sort_order`, `created_at` | Immutable | No | No |
| graph | 3 | Owned warnings, properties, parameters (nested), value nodes (nested) | Immutable | No | No |
| calendar | 4 | `calendar_id` | Reference to 160, hiding parent | No | Yes |
| updated | 5 | `updated_at` | Max | No | No |

`projected_kind` and `projected_id` become hook-maintained derived columns, rebuilt by the Calendar post-apply hook from `calendar_events.icalendar_component_id` and `calendar_event_overrides.icalendar_component_id`, and by the local writers in the same transaction as today. Import and copy write them on insert, never as a later UPDATE of a captured row.

Object ids stay random per import, so objects and components are not derived-key tables. The same file imported on two devices offline leaves two objects for one `(calendar_id, source_kind, source_name)`; the event `component` register picks one winner, and the next import of that source replaces both as today. Export includes a standalone object without linked components only when no other object of its source is linked, so the export carries one copy.

`calendar_events` (163) has random ids and tombstones, so deleted events reach recovery. Resolution for `UNIQUE (calendar_id, source_uid)`: duplicate repair, lowest key kept, references follow the repair redirect.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at` | Immutable | No | No |
| source | 1 | `source_uid` | Register | No | Yes |
| component | 2 | `icalendar_component_id` | Reference to 162, null allowed | No | Yes |
| title | 3 | `title` | Register | Yes | Yes |
| description | 4 | `description` | Register | Yes | Yes |
| timing | 5 | `start_time`, `end_time`, `timezone`, `all_day`, `rrule`, `repeat_until`; owned exdates, rdates, overrides, override extended properties (nested) | Coupled | Yes | Yes |
| calendar | 6 | `calendar_id` | Reference to 160, hiding parent | No | Yes |
| project | 7 | `project_id` | Reference to the milestone 6 `projects` table, null allowed | No | Yes |
| place | 8 | `location`, `geo_lat`, `geo_lng` | Coupled | No | Yes |
| color | 9 | `color` | Register | No | Yes |
| availability | 10 | `transparency`, `status` | Register | No | Yes |
| privacy | 11 | `visibility` | Register | No | Yes |
| priority | 12 | `priority` | Register | No | Yes |
| link | 13 | `url`, `meeting_enabled` | Register | No | Yes |
| guests | 14 | `guest_can_modify`, `guest_can_invite_others`, `guest_can_see_other_guests` | Register | No | Yes |
| rsvp | 15 | `local_rsvp_status` | Register | No | Yes |
| sequence | 16 | `sequence` | Max | No | No |
| music | 17 | `playlist_id` (plain text, no foreign key, null allowed) | Register | No | Yes |
| environment | 18 | `environment_id` | Register | No | Yes |
| alarms | 19 | Owned `calendar_event_alarms` | Register | No | Yes |
| attendees | 20 | Owned `calendar_event_attendees` | Register | No | Yes |
| organizer | 21 | Owned `calendar_event_organizers` | Register | No | Yes |
| categories | 22 | Owned `calendar_event_categories`, ordered by `sort_order` | Register | No | Yes |
| notifications | 23 | Owned `calendar_event_notifications`, ordered by `sort_order` | Register | No | Yes |
| properties | 24 | Owned `calendar_event_extended_properties` | Register | No | Yes |
| focus | 25 | Owned Focus configuration, count rhythms (nested), sequence steps (nested) | Register | No | Yes |
| updated | 26 | `updated_at` | Max | No | No |
| exists | 27 | Existence, no column | Register | No | No |

Decisions behind the shape:

- Timing is one surfaced coupled value (README decision and `docs/data/sync.md:195`), including every override row. Calendar validation refuses an edit or import whose encoded timing value exceeds 256 KiB with a localized "too many changed occurrences" error; on import the event is not projected, the iCalendar object keeps the full source, and a diagnostic names the event.
- A timing conflict suspends desktop automatic admission for that event's occurrences until resolved; `focus_context.rs` filters events with an open timing conflict through milestone 2 step 9's `open_conflicts(conn, table)`. Reminders and Android alarms keep using the winning value, because a missed reminder is worse than a reminder at the winning time.
- Event ids: created, split, and scheduled events keep their current derivations; each must include a per-command random input (Verify). Imported events keep random ids. The same file imported on two devices gives two rows with one `(calendar_id, source_uid)`, and duplicate repair keeps the lower key, so every replica shows one event; a re-import after deletion creates a new id, so the recreate guard never applies.
- The Calendar conversion migration rebuilds `calendar_events.calendar_id` from `ON DELETE NO ACTION` to `ON DELETE CASCADE` (README foreign key rule; `SET NULL` is impossible because the column is `NOT NULL DEFAULT 'local'`). Calendar removal already archives or deletes the calendar's events before deleting the row (`calendars.rs` `archive_or_delete_calendar_events_for_calendar`), so no local behavior or "in use" refusal changes. The cascade hides instead of redirecting, because redirects must point to lower keys and `'local'` is higher than generated ids. An event created or moved into a calendar that another device removed is hidden and becomes a recovery entry whose Restore places it in `'local'`.
- Projects keeps writing `project_id`; capture triggers record its writes as Calendar operations. Music stops writing `playlist_id` in milestone 5, and this milestone owns the column as a plain silent register: no foreign key exists, and readers already treat an unknown playlist id as none.
- Event-owned `music_context_assignments` rows stay in milestone 5's manifest. Calendar deletion tombstones them and Undo restores them; a reader ignores an assignment whose owner event is missing.

`calendar_event_archives` (164). Archives replicate because closed runs reference them and deleted-event history must read the same on every device; `docs/data/sync.md:172` already lists archive as portable.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| event | 0 | `source_event_id`, `archived_at`, `original_occurrence_id`, `recurrence_date`, `calendar_id`, `project_id`, `playlist_id`, `environment_id`, `source_uid`, `icalendar_component_id`, `sequence`, `created_at`, `updated_at` | Immutable | No | No |
| timing | 1 | `title`, `start_time`, `end_time`, `timezone`, `all_day`, `rrule`, `repeat_until` | Immutable | No | No |
| details | 2 | `description`, `location`, `url`, `color`, `transparency`, `status`, `visibility`, `priority`, `geo_lat`, `geo_lng`, `local_rsvp_status`, `meeting_enabled` | Immutable | No | No |
| guests | 3 | `guest_can_modify`, `guest_can_invite_others`, `guest_can_see_other_guests` | Immutable | No | No |
| alarms | 4 | Owned `calendar_event_archive_alarms` | Immutable | No | No |
| attendees | 5 | Owned `calendar_event_archive_attendees` | Immutable | No | No |
| categories | 6 | Owned `calendar_event_archive_categories` | Immutable | No | No |
| exdates | 7 | Owned `calendar_event_archive_exdates` | Immutable | No | No |
| properties | 8 | Owned `calendar_event_archive_extended_properties` | Immutable | No | No |
| music | 9 | Owned `calendar_event_archive_music_assignments` | Immutable | No | No |
| notifications | 10 | Owned `calendar_event_archive_notifications` | Immutable | No | No |
| organizer | 11 | Owned `calendar_event_archive_organizers` | Immutable | No | No |
| overrides | 12 | Owned `calendar_event_archive_overrides`, override extended properties (nested) | Immutable | No | No |
| focus | 13 | Owned `calendar_event_archive_pomodoro_configs`, count rhythms (nested), sequence steps (nested) | Immutable | No | No |
| rdates | 14 | Owned `calendar_event_archive_rdates` | Immutable | No | No |
| task_links | 15 | Owned `calendar_event_archive_task_links` (plain `task_id`, no FK) | Immutable | No | No |

Archive reference columns are plain text snapshots with no FK and are not `Reference` groups.

Archive ids stay `calendar-archive-{hash}` and must be unique per deletion command (Verify). Undo deletes the archive under the milestone 2 deletion hold, so an Undo inside the 5 s lease publishes neither the event tombstone nor the archive tombstone.

Composite keys. Tables 165 and 166 have composite primary keys, so each gains `sync_key TEXT NOT NULL UNIQUE` (README derived key decision). The Calendar and Projects writers fill it with `derived_key` from `ganbaru-sync-contracts` on insert. The migration fills existing rows with `'pending:' || rowid`; the Calendar conversion pass, run in Rust at vault open before capture seeding, rewrites every placeholder to the derived value, and a startup assertion refuses any remaining placeholder.

`calendar_event_archive_import_objects` (165), `sync_key` derived from `(archive_event_id, object_id)` with an existence register:

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| identity | 0 | `archive_event_id`, `object_id` | Immutable identity group; reference columns to 164 and 161, `Cascade` | No | No |
| exists | 1 | Existence, no column | Register | No | No |

`project_task_event_links` (166), `sync_key` derived from `(task_id, event_id)` with an existence register. Only `sync_key` inputs are written once, so `created_at` is a register:

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| identity | 0 | `task_id`, `event_id` | Immutable identity group; reference columns to milestone 6 `project_tasks` and 163, `Cascade` | No | No |
| created | 1 | `created_at` | Register | No | No |
| kind | 2 | `link_kind` | Register | No | Yes |
| exists | 3 | Existence, no column | Register | No | No |

### Calendar post-apply hooks

Hooks receive the connection and a change sink, never an `AppHandle` (milestone 2 `MaterializeHook`), so the same hooks run in the desktop app and in milestone 3's headless Android pass through `SyncHost`. A hook writes derived columns on the connection and reports touched events to the sink; the host acts on the reports after commit.

- Notifications: report events whose timing, alarms, notifications, or existence changed in the batch; the host rebuilds desktop scheduler entries and Android notification schedules for them through the existing per-event scheduling path that startup reconciliation runs today (`docs/data/sync.md:49`). Android alarms stay reminders and never start runs.
- Projection index: recompute `projected_kind` and `projected_id` for components touched by the batch.
- Focus: report touched events and runs; the desktop host notifies the native Focus runtime so its projection refreshes titles and links. A remote change never stops, starts, or retargets the local open run; a remote event delete reaches the open run only through the FK `SET NULL`.

### Focus mapping

Runs publish once closed: the publication predicate on `pomodoro_runs` is `ended_at IS NOT NULL`, which is monotonic because a closed run is never reopened (Verify). Remote runs therefore always arrive closed, so `idx_pomodoro_runs_single_open` and `idx_pomodoro_segments_single_active` stay global and cannot collide. Heartbeats touch only the unpublished open row and cost nothing.

`pomodoro_runs` (224):

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| plan | 0 | `original_event_id`, `event_date`, `planned_start`, `planned_end`, `rhythm_kind`, `rhythm_source`, `preset_key`, `idle_timeout_minutes`, `event_title_snapshot`, `start_trigger`, `created_at` | Immutable | No | No |
| outcome | 1 | `started_at`, `ended_at`, `end_reason`, `last_heartbeat` | Immutable | No | No |
| inheritance | 2 | `inherited_focus_minutes`, `inherited_rhythm_position`, `inherited_focus_milliseconds`, `inherited_phase_milliseconds` | Immutable | No | No |
| inherited_from | 3 | `inherited_from_run_id` | Immutable plain id, no foreign key: conformance refuses references into the predicate table 224, so the Focus conversion migration rebuilds `pomodoro_runs` without the `SET NULL` foreign key and readers treat an unknown id as no predecessor | No | No |
| rhythm | 4 | Owned `pomodoro_run_count_rhythms`, `pomodoro_run_sequence_steps` | Immutable | No | No |
| calendar_link | 5 | `event_id`, `current_occurrence_id`, `current_event_date`, `current_event_title`, `calendar_archive_id` | Coupled, `event_id` resolved against 163 and `calendar_archive_id` against 164 | No | No |

Calendar keeps writing `calendar_link` exactly as today; the writes are captured as Focus operations. A run closed on device B that references an event device A deleted arrives with its event resolved to null and no archive link; it still reads correctly from its title snapshot, and the existing Undo relink path already matches runs with a null archive by occurrence id.

`pomodoro_segments` (225), published with their run. Segments stay rows because block events and adaptive rows reference them.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| run | 0 | `run_id` | Reference to 224, hiding parent, written once | No | No |
| phase | 1 | `rhythm_position`, `phase`, `planned_start`, `planned_end`, `actual_start`, `actual_end`, `status`, `end_reason`, `event_date`, `chosen_duration_ms`, `created_at` | Immutable | No | No |
| event | 2 | `event_id` | Reference to 163, null allowed (written by Calendar) | No | No |
| pauses | 3 | Owned `pomodoro_pauses` | Immutable | No | No |

`pomodoro_run_events` (226), published with their run. It is top-level, not owned by the run, because its `segment_id` FK must materialize after segments.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| run | 0 | `run_id` | Reference to 224, hiding parent, written once | No | No |
| segment | 1 | `segment_id` | Reference to 225, null allowed, written once | No | No |
| event | 2 | Every remaining column (`event_type`, `occurred_at`, and the payload columns) | Immutable | No | No |

`pomodoro_device_activity` (227, new) shows what each device is focusing on. Key: the stable device id from `stable_device_id(app)` in `apps/client/src-tauri/app/src/sync/devices.rs`, which returns the `app-state.json` device id that sync membership pairs with this installation (`crate::vault::ensure_device_id`, `vault.rs:124`), never the writer id, so it survives writer rotation and restore; usage totals use the same id. The Focus runtime writes it in the same transaction as run start, phase change, pause, resume, and stop, never on heartbeat. `expires_at_ms` is the planned phase end plus 10 minutes for a running phase and the pause start plus 30 minutes for a pause. Readers treat an expired row as "last seen at" and never as focusing.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| activity | 0 | `state` (`idle`, `focus`, `short_break`, `long_break`, `paused`), `run_id` (plain text, because the run is unpublished), `occurrence_id`, `event_title`, `phase_started_at_ms`, `phase_ends_at_ms`, `expires_at_ms`, `updated_at_ms` | Coupled | No | No |

- Explicit start: if another device has a fresh row in a running or paused state, the start dialog asks for confirmation naming that device and its event. Confirming starts the run normally.
- Desktop automatic admission skips an occurrence that another device's fresh row is focusing on; other occurrences still admit, because admission already requires fresh local activity.
- Device names come from the sync membership records; the table stores no names.

Adaptive tables stay Local: the static policy and experiment ids are created independently on every device, context states are blended aggregates that cannot merge as registers, and adaptive is local and opt-in by contract. Each device learns from its own runs; rebuilding from replicated history is Deferred.

### Distraction mapping

Rules move from `config.json` into SQLite. No data migration: development vaults start from the seeded defaults (AGENTS.md "no external users"), and the `distractions` key and its reader are deleted. After milestone 4, only the `distractions` and `chat` sections remain in `config.json`; whichever of this milestone's rules step and milestone 9's Chat move merges last also deletes `config.json` and its loader. Rust owns validation and assembles the existing `DistractionsConfig` shape for the frontend and runtime readers; writes go through fine-grained commands (toggle, add, remove, rename, reorder entries). The whole-config save command is removed, because rewriting every row would turn every save into a conflict with every concurrent change.

Keys:

| Table | Id | Key |
| --- | --- | --- |
| `distractions_settings` | 272 | Seeded singleton `vault` |
| `distractions_category_rules` | 273 | Built-in category id, derived with existence register; a missing row means the default |
| `distractions_custom_categories` | 274 | Random id |
| `distractions_host_rules` | 275 | `derived_key("host", [list_kind, category_id, normalized host])` with existence register |
| `distractions_app_rules` | 276 | `derived_key("app", [platform, identity])`, identity is the package name on mobile and the normalized name on desktop, with existence register |
| `distractions_usage_limits` | 277 | Random id |

Groups (rule edits are low-stakes toggles, so nothing is surfaced; every mutable group is recoverable):

| Table | Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- | --- |
| 272 | mode | 0 | `mode` | Register | No | Yes |
| 272 | enabled | 1 | `enabled` | Register | No | Yes |
| 272 | phases | 2 | `block_during_focus`, `block_during_short_breaks`, `block_during_long_breaks`, `pause_during_focus_pause` | Coupled | No | Yes |
| 272 | desktop | 3 | `desktop_enabled` and the four `desktop_` prefixed phase flags | Coupled | No | Yes |
| 272 | mobile | 4 | `mobile_enabled` and the four `mobile_` prefixed phase flags | Coupled | No | Yes |
| 272 | limits | 5 | `limits_enabled` | Register | No | Yes |
| 273 | enabled | 0 | `enabled` | Register | No | Yes |
| 273 | exists | 1 | Existence, no column | Register | No | No |
| 274 | created | 0 | `created_at` | Immutable | No | No |
| 274 | name | 1 | `name` | Register | No | Yes |
| 274 | enabled | 2 | `enabled` | Register | No | Yes |
| 275 | rule | 0 | `list_kind` (blocked, exception, allowed, or category), `host` | Immutable | No | No |
| 275 | category | 1 | `category_id` | Reference to 274, hiding parent, null for global lists, written once | No | No |
| 275 | enabled | 2 | `enabled` | Register | No | Yes |
| 275 | created | 3 | `created_at` | Register | No | No |
| 275 | exists | 4 | Existence, no column | Register | No | No |
| 276 | rule | 0 | `platform`, `identity` | Immutable | No | No |
| 276 | label | 1 | `display_name`, `match_names_json` | Coupled | No | Yes |
| 276 | enabled | 2 | `enabled` | Register | No | Yes |
| 276 | created | 3 | `created_at` | Register | No | No |
| 276 | exists | 4 | Existence, no column | Register | No | No |
| 277 | created | 0 | `created_at` | Immutable | No | No |
| 277 | name | 1 | `name` | Register | No | Yes |
| 277 | enabled | 2 | `enabled` | Register | No | Yes |
| 277 | budget | 3 | `minutes_per_day`, `minutes_per_week` | Coupled | No | Yes |
| 277 | entries | 4 | Owned `distractions_usage_limit_entries`, ordered by `sort_order` | Register | No | Yes |

Columns are the snake case forms of the `DistractionsConfig` fields (`rules.ts:259-346`). On the derived-key tables (273, 275, 276) immutable groups hold only key inputs, because a re-add seals every group again; `created_at` is a register. `distractions_usage_limit_entries` is created in the generic owned key shape `(limit_id, ordinal)`.

Lists display in `created_at` then host or name order; there is no `Position` group, since rule order has no meaning for matching.

Usage. `distractions_usage_totals` (278) holds one row per device, source, and local day: key `derived_key("usage", [device_id, source_type, source_key, local_date])` with `device_id` from `stable_device_id`. The table does not declare `derived_key`: each row has one writer and is never deleted, so it needs no existence group, and its immutable group holds only key inputs.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| bucket | 0 | `device_id`, `source_type`, `source_key`, `local_date` | Immutable | No | No |
| elapsed | 1 | `elapsed_seconds` | Max | No | No |
| label | 2 | `display_name` | Register | No | No |

Each row has one writer and only grows, so `Max` converges without a Counter kind. It is keyed by source, not rule, because limits sum matching sources (`limits/store.rs:7-58`) and limit definitions change independently of history.

- `distractions_usage_samples` becomes Local: it stays the device's idempotent accounting journal (sample ids dedupe spool batches) and keeps its 400-day retention.
- A device recomputes its own totals rows from its samples at most every 5 minutes per bucket and on background or exit. Limit evaluation sums other devices' totals rows plus the sum of its own samples, so local enforcement is never stale.
- Totals rows are never deleted; the 400-day window is a read filter, so usage never creates tombstones.
- Disconnected devices can still briefly exceed a combined limit, as today; reconnection gives the exact sum.

`distractions_block_events` (279). Events with a `run_id` publish with their run; others publish immediately.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| event | 0 | `occurred_at`, `source_type`, `source_key`, `display_name`, `phase`, `decision`, `rule_id`, `category_id`, `created_at` | Immutable | No | No |
| run | 1 | `run_id` | Reference to 224, null allowed, written once | No | No |
| segment | 2 | `segment_id` | Reference to 225, null allowed, written once | No | No |
| snapshot | 3 | Owned `distractions_block_event_rule_snapshots` | Immutable | No | No |

Distraction post-apply hook: when any rule table changes, report it to the change sink; the host then republishes the desktop runtime files the native messaging host and the blocker read and the Android projection, through the same path a local rule edit uses.

### Spool and native host

- The host writes usage batches and block events only to `distractions-device-spool.sqlite`; `events.rs` loses its vault connection and `native_vault_is_writable` is deleted. The app ingests block events into `distractions_block_events` and usage into samples, then deletes the spool rows.
- The owner drain, linked import, combined aggregation, accepted and exported tables, guardian acknowledgements, the coordinator usage exchange, and the Android usage exchange are deleted with their protocol messages (`DistractionsExchange` and `DistractionsAcknowledged` in `crates/ganbaru-handoff/src/protocol.rs`). Removing them changes the handoff wire, so that step takes the next protocol version under the README wire versions rule.

### Migrations

Each conversion step adds one timestamped migration in `crates/ganbaru-db/migrations/` with the new tables, seed rows, and capture triggers rendered from its manifest (checked by the conformance test). Migrations never seed captures; milestone 2's `seed_captures` publishes existing rows at the next vault open, after the conversion passes. Step 2 rebuilds `calendar_events` with `calendar_id` as `ON DELETE CASCADE` and fixed-point `geo_lat` and `geo_lng` (recreating its indexes and triggers), rebuilds every Calendar, iCalendar, and archive owned table to the generic or nested key shape (archive `geo` rescaled with `ROUND(value * 1000000)`, value node numbers cast to `TEXT`, and parameter value nodes given their parameter's `property_id`), and adds `sync_key` with placeholders to `calendar_event_archive_import_objects` and `project_task_event_links`. Step 3 rebuilds the run rhythm tables and `pomodoro_pauses` to the generic key shape and creates `pomodoro_device_activity`. Step 4 creates and seeds the rule tables, with `distractions_usage_limit_entries` in the generic key shape. Step 5 creates `distractions_usage_totals`. Step 6 rebuilds `distractions_block_event_rule_snapshots` to the generic key shape. Each step that changes the manifest increments `VAULT_MANIFEST_VERSION` by one.

### Frontend

- Calendar: conflict badges and the resolution dialog for title, description, and timing (both timing versions as start, end, and recurrence summaries), recovery entries for hidden events, and a timing conflict note in the Focus admission explanation.
- Focus: the "focusing on another device" line in the Focus panel and the start confirmation.
- Distractions: the Settings section reads and writes through the new commands; no visible change otherwise.
- Strings go in the Calendar, Pomodoro, and Distractions catalogs (`apps/client/src/lib/i18n/messages/{en,es}/`), never `sync.ts`.

## Testing

- Conformance, codec round-trip, and crash-boundary tests for every converted table, driven by the manifest.
- Simulation scenarios added to the seed set: concurrent title and color edits on one event; concurrent timing edits giving one surfaced conflict on every replica; the same `.ics` imported on two replicas giving one event per UID; remove then re-import; calendar removal against a concurrent event add giving a hidden event and a recovery entry; delete against edit; delete and Undo inside the hold; a split on one replica while another closes a run on the split occurrence; a run closed on B for an event deleted on A; open runs never captured; segments, run events, and block events arriving with their run; usage `Max` convergence with interleaved flushes; rule toggles from two replicas; a host rule removed and added again.
- Calendar: tests for duplicate repair when two devices import one file (one event per `(calendar_id, source_uid)` survives with its owned rows), a re-import after deletion creating a new id, unchanged owned rows keeping their ids across an edit, owned rows read back by id after the key rebuild, `geo` and value node number round trips, the `sync_key` conversion pass and the placeholder assertion, calendar removal still archiving events under the `CASCADE` foreign key, the 256 KiB timing bound on edit and import, projection on insert, and admission skipping events with a timing conflict (`crates/ganbaru-calendar`).
- Focus: tests that closing a run publishes the run with its children in one seal, heartbeats capture nothing, activity rows are written on each transition and expire as specified, and the start warning reads fresh rows only (`crates/ganbaru-pomodoro`, `ganbaru-tauri-app`).
- Distractions: command validation, config assembly equal to the previous `DistractionsConfig` for the default seed, limits summing remote totals with local samples, spool ingestion idempotence, and a host test proving it never opens the vault.
- Frontend: Vitest for the timing conflict summary, the activity line, and the distraction API wrappers.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `refactor/calendar-sync-keys` | Value nodes always carrying `property_id`, projection written on component insert, the timing size bound, per-command randomness in every event id derivation. No sync code. Needs nothing. | `cargo test -p ganbaru-calendar --lib -j 4 import -- --test-threads=4`, `cargo test -p ganbaru-calendar --lib -j 4 events -- --test-threads=4` |
| 2 | `feat/sync-calendar` | Tables 160 to 166, `calendar.rs` manifest and adapter, migration with triggers, the `calendar_events.calendar_id` `CASCADE` rebuild, owned table rebuilds to the generic and nested key shapes, fixed-point `geo` and `TEXT` value node numbers with their readers, the export rule for duplicate import objects, `sync_key` on tables 165 and 166 with the Rust conversion pass and startup assertion, Local classification, hooks, timing conflict admission filter, conflict and recovery UI, catalogs. The Calendar domain joins the per-domain replica write flag, which lifts milestone 6's `owner_required` gates on Projects commands that write `calendar_events` or `project_task_event_links`. Needs 1, milestones 5 and 6. | `cargo test -p ganbaru-sync --lib -j 4 calendar -- --test-threads=4`, `cargo test -p ganbaru-calendar --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 calendar -- --test-threads=4`, focused Vitest for Calendar conflict components |
| 3 | `feat/sync-focus-history` | Tables 224 to 227, `focus.rs` manifest and adapter, run rhythm and pause rebuilds to the generic key shape, run publication, Local adaptive and execution tables, activity writer, start warning, admission check, Focus hook, catalogs. Needs 2 and 4. | `cargo test -p ganbaru-sync --lib -j 4 focus -- --test-threads=4`, `cargo test -p ganbaru-pomodoro --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 pomodoro -- --test-threads=4`, focused Vitest for the Focus panel |
| 4 | `feat/sync-distraction-rules` | `stable_device_id` in `apps/client/src-tauri/app/src/sync/devices.rs`, tables 272 to 277 with seed, persistence in `ganbaru-distractions`, commands, frontend API, runtime and Android readers, `config.json` section removal (and the file and its loader if milestone 9 already moved the `chat` section), rule hook. Needs milestone 2 only; runs in parallel with 1 to 3. | `cargo test -p ganbaru-distractions --lib -j 4 rules -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 distractions -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/distractions/rules.test.ts --maxWorkers=1` |
| 5 | `feat/sync-distraction-usage` | Table 278, Local samples, own-total flushing, limit evaluation, removal of the owner drain, aggregation, guardian, coordinator, and Android usage exchange with their protocol messages (next protocol version). Needs 4; keys totals with step 4's `stable_device_id`. | `cargo test -p ganbaru-distractions --lib -j 4 limits -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 distractions -- --test-threads=4` |
| 6 | `feat/sync-distraction-history` | Table 279 published with runs, rule snapshot rebuild to the generic key shape, host writes the spool only, block event ingestion. Needs 3 and 5. | `cargo test -p ganbaru-native-messaging --bin ganbaru-native-messaging -j 4 -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 distractions -- --test-threads=4` |
| 7 | `docs/sync-milestone-7` | Remaining docs, acceptance results mapped to automated coverage, goal README. | `pnpm -w run validate` |
| 8 | None | Physical acceptance on the devices below, recorded here. | Manual |

Steps 2 and 3 ship in the same release. Between them, a Calendar edit on a replica that rewrites closed-run references is refused by the Focus guard; that window exists only on `feat/sync`.

## Documentation

- `docs/data/sync.md`: rewrite the distraction usage exception (59-61) as per-device daily totals merged with `Max`; drop the spool from the mixed mode owner-only list (76); storage ownership rows for the distraction blocker (174: rule definitions, daily usage per device, block history; device-local raw samples, permissions, bindings, enforcement, spool) and Focus (178: closed runs with segments, pauses, events, and per-device activity; device-local open run, execution state, receipts, heartbeats, adaptive model); the Calendar coupled value rule (195) gains the reminder rule; delivery rows 239 (remove controller ownership and remote commands, keep live companion status) and 243; add "No process other than the app writes the vault; the native messaging host writes the spool only." These close the README rows for Focus runs and the native host.
- `docs/data/schema/sync.md`: the currently replicated list.
- `docs/data/schema/calendar.md`: classification, the owned key shape, imported event duplicate repair, the timing bound, `visible` device-local, projection columns derived, fixed-point `geo`, and `TEXT` value node numbers.
- `docs/data/schema/pomodoro.md`: closed-run publication, Local tables, the activity table.
- `docs/data/schema/supporting-domains.md` (41-51): rules in SQLite, usage totals, Local samples, the spool.
- `docs/algorithms/pomodoro/focus-authority.md`: replace "Planned: device controller ownership" with per-device runs, closed-run replication, activity rows, and no remote control.
- `docs/features/calendar/`: conflicts and the admission suspension; `docs/features/pomodoro/README.md`: activity and the start warning; `docs/features/distractions/README.md`, `usage-limits.md`, and `browser.md`: shared rules, cross-device limits, and the host writing the spool only.
- `docs/data/security/application-boundaries.md`, `docs/architecture/integrations.md`, `docs/architecture/native-backend.md`, and `docs/platforms/desktop/README.md` (step 6): the host no longer opens the vault and writes block events to the spool.
- `docs/features/README.md` status rows and `AGENTS.md` map lines for `distractions/` if their descriptions change.

## Manual acceptance

Two desktops (A is the coordinator, B is linked) and one Android phone (P).

| Scenario | Expected |
| --- | --- |
| Create, edit, and delete events on P offline, then reconnect | Same events on A and B; reminders rescheduled on every device. |
| A changes an event title while B changes its color | Both changes kept, no conflict. |
| A and P move the same event to different times offline | One conflict shown everywhere; A does not auto-start Focus for it until resolved; reminders follow the winner. |
| Import the same `.ics` on A and B offline | One copy of each event. |
| Remove a calendar on A while P adds an event to it offline | P's event becomes a recovery entry; Restore places it in Local. |
| Delete an event on A and press Undo | Event keeps its id; no deletion reaches B or P. |
| Hide a calendar on B | Still visible on A and P. |
| Focus on P, stop the run | The run with phases and pauses appears on A and B after stop, not before. |
| Focus on A, then press Start on B | B asks for confirmation naming A and the event. |
| A runs Focus and loses power | A's activity row expires; recovery on A closes and publishes the run. |
| Delete on A an event that B is focusing on | B keeps running; the closed run shows its title snapshot. |
| Toggle a blocked host on A and a category on P offline | Both apply everywhere; the browser extension and Android blocker follow. |
| Browse a limited site on A and use the limited app on P | Each device's limit counts both; values converge after reconnect. |
| Browser blocks a site during Focus on A | The block event appears in history on B after the run closes. |
| Inspect the vault during host activity | The host process never opens `ganbaru.sqlite`. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Seal after importing 1000 events | Under 3 s desktop |
| Apply of that batch | Under 10 s Android, reminders rescheduled within the same pass |
| Seal of a closed run with 40 segments and 300 block events | Under 100 ms desktop |
| Usage operations per device | At most one per source and day every 5 minutes |
| Activity operations | At most one per run transition |

## Risks

| Risk | Mitigation |
| --- | --- |
| Whole-row Calendar rewrites create spurious conflicts | Unchanged-group elision (milestone 2 step 10) with a simulation case for concurrent edits of different groups. |
| A recurring event with many overrides exceeds the blob bound | 256 KiB validation bound with an import diagnostic; the iCalendar object keeps the full source. |
| Calendar code writes Focus rows while Focus is unconverted | Steps 2 and 3 ship together; the gap exists only on `feat/sync`. |
| Cloned disks share a device id and undercount usage under `Max` | Accepted; writer forks are already handled, and usage is advisory. |
| Many components from a large import | One component per value, budgets measured in step 2. |
| Rules lost when development vaults upgrade | Documented reset; no external users. |
| Hooks that reschedule Android notifications are slow | Batched per apply, limited to touched events, measured on P. |

## Deferred

- Adaptive model replication or rebuild from replicated history.
- Remote Focus control and live companion status.
- Usage row compaction (milestone 10 compacts operation bodies only).
- Device attribution columns on block events.
- Periodic remote calendar subscription refresh.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- Settled from source: every event id derivation hashes the command id (`create.rs:58`, `plan.rs:311`, `task_schedule.rs:136`, archives at `deletion/write.rs:47-52`), and `copied_id` (`metadata/copy.rs:19`) hashes a parent id that already does, so no derivation is content-only. Step 1 adds a test that two command ids over equal content produce distinct ids.
- Settled from source: no code deletes `pomodoro_runs`, `pomodoro_segments`, or `pomodoro_run_events` (no `DELETE` on them in `crates/` or `apps/`); a path added later writes a tombstone through capture and its children hide by cascade.
- **Earlier step:** A closed run is never reopened and never keeps an `active` segment or open pause, including crash recovery (`observations.rs:467-499`) and Android resume. Rule: if any path reopens, it starts a new run instead.
- **Earlier step:** Component columns other than `calendar_id`, `projected_*`, and `updated_at` are never updated after insert. Rule: any other updated column moves into its own Register group.
- **Vault data:** The realistic encoded size of the largest timing value in the maintainer's vault and test fixtures. Rule: keep 256 KiB unless a real calendar exceeds it; then move override content (not recurrence ids or times) into a separate owned Register group.
- **Earlier step:** Whether any UI reads per-session usage. Rule: it reads Local samples and shows only this device; cross-device views read daily totals.

## Progress

- [x] Step 1: Calendar keys and bounds.
  - Branch `refactor/calendar-sync-keys` (worktree agent-a05ba4b0020f840d7). Commits: 1523e870 (backend, docs), 1708c1cd (i18n and client mapping), 4c6bf2cc (signed merge of feat/sync 8043d98f).
  - (a) Parameter value nodes now carry `property_id` in the import writer and the copy path (a legacy null is derived from the parameter owner). Readers no longer assume a node with a parameter has a null property. Fixtures updated. Import test asserts no ownerless value nodes. The NOT NULL change is left to step 2.
  - (b) `projected_kind` and `projected_id` are written on component insert (event, alarm, override components) in the import transaction via a planning pass; the separate link step is removed. Copy path already carried projection.
  - (c) `timing_bound.rs`: `MAX_TIMING_VALUE_BYTES` (256 KiB) and `TimingSize`, the one estimate function set. Edits are refused in `prepare_mutation`, creations, and retained deletion with message led by `calendar-timing-too-large`. Import skips projecting an oversized event, keeps the full source, adds a warning and an object diagnostic naming the event. Client maps the code via `timing-bound-error.ts` (en and es). Tests: exactly at bound and one over (unit, import with overrides, edit), one oversized event among normal ones.
  - (d) Held for created, scheduled, edit, split, archive, and copied event ids (all hash the command id); `command_identity.rs` asserts it. Not changed. Finding: date rows (exdate, rdate) added to a retained source derive from source id and date only (`copied_id(source, "exdate", date)`), so they are command independent; same-date adds converge on one row id. Left as is; test excludes them.
  - (e) Implementation check "Vault data: realistic encoded size of the largest timing value": branch held (far below the bound). Largest measured upper bound over all repo fixture VEVENT blocks (`apps/client/test-fixtures/ics/**`): 915 bytes (outlook-sample.ics), charging the whole VEVENT text; the bound is 262144. No maintainer vault on this machine.
  - Plan-silent decisions: error carried as a string prefix since backend errors are Strings; restore (`prepare_restore`) and direct exdate adds are not bound-checked; unknown generated ids charged 96 bytes, timestamps 40.
  - Checks: ganbaru-calendar lib 295 passed; ganbaru-tauri-app calendar 7 passed; clippy --all-targets -D warnings on ganbaru-calendar and ganbaru-tauri-app clean; cargo fmt --check clean; svelte check 0 errors; Vitest timing-bound-error 3 passed. After the merge, calendar lib tests re-run and pass.
  - Notes for step 2: the estimate in `timing_bound.rs` is Calendar's upper bound. Step 2 either adopts it as the contract or swaps in the codec's encoded size and reconciles the tests (`timing_bound.rs` unit tests, `events/tests/timing_bound.rs`, the import bound tests), so only one bound exists. Step 2's migration makes `icalendar_value_nodes.property_id` NOT NULL; no writer leaves it null now. Exdate and rdate ids are not command scoped.
- [x] Step 2: Calendar conversion.
  - Merges on feat/sync: 77363400 (part a), cdf5bc15 (part b), f948d1d2 (part c).
  - Open deviation (engine follow-up): Undo inside the deletion lease still publishes and then tombstones the archive copy when a seal runs inside the lease, because `sync_capture` only allows holds on deleted captures; the event itself publishes nothing.
  - Part a (data layer):
    - Status: complete at 2625782b, feat/sync a0de7409 already merged. Final checks passed: lib tests of ganbaru-sync, calendar, db, projects; tauri-app lib `calendar` (8) and `sync` (45); clippy all targets; fmt.
    - Earlier status: in progress. Branch `feat/sync-calendar` from feat/sync 49bafc67, worktree agent-ab30e5f526924df2c, not merged.
    - Commit d3caee85 (signed) `fix(calendar): bound exdate adds and restores, keep unchanged music assignments`:
      - Direct exdate gap closed: `archive::add_exdate` goes through `metadata::add_exdate`, which charges the date row against the timing bound and derives the row id from source and date (same derivation as a retained deletion), so a repeated exclusion is one row. Plan-silent choice: derived id instead of a random one.
      - `prepare_restore` gap closed: Undo checks the timing bound before restoring.
      - Music assignments: edits upsert by (owner_kind, owner_id, phase) with a no-change guard and remove only dropped phases, so an unchanged assignment captures nothing. Deletion and archive paths still delete (real tombstones). The generated `insert` carries a scoped `expect(dead_code)`.
      - Tests: exdate refused when its date row crosses the bound, Undo refused over the bound, edit leaves music rows untouched.
    - Commit 06af8662 (signed) `test(calendar): cover music assignment upsert and stale removal`.
    - Commit 8c3c103d (signed) `feat(sync): add domain-bounded owned tables`: `OwnedTable.bound` is `OwnedBound::Static` (the worst-case check) or `OwnedBound::Domain` (skips it; the domain's write validation keeps the value within `MAX_BLOB_FIELD_BYTES`, and seal reports a value that still exceeds as an invalid row, captured and unpublished). The summary adds ` bound domain` only for Domain, so earlier summaries are unchanged. Fixture table `fixture_scrolls` (id 60015) with a Domain-bounded owned table; tests for the static check and the seal path. Conformance codec samples for Domain tables shrink texts until the max-row sample fits one blob field.
    - Implementation check held: the only production update of `icalendar_components` sets `calendar_id` (`calendars.rs:210`).
    - Plan-silent decisions (coordinator, recorded here):
      - D1: owned size bound by opt-in `OwnedBound::Domain` in `manifest.rs` instead of a static worst case.
      - D2: one Calendar sync value bound (256 KiB) applies to every Domain-bounded group; oversized iCalendar component properties are dropped from preservation with a diagnostic, and the event is not projected only if the rest still exceeds.
      - D3: override, alarm, and attendee `icalendar_component_id` have no foreign key and no owned reference; each is a plain optional text column, and readers treat a missing component as none (plan deviation: the plan named an owned reference).
      - D4: nested parent-id foreign keys and multi-column unique constraints are dropped from owned tables; Calendar writers enforce uniqueness.
      - D5: events group id 27 stays unused (no existence group).
      - D6: `icalendar_objects.import_id`, immutable and random per import command, in the source group; backfill one id per (calendar_id, source_kind, source_name, created_at); a standalone object exports only when no linked object of the same source has a different `import_id`.
    - Commit bf345719 (signed) `feat(calendar): replicate calendar tables with keyed owned rows`: migration 20261023183413 rebuilds tables 160 to 166 and owned children (flat under the event, component, object, or archive; events cascade with their calendar; geo INTEGER millionths; `number_value` TEXT; `property_id` NOT NULL; `import_id` backfilled per batch; sync_key with RAISE triggers on 165 and 166; rendered capture triggers; foreign key check guard). Manifest version 8 with fixture v8, Calendar `replica_writable: false`. Calendar conversion pass (`archive-import-object`) and Projects `task-event` keys, both asserted at sync start and run at vault open (`db.rs` `convert_sync_keys`). Writers fill sync_key (restore, relationship commands, archive). Writers delete flat children explicitly where cascades were removed. ganbaru-db upgrade test (row counts, geo rounding, TEXT numbers, import_id grouping, placeholders, calendar cascade) and guard refusal test.
      - Plan deviations: archive override ids dropped; exdates and rdates are date-keyed sets (PK event and date, insert ignores duplicates); override properties, focus rhythms, and steps are flat under the event; one-row tables carry an `ordinal` fixed at 0; Calendar owned tables use the Custom codec; archive import objects take no Undo hold; components and objects are immutable except `calendar_id`.
      - App recovery restorer coverage test now covers replica-writable domains only, so part b's flag flip makes it demand Calendar restorers.
    - Merge 9b2164f2 (signed) of feat/sync at a0de7409 (brings migration 20261023162759).
    - Commit 3ba1f9d2 (signed) `feat(calendar): export one import per source when duplicates are linked`: D6 export rule, where the same source means the same (calendar_id, source_kind, source_name).
    - Commit 8ba93b22 (signed) `feat(calendar): bound every replicated event group by one sync value size`: `timing_bound` renamed `value_bound` (`ValueSize`, `MAX_VALUE_BYTES`, new stable code `calendar-value-too-large`). Edits, creations, splits, and Undo check every event group (alarms, attendees, organizer, categories, notifications, extended properties, Focus settings, music assignments, task links) besides timing; import leaves an event with an oversized group unprojected with a diagnostic and imports the rest.
    - Commit f617d5f5 (signed) `feat(calendar): bound preserved component graphs and object diagnostics`: import keeps warnings, then whole properties (with parameters and value nodes), while the component graph fits, and appends one counted omission warning; object diagnostics are bounded the same way.
    - Commit 9c0f1ff5 (signed) `feat(projects): bound task links per calendar event`. Plan-silent: a calendar event links at most 100 tasks and link ids are at most 256 bytes, enforced at the Projects link site; relinking an existing pair is exempt. This keeps the archived link copy under a quarter of the Calendar bound, leaving room for links added concurrently elsewhere.
    - Commit 133a01be (signed) `feat(calendar): charge ids at the copy allowance so archives fit their source bound`. Plan-silent: every id or `_id` column is charged at no less than the generated id allowance (96 bytes), because archives and split copies replace ids with derived 78 byte ids. Live admission therefore covers the archive form, including archive alarm and attendee rows that carry an extra source id; deletion never refuses. The override property timing charge now includes `override_id`. Tests archive the seeded event and compare each archive group, and each copied component graph, against the live charge.
    - Commit 912554e6 (signed) `test(sync): check calendar value estimates against the real codec`: ganbaru-calendar is a dev-dependency of ganbaru-sync only; every Calendar owned group value, filled with each sample shape of every owned table at once, encodes within the generic `ValueSize` charge.
    - Commit d5bd93d0 (signed) `fix(calendar): skip imported events that repeat an owned row key` (D4): owned keys are composite primary keys under their parent, so the database still refuses repeats; the import writer now skips an event that repeats an override recurrence id, override id, alarm id, or attendee id with a warning instead of failing the whole import, since override ids lost their primary key and override properties refer to them. Property maps dedupe by key; dates insert as sets.
    - Commit 38a46f19 (signed) `test(calendar): round trip geo coordinates through import and export`: degrees round to millionths once; exported payloads re-import to the same stored units.
    - Commit 36483579 (signed) `test(sync): simulate calendar edits, imports, removals, and held deletes across replicas`: title and color edits merge; concurrent timing edits give one conflict on every replica with one timing winning whole (owned exdates included); one UID imported on two replicas keeps one event with the survivor's alarm; remove then re-import keeps the new id; calendar removal against an event add hides the event and offers the calendar for recovery; delete against edit offers the event.
    - Plan deviation (open, for part b or the engine track): plan line 207 says Undo inside the lease publishes neither the event nor the archive tombstone. The event claim holds. The archive claim fails when a seal runs inside the lease: `hold::hold_deletes` holds only captures with `deleted = 1`, so the archive insert made in the deletion transaction seals, and Undo's archive delete becomes a real tombstone. Seal skips captures on `held_until_ms` alone (`seal.rs:1086`), so a fix can stay in `hold.rs` (hold the same-transaction insert capture of the archive too). The sim asserts only the event tombstone and documents the gap.
    - Commit cfd328db (signed) `test(sync): record calendar import seal and apply budgets`: ignored test, recorded never asserted. Release medians on this host: seal of 1,000 imported events (with one alarm each) 1.94 s against 3 s; apply on a second replica 1.33 s desktop against the 10 s Android budget (Android unmeasured).
    - Commit 2625782b (signed) `docs(calendar): describe the sync value bound, id charging, and task link cap`.
    - Client note: `apps/client/src/lib/calendar/timing-bound-error.ts` maps only the timing code; localizing `calendar-value-too-large` is client work for later.
    - Left for part b: project task link delete and reinsert in `metadata/write.rs` churns captures once it has a sync_key.
  - Part b (hooks, restorers, replica writes):
    - Landed: task link diff writes (`TaskLink::upsert` and `TaskLink::remove_stale` in `events/metadata/rows.rs`); an unchanged link captures nothing.
    - Landed: deletion holds in production. `PreparedCommit::write_held(tx, DeletionLease)` holds a removed source's event delete (`hold_deletes` on table 163, cascade children included) for `DELETE_UNDO_LIFETIME` on the wall clock; Undo calls `release_hold` and refuses ("already synced; restore from sync recovery") only when the event was published and the hold ended. `ganbaru-calendar` now depends on `ganbaru-sync`.
    - Landed: post-apply hooks. `ganbaru_calendar::sync_apply::with_hooks` rebuilds iCalendar projections (`projected_kind`, `projected_id`) for applied events and components; the app refreshes Focus Calendar context and emits `calendar-window-sync` (`{ sourceId: "sync", payload: { kind: "data-changed" } }`) after a pass that changed Calendar rows.
    - Landed: replica writes and recovery. Calendar `replica_writable` is true; `projects_link_task_event` and `projects_unlink_task_event` no longer require the active owner (the group delete and external folder removal gates stay). Restorers: an event comes back under a new id with its owned rows under new ids and its task links whose task exists; its calendar falls back to Local, a gone project or component is nulled, and `source_uid` is cleared only when a live event in that calendar holds it. A removed calendar restores its deleted events into Local and is not recreated (it comes back by adding or subscribing again). iCalendar objects and components refuse, so their offers can only be discarded. A restored task brings back its event links whose event exists.
    - Landed: timing conflict admission. `reads::focus_context::resolve` reads events whose open conflict mask (`OPEN_CONFLICT_MASK_SQL`) includes `event_group::TIMING`; automatic selection skips them, while a requested occurrence and the running owner are still selected and the day plan keeps them. Reminders are untouched, so they follow the winning value.
    - Landed: replica tests in `crates/ganbaru-sync-replica/src/tests/exchange/calendar_recovery.rs` (task deleted while linked elsewhere converges and restores with its link; event restore with new alarm ids; removed calendar restores events into Local). Docs: `docs/data/schema/sync.md`, `docs/data/sync.md`, `docs/features/projects/README.md`, `docs/features/calendar/README.md`.
    - Landed: conflict reads and resolutions. `ganbaru_calendar::conflicts` (`conflicts_from_pool`, `resolve_from_pool`) behind the app commands `calendar_conflicts` and `calendar_resolve_conflict`, registered on desktop and mobile and allowed in `app-commands`. A chosen timing version is written whole (columns plus excluded dates, added dates, overrides, and override properties, keeping the version's row ids) and the group is forced so the resolution seals. Replica tests in `exchange/calendar_conflicts.rs` cover a timing conflict resolved to either version (excluded dates, overrides, and override properties written and dropped) and a title kept as displayed, both closing on both devices. Also fixed `guarded_connections_refuse_local_tables_and_signal_commits`, which still probed `calendars`.
    - Commits: 56c79494, d1795983, 1c3d0ff3, 1c363193, e9f78813, b64e3e26, 0271ec0b, f7e59ef6. feat/sync had not moved (77363400), so no merge.
    - Final checks: `cargo test --lib` for ganbaru-sync (287), ganbaru-calendar (333), ganbaru-projects, ganbaru-sync-replica (all pass except the guarded probe `guarded_connections_refuse_local_tables_and_signal_commits`, broken by 1c363193 and fixed in 0271ec0b); tauri-app filters calendar (10), sync (47), projects (36) pass; clippy `--all-targets -D warnings` on ganbaru-sync, ganbaru-calendar, ganbaru-projects, ganbaru-sync-replica, ganbaru-tauri-app; `cargo fmt --check`; `pnpm --dir apps/client run check` (0 errors); Vitest vault state, owner-required, and mobile sync transport (26 passed). After 0271ec0b: ganbaru-calendar lib (336 passed), ganbaru-sync-replica (81 passed after f7e59ef6), tauri-app filters runtime (126) and permission/calendar/capabilit (20) pass, clippy `--all-targets -D warnings` on ganbaru-calendar, ganbaru-sync-replica, ganbaru-tauri-app, and `cargo fmt --check`. Pre-existing, not from this step: tauri-app `vault::handoff::receiver::tests::receive_modes_map_to_the_single_transport_purpose` asserts `PROTOCOL_VERSION == 10` but 99ac9db3 raised it to 11.
    - Plan-silent decisions: owned child rows of a restored event get new ids so two devices restoring the same offer never collide; `source_uid` is kept unless a live event in the target calendar holds it; Music assignments of a restored event (Music domain) are not restored; the post-apply notice reports Calendar rows only (Focus runs do not replicate yet); guard tests now use `notes_local_users` as the still-guarded table; a conflict resolution writes event rows outside the native Focus edit path, like applied sync rows, then emits `calendar-window-sync` and marks the Focus Calendar projection stale; a resolved timing version keeps its owned row ids so two devices choosing the same version write equal values; replica tests publish the built-in Local calendar first because tests never seed captures (sealing an event whose calendar the engine does not know fails on `calendar_id`).
    - For part c: recovery uses the existing sync recovery list, restore, and discard commands with `RecoveryEntryView { table, rowKey, title, preview, descendants, device, editedAtMs }`; Calendar offers carry table names `calendars`, `icalendar_objects`, `icalendar_components`, and `calendar_events`, and objects and components only accept discard (restore returns "preserved iCalendar data comes back by importing the file again"). Conflicts use new per-domain commands (no TypeScript wrapper yet; that is part c's): `calendar_conflicts({ dbUrl })` returns `{ conflicts: [{ eventId, field: "title" | "description" | "timing", versions: [{ version, device: DeviceRef, editedAtMs, displayed, value }] }] }` for every event, empty on a vault that does not sync, where `value` is `{ kind: "text", text }` or `{ kind: "timing", startTime, endTime, timezone, allDay, rrule, repeatUntil, excludedDates, addedDates, overrides }` (the last three are counts); `calendar_resolve_conflict({ dbUrl, resolution: { eventId, field, choice: { kind: "displayed" } | { kind: "version", version } } })` returns nothing and errors as `{ code: "resolved" | "failed", message }` (`resolved` means reload). Resolving emits `calendar-window-sync` itself. Views refresh on the `calendar-window-sync` event with `{ sourceId: "sync", payload: { kind: "data-changed" } }`. The `vaultOwnershipPrompt.replicaDescription` copy still says only Quick notes stay editable on a linked device.
    - Implementation checks: `cargo test -p ganbaru-calendar --lib` (327 passed); clippy `--all-targets` on ganbaru-calendar; `cargo check -p ganbaru-tauri-app`.
    - Blocked (engine track): the archive hold gap. Holding the archive insert violates `sync_capture.held_until_ms` CHECK `held_until_ms IS NULL OR (held_until_ms >= 0 AND deleted = 1)` from `20261022094729_sync_publish_holds.sql`. Relaxing it needs a `sync_capture` rebuild migration (capture triggers reference the column) and a review of consumers that assume held implies deleted (`captured_in` early-seal skip, `release_hold`, the insert trigger's reinsert branch). Consequence today: a seal inside the 5 s lease publishes the archive create, so Undo then tombstones the archive (the event itself publishes nothing). Once unblocked, `calendar_a_deletion_undone_inside_its_hold_publishes_no_event_tombstone` should also assert no `sync_tombstones` or `sync_rows` for table 164.
  - Part c (conflict and recovery UI):
    - Landed: typed wrappers for calendar_conflicts and calendar_resolve_conflict with validators and a resolved/failed error mapping; conflict badge (TriangleAlert) on grid events; lazy-loaded resolution dialog for title, description, and timing (timing shows start, end, zone, recurrence, and excluded, added, and override counts); event panel notice with the Focus timing conflict note (also in PomodoroSection); views and the conflicts store refresh on calendar-window-sync from sync; Calendar sync presentation (events and calendars restorable, iCalendar objects and components discard only, other tables neutral); calendar-value-too-large localized beside the timing code (bound-errors.ts, renamed from timing-bound-error.ts); vaultOwnershipPrompt.replicaDescription lists Calendar in en and es.
    - Commits: e6907965 (API, summaries, store), 07491bfd (badges, dialog, notice, i18n), 06efbbbb (sync presentation, baselines), 18c3c0ff (signed merge of feat/sync, Rust-only change).
    - Checks (under the lock): client check 0 errors; focused Vitest (23 files, 482 tests) passed; editor-check passed; bundle-contracts passed after baseline updates.
    - Bundle numbers: first-use route chunks (Projects, Notes, Chat) 377 -> 378 source modules (calendar-conflicts store reached from calendar.svelte.ts). Android: shell 152 -> 156, calendar 215 -> 220, projects 317 -> 321, notes 427 -> 428, quick-notes 140 -> 141, music 208 -> 209, chat 325 -> 326, total 1312 -> 1320. Measured growth from the new Calendar modules plus the sync presentation and its event module; the presentation notifies the Calendar store through calendar/sync-events.ts so the sync settings do not statically import the Calendar stores (a direct import pulled dompurify into the onboarding prewarm and a dynamic import created a chunk cycle).
    - Plan-silent decisions: no existing Focus admission explanation UI, so the timing note lives in the event panel conflict notice and PomodoroSection; the badge is a TriangleAlert icon; one store reads all conflicts and is invalidated by the sync-sourced reload and after each resolution; the panel closes after a resolution so no stale draft remains and Review is disabled while the panel has unsaved edits; value-too-large import warnings map to an event label or a generic omission message; strings live in the Calendar catalog (calendar.conflict, calendar.sync), not sync.ts.
- [x] Step 3: Focus history and activity.
  - Landed: Focus domain manifest (tables 224 to 227, replica writable), adapter and post-apply hook topic `focus`, migration 20261024045821_focus_replication (pomodoro_runs rebuild without the inherited_from FK, owned rhythm and pause shapes, pomodoro_device_activity, gated single-open triggers, rendered sync triggers), activity writes on start, phase change, pause, resume, and stop, desktop automatic admission skip, Focus gates on can_write_domain, `focus_device_activity` command, app Focus sync refresh, Focus menu activity line and start confirmation, Android bundle ceilings raised by the measured amount (deviation: device-activity.ts is new in the mobile closure and one existing module joined the shell and Calendar closures, so those rose by 2 and the total by 1), docs.
  - Commits (all signed): e5fa79c0 feat(sync): replicate closed focus runs and device activity; 7fe672ab feat(pomodoro): run focus on linked replicas and skip occurrences other devices focus on; 64790c12 feat(pomodoro): show other devices' focus activity and confirm starts; 03ead0dc test(sync): cover crashes while sealing or applying a closed focus run; cde8ed94 docs(sync): describe replicated focus history and activity; 304a563c style(sync): format focus crash test; 4690ffb3 test(android): raise bundle ceiling for focus device activity. feat/sync still 8ad8db4f, no merge needed.
  - Manifest version: 11 (v11 fixture added).
  - Checks: lib tests pass for ganbaru-pomodoro, ganbaru-sync, ganbaru-db, ganbaru-calendar, ganbaru-sync-replica, ganbaru-tauri-app; clippy workspace all targets, fmt check, ganbaru bin check, client check, editor-check, focused Vitest (20 tests), build, and bundle-contracts pass.
  - Budgets: activity writes are one op per transition (test). Focus seal of a closed run with 40 segments and 300 run events measured 262 ms and 277 ms median on two release runs against the 100 ms budget: missed. Per-row cost matches the engine seal (about 0.78 ms per row for about 345 captured rows, against about 0.8 ms per row for the 100-note seal), so the Focus adapter adds nothing of its own and the fix belongs to the engine track (seal.rs is out of scope). The same runs also missed the notes seal (72 to 84 ms against 20 ms) and Projects apply (486 to 560 ms against 50 ms) budgets, which this branch does not touch.
  - Plan-silent decisions: plan, outcome, and inheritance groups are Register; inherited_from_run_id is plain text; partial unique indexes became gated triggers; enum CHECKs moved to the adapter; pause ids are ordinals within the run; activity is written only when its projection changes (idle device or no device id writes nothing); a run closed on a split occurrence keeps its closed occurrence; Calendar edits through the native owner need both calendar and focus domains writable; the admission skip applies only to automatic starts, explicit starts use the frontend confirmation from the activity list read at menu mount and refreshed on `focus` applied notices; the Focus hook reuses the Calendar publish_change, no restorers.
  - Notes for step 6: block events (table 279) should publish with their closed run like segments and run events, through published_with on the run in the Focus manifest; Focus seal cost already exceeds its budget, so adding rows to the run seal raises it further unless the engine seal is faster.
- [x] Step 4: distraction rules.
  - Landed so far: manifest tables 272 to 277 with the owned `distractions_usage_limit_entries` (generic `(limit_id, ordinal)` shape), adapter, rules hook (topic `distractions_rules`, key = table name), migration `20261023233521_distraction_rules.sql` with seeded `vault` settings row and rendered triggers, `VAULT_MANIFEST_VERSION` 9 with `vault_manifest_v9.txt`, replica scenarios in `crates/ganbaru-sync/src/tests/distractions.rs`.
  - Landed: `ganbaru-distractions::config` (assembled `DistractionsConfig`, defaults from the shared `categories.json` with `defaultEnabled`, normalization) and `config::store` (`read`, `apply(RuleEdit)` in one transaction, `RuleRefusal` codes matching Settings results).
  - Landed: ten fine-grained `distractions_rules_*` commands (load, set mode, set enabled, add hosts, add desktop app, add mobile app, remove, save custom category, save usage limit, reset) on desktop and mobile with permissions; whole-config save and the `distractions` branch of `config.json` removed (config.json keeps only `chat`; the `VaultConfig` patch scope is refused); desktop blocker, close authorization, limits read, and Android projection read one assembled SQLite snapshot guarded by a process-wide rules revision; the native host reads a device-local `distractions-rules.json` the desktop owner publishes; sync applies and recovery restores of distraction tables republish through the same path as local edits; recovery restorers for custom categories and usage limits; `stable_device_id` in `sync/devices.rs`.
  - Landed: frontend typed wrappers (`lib/api/distractions.ts`) with strict runtime validation (`parseDistractionsConfig`, `parseDistractionsRulesEditOutcome` in `lib/distractions/rules.ts`, tests check refusal codes against `store.rs`); the Settings store keeps its synchronous API and local validation, shows edits at once, runs commands through one ordered queue, takes the newest saved rules, reloads after a refusal or failure, and reloads on sync applies of the rules tables or topic and on a `distractions-rules-changed` window channel; creates (custom category, usage limit) wait for the native id instead of showing a temporary one; Android copy publishing no longer flushes `config.json`. Docs: schema (supporting domains, sync), sync, architecture, application boundaries, distraction feature, Android native data, features status, AGENTS.md.
  - Commits: `1a07a190 feat(sync): replicate distraction rule tables`, `27792144 feat(distractions): persist rules in SQLite`, `74cf97b5 feat(distractions): edit and read rules through SQLite commands`, `8ffd429c feat(distractions): edit rules from Settings through SQLite commands`, `4fe298f0 docs(distractions): describe rules as replicated SQLite data`.
  - Plan-silent decisions (frontend): refusals only the backend can produce (such as `too-large`) reload the stored rules with a console warning and no user-facing message; the desktop store loads after vault bootstrap because `App.svelte` is imported dynamically, and every Android Guardian pass reads the SQLite snapshot itself, so startup needs no frontend publication of rules.
  - Frontend checks: client check, focused Vitest (rules, config, api distractions, mobile distractions), bundle contracts pass. Final: ganbaru-sync lib (290), ganbaru-db lib (131), `cargo check -p ganbaru --bin ganbaru`, `cargo fmt --check` pass; feat/sync unchanged at f948d1d2, so no merge.
  - Checks after `74cf97b5`: ganbaru-distractions lib (34), ganbaru-tauri-app lib (1030), ganbaru-native-messaging bin (46), workspace clippy `--all-targets`, `cargo fmt --check` pass.
  - Plan-silent decisions (backend wiring): the rules revision is bumped after the SQLite commit, so a reader that captured it just before can act once on the old rules within one observation tick; the native host reads `{ "vaultPath", "distractions" }` from the app config dir with a 4 MiB bound and ignores it unless `vaultPath` equals the active vault path; edits return `saved` with the assembled rules or `refused` with a reason code instead of an error; commands use `connect_vault` and the `can_write_domain` guard; native default categories come from `defaultEnabled`; `ganbaru-distractions` depends on `ganbaru-sync` for manifest keys and recovery decoding; recovery restores a category or limit under a new id, suffixes a taken category name with the lowest free number from 2, keeps the retained enabled state, and refuses the seeded settings row; the republish hook lives in the app host's `emit_applied`, matching distraction table names or the rules topic, so restores (which only emit applied tables) also republish.
  - Implementation checks: ganbaru-sync lib (full suite, then conformance and distractions after the last DDL change) pass; ganbaru-db lib pass; ganbaru-distractions lib (33) and clippy pass.
  - Plan-silent decisions: host rule key inputs are `(list_kind, scope, host)` with non-null `scope` ('' for global lists, custom category id for category hosts), `category_id` stays a cascading reference; built-in category rows keyed by category id with no prefix and no seed rows (missing row means default); app identity is the package for mobile and the lowercase name for desktop; timestamps are `created_at_ms` integers; entries ordered by `ordinal`; multi-column and JSON checks are enforced by Rust and the adapter, not SQLite, since conformance forbids unresolved multi-column checks; `match_names_json` columns are explicit exceptions in the JSON storage schema test. Saving a custom category diffs its hosts so kept hosts keep their enabled state and row; saving a usage limit rewrites entries only when they changed; a usage limit is refused (`too-large`) when an upper bound of its encoded entries exceeds `MAX_BLOB_FIELD_BYTES`; mobile package identity keeps its case and is deduplicated case-insensitively; assembly skips custom categories without hosts, limits without entries or budget, unknown category ids, and invalid hosts or entries instead of failing; toggling a rule that no longer exists is a no-op.
  - Notes for step 5: the settings row is seeded by migration and captured by `seed_captures` at vault open on every replica, so concurrent toggles of different groups survive; without the seed, first edits would be competing whole-row creates.
- [x] Step 5: distraction usage.
  - Landed on feat/sync-distraction-usage (worktree agent-aad44433139cb1eb4). Replicated daily usage totals (table 278 `distractions_usage_totals`; Max seconds, Register label, Immutable bucket), samples are Local, a flush planner (at most every 5 minutes per bucket, plus forced flushes on desktop exit, Android background through the Suspended window event, and before snapshots), and limits read other devices' totals plus this device's samples and pending spool or Guardian events. The owner drain, linked import (linked.rs), the exchange messages, and the transport and coordinator arms are removed. Settings shows a localized (en, es) Notice when a rule edit is refused or fails, which clears on the next saved edit.
  - Commits: 2d58ef72 feat(distractions): replicate daily usage totals per device; 89220e89 refactor(distractions): replace the usage exchange with replicated totals; 579b3803 feat(distractions): explain refused or failed rule edits in Settings; 85e2bf18 docs(distractions): describe replicated usage totals and the removed exchange; 4b6da953 merge of feat/sync (488cad46); b5832c16 test(android): raise bundle ceiling for distraction edit status; 9b995b6c docs(sync): list distraction usage totals as replicated (AGENTS.md status lines and workspace map). All signed.
  - Versions taken: table 278, VAULT_MANIFEST_VERSION 10 (fixture vault_manifest_v10.txt), handoff PROTOCOL_VERSION 12, migration 20261024015213_distraction_usage_totals.sql, Android maxTotalSourceModules 1321.
  - Checks after the merge (all under the lock): cargo fmt clean; lib tests ganbaru-db 133, ganbaru-distractions 41, ganbaru-handoff 98, ganbaru-sync 301 (3 ignored), ganbaru-sync-replica 81, ganbaru-tauri-app 1019; ganbaru-native-messaging bin 46; clippy workspace all-targets clean; cargo check -p ganbaru --bin ganbaru clean; client check 0 errors; vitest edit-status, rules, catalog-shape 54 passed; pnpm -w run bundle-contracts passed (the client-only script needs the Turbo build first).
  - Plan-silent decisions: the migration clears existing samples; device_id is bounded by MAX_DEVICE_ID_BYTES and elapsed seconds are clamped at 90000; totals are excluded from the rules republish hook; old spool tables are not dropped (no shims), and a test asserts a fresh spool has only 2 tables; the tick keeps its "no budgets, no drain" gate while exit, background, and snapshot flushes are forced; the label is MAX(display_name); spool sample ids are kept as-is; Android import applies the retention window and insert_own_samples prunes older samples; devices that cannot write read the vault plus pending spool or Guardian events; the Android refresh read takes no write permit; the background flush is spawned from the Suspended hook, not block_on; flush_active_vault takes the replicated write permit and rechecks the vault id; the edit-status Notice uses tone warning for failures too, since only neutral and warning exist; the Android bundle ceiling rose by 1 for the new edit-status module.
  - Notes for step 6: block events are unconverted and stay owner-only; runtime snapshots stay device-local. A cloned installation sharing one device id undercounts because both write the same total keys. The native-messaging spool error text still says "linked-device" (left unchanged, since that crate was out of scope). The docs/data/sync.md merge conflict kept the feat/sync bullets and says protocol 11 and later refuse the removed exchange messages.
- [x] Step 6: block events and the host spool.
  - Landed: table 279 `distractions_block_events` (event 0 Immutable, run 1 Immutable cascade reference to 224, segment 2 Reference to 225 SetNull, snapshot 3 owned Immutable), published with its Focus run with null publishing at once; migration rebuilds run_id to ON DELETE CASCADE and the snapshots to `(block_event_id, ordinal)`; adapter validates source type, phase, decision, URL-free source key, and snapshot kinds and modes.
  - Host: `linked_usage.rs` renamed to `spool.rs`; the host reads only `app-state.json` and `vault.json`, writes usage samples and block events (`pending_block_events`, cap 4000 per vault and device) to the device spool, and never opens the vault. `native_vault_is_writable` and the ownership state reader are gone; error texts name the usage spool instead of a linked device.
  - App: `record_spooled_block_event` in ganbaru-distractions validates every spooled row against the capture bounds, nulls a run the vault no longer holds, and writes with INSERT OR IGNORE (desktop snapshot insert switched from INSERT OR REPLACE too). The app spool ingests batches of 200 (vault commit first, then spool delete), dropping invalid rows with a log line; it runs on the 5 s desktop tick behind an EXISTS gate and in `drain_and_flush` (usage read, exit, snapshots). Desktop block inserts and ingestion gate on `can_write_domain` with `acquire_replicated_write`. Android `usage_only` removed in Rust and Kotlin.
  - Commits: 0dbfbd61 feat(sync): replicate distraction block events with their Focus run; a274e30d feat(distractions): spool browser block events and ingest them into the vault; e3179fcd test(sync): time the closed Focus run seal with its block events; 44ca6b00 docs(distractions): describe replicated block history and the host spool. All signed.
  - Manifest version taken: 12 (fixture `vault_manifest_v12.txt`).
  - Measurement: release closed Focus run seal with 200 block events, median 438 ms (421, 425, 438, 686, 695) against the 100 ms budget; it seals two operations (Focus and Distractions). The budget already failed before this step (262 to 277 ms without block events). Not tuned, per the brief.
  - Plan-silent decisions: segment_id SetNull; snapshot enum CHECKs dropped in favor of adapter and ingestion validation; ingestion nulls a missing run; invalid spool rows are dropped and logged; `created_at` equals `occurred_at`; the desktop tick gates on an EXISTS query rather than file mtime.
  - Docs: application-boundaries, integrations, native-backend, platforms/desktop, data/sync.md (with the required sentence), supporting-domains, plus stale owner-only sentences in features/distractions README and browser.md and platforms/android native-services-and-data.
  - feat/sync did not move from 04375141; no merge needed.
  - Final checks, all green under the lock: native-messaging bin 43; `--lib` ganbaru-distractions 43, ganbaru-sync 345 (6 ignored), ganbaru-db 141, ganbaru-pomodoro 88, ganbaru-sync-replica 83 (1 ignored), ganbaru-tauri-app 1024; workspace clippy with all targets and `-D warnings`; `cargo fmt --check`; `cargo check -p ganbaru --bin ganbaru`.
  - Notes for step 7: AGENTS.md's concurrent sync list does not mention distraction block history yet. Kotlin `usageOnly` removal is not build-verified here (no Android build). The Focus closed-run seal budget needs engine work to meet 100 ms.
- [x] Step 7: docs, validation, and automated acceptance.
  - Date: 2026-10-24.
  - Branch: `docs/sync-milestone-7` from `feat/sync` 0dacb4e4.
  - Commits (all signed): d6ce194d `fix(distractions): drop refused spooled block events and refuse a segment without its run`; e2c4651f `docs(sync): describe replicated Calendar, Focus, and block history across the docs`; a631d6bc `fix(pomodoro): keep the device activity reader out of the overlay command graph`; 39360714 `merge: feat/sync into docs/sync-milestone-7` (feat/sync 97192d32, no conflict; `budgets.rs` keeps both the closed continuing group chain and the 40 segment, 300 block event Focus workload); a8cd032b `test(client): raise bundle ceilings for focus device activity`.
  - Validate fixes: step 3 (64790c12) made `$lib/api/focus`, which Focus overlay windows import, reach `focus_device_activity` and the sync commands, failing `focus-overlay-commands.test.ts`. The reader moved to `$lib/api/focus-device-activity`, so overlays keep their narrow permission set. The first-use route ceilings (378 to 380) and the Android total (1322 to 1323) were raised for the step 3 modules and that split.
  - Review follow-ups (d6ce194d):
    - Spool ingestion records each block event and its snapshot in a savepoint. A constraint or trigger refusal (SQLite primary code 19) drops and logs the row while the rest import; any other vault error keeps the whole batch in the spool. Validation refuses NUL in every text field, blank or oversized row keys, and oversized timestamps.
    - `SyncAdapter::validate_row` (default Ok) is called from `validate::change` on full-row writes; the Distractions adapter refuses a block event with a segment but no run. Ingestion never produces one.
    - The budget workload is now 40 segments, 40 run events, and 300 block events (`#[ignore]`, release-only assertion); it asserts 382 sealed changes.
    - Tests: `ganbaru-db` `block_history_migration_keeps_events_and_rule_snapshots`; app `spool::tests::block_events_the_vault_refuses_are_dropped_and_the_rest_import`, `a_vault_failure_that_is_not_a_refusal_keeps_the_batch_in_the_spool`, `a_block_event_ingested_during_an_open_run_publishes_when_the_run_closes`; `ganbaru-sync` `domains::distractions::tests::a_block_event_names_a_segment_only_with_its_run`; 16 validation cases in `ganbaru-distractions` `usage/tests.rs`.
  - Docs touched: `AGENTS.md` (intro, Sync convention, `distractions/` map line); `docs/data/sync.md` (status, block segment rule, storage ownership rows for the distraction blocker and Focus, Calendar reminder rule, Focus delivery row, Calendar and Distractions delivery rows); `docs/data/schema/sync.md` (replicated list); `docs/data/schema/calendar.md` (new Replication section); `docs/data/schema/supporting-domains.md` (savepoint ingestion); `docs/algorithms/pomodoro/focus-authority.md` (status, controller ownership section replaced by no remote control); `docs/algorithms/pomodoro/README.md`; `docs/algorithms/calendar/time-conflict-detection.md`; `docs/features/README.md` (sync paragraph); `docs/product/system-map.md`. Already current and left alone: `schema/pomodoro.md`, `features/calendar/README.md`, `features/pomodoro/README.md`, `features/distractions/{README,usage-limits,browser}.md`.
  - Acceptance mapping (S = `crates/ganbaru-sync/src`, CAL = `crates/ganbaru-calendar/src`, POM = `crates/ganbaru-pomodoro/src`, DIS = `crates/ganbaru-distractions/src`, NM = `crates/ganbaru-native-messaging/src`, APP = `apps/client/src-tauri/app/src`, FE = `apps/client/src/lib`):
    - Offline edits on P, reconnect, reminders rescheduled: physical only (Android alarms and network). Partial: `APP/calendar/sync.rs::calendar_sync_refreshes_for_any_calendar_table_and_ignores_other_domains`, `FE/scheduling/mobile-calendar-notifications.test.ts`.
    - Title and color edits both survive: `S/tests/calendar.rs::calendar_concurrent_title_and_color_edits_both_survive`.
    - Same event moved to different times: `S/tests/calendar.rs::calendar_concurrent_timing_edits_surface_one_conflict_on_every_replica`, `CAL/reads/focus_context/tests.rs::native_focus_skips_automatic_admission_of_an_event_with_an_open_timing_conflict`. Gap: no test that reminders follow the winner.
    - One file imported on two replicas: `S/tests/calendar.rs::calendar_one_file_imported_on_two_replicas_keeps_one_event_per_uid`.
    - Calendar removal against a concurrent add: `S/tests/calendar.rs::calendar_removal_against_a_concurrent_event_add_hides_the_event_and_offers_recovery`. Gap: Restore into Local not tested end to end.
    - Undo inside the hold: `S/tests/calendar.rs::calendar_a_deletion_undone_inside_its_hold_publishes_no_event_tombstone`, `S/tests/holds.rs::undo_inside_the_lease_restores_a_held_delete_without_a_tombstone`.
    - Hidden calendar stays per device: generic only (`S/tests/conformance.rs::replicated_columns_are_partitioned`, capture `local_and_derived_columns_and_unchanged_values_are_not_captured`).
    - Open run invisible until closed: `S/tests/focus.rs::focus_open_runs_and_heartbeats_capture_nothing`, `focus_closing_a_run_publishes_it_with_its_children_in_one_seal`.
    - Start warning names the other device: `POM/execution/tests/activity.rs::activity_reads_return_only_fresh_rows_of_other_devices`, `FE/pomodoro/device-activity.test.ts`. The dialog itself is physical.
    - Power loss mid run: `activity_is_written_on_each_transition_and_expires_as_specified`, `POM/tests/recovery.rs::crash_recovery_closes_run_and_segment_at_last_heartbeat`. Real power loss is physical.
    - Run closed for an event deleted elsewhere: `S/tests/focus.rs::focus_a_run_closed_for_an_event_deleted_elsewhere_arrives_without_its_event`.
    - Rule toggles on two devices: `S/tests/distractions.rs::distractions_toggles_of_different_settings_from_two_replicas_both_survive`, `distractions_concurrent_toggles_of_one_rule_converge`, `NM/tests.rs::reads_enabled_host_rules`. Real enforcement is physical.
    - Cross-device limit: `distractions_usage_totals_converge_to_each_devices_largest_flush`, `DIS/limits/tests.rs::budgets_count_other_devices_totals_with_own_samples_but_not_own_totals`.
    - Block history with its run: `distractions_block_events_publish_at_once_or_with_their_focus_run_and_leave_with_it`, `APP/distractions/spool/tests.rs::a_block_event_ingested_during_an_open_run_publishes_when_the_run_closes`.
    - Host never opens the vault: `NM/tests.rs::block_events_are_spooled_without_opening_the_vault_or_keeping_full_urls`, `usage_samples_are_spooled_outside_the_vault_until_the_app_acknowledges_them`. Live process inspection is physical.
    - Testing, schema and failpoints: `S/tests/conformance.rs` (`every_schema_table_is_classified`, `owned_codecs_round_trip_through_the_schema`, `installed_triggers_match_rendering`), `focus_a_crash_while_sealing_or_applying_a_closed_run_leaves_no_partial_history`, `S/tests/engine.rs::failpoints_roll_back_every_entry_point`.
    - Testing, scenarios: hand-written two-replica tests in `S/tests/{calendar,focus,distractions}.rs`; `S/tests/sim.rs` randomizes authorization and topology only.
    - Testing, Calendar conversion: `CAL/geo.rs::degrees_round_trip_through_stored_units`, `CAL/stored_number.rs::stored_numbers_round_trip_integers_and_fractions_exactly`, `CAL/conversion.rs::conversion_derives_archive_import_object_keys_once`, `S/tests/engine.rs::derived_key_pending_placeholders_are_refused_at_startup_and_rekeyed_once`, timing bound tests in `CAL/events/tests/timing_bound.rs` and `CAL/import.rs`, `CAL/sync_apply.rs::an_applied_event_projects_its_linked_components`, `CAL/events/tests/complete_archive.rs::removing_the_original_calendar_preserves_archived_import_envelopes_and_components`. Gap: no Calendar test that a rewrite keeps ids of unchanged owned rows.
    - Testing, Focus: the Focus seal and capture tests above plus POM activity tests.
    - Testing, Distractions: `DIS/config/tests.rs::rule_writers_refuse_what_blockers_cannot_enforce`, `fresh_vault_assembles_the_settings_defaults` (gap: equality with the previous `DistractionsConfig` default is not asserted field by field), the limits test above, spool idempotence tests, NM vault tests.
    - Testing, frontend: `FE/calendar/conflicts.test.ts`, `FE/pomodoro/device-activity.test.ts`, `FE/api/distractions.test.ts`.
  - Budgets:
    - Calendar import seal of 1000 events: 1.64 s after engine follow-up 3 (was 1.94 s; budget 3 s, met).
    - Apply of that batch: 1.71 s desktop after follow-up 3; Android unmeasured (budget 10 s).
    - Closed-run seal, release on the merged tree, 40 segments, 40 run events, 300 block events: median 187 ms of 178 to 226 ms (budget 100 ms, missed). Was 438 ms with 200 block events before follow-up 3. Meeting it needs cross-row batching of engine statements (follow-up work); seal of 100 notes is 64 ms against 20 ms for the same reason.
    - Activity writes: one operation per transition (tested). Usage: at most one operation per source and day every 5 minutes (flush planner).
  - Android: no SDK on this host, so no device build. Kotlin review of a274e30d: `usageOnly` removed consistently from `DistractionsGuardianBridge.kt`, `DistractionsJournal.kt`, and `MobileDistractionsPlugin.kt`; `android.rs` commits the import before acknowledging the Guardian. Device-build checklist: the plugin compiles without `usageOnly`; one Guardian export returns usage and block kinds; a block during a running Focus session reaches the vault with its run; acknowledgement happens only after the commit. Gap: Android import validates and imports the whole batch in one transaction, so one invalid or refused Guardian event stalls the batch unacknowledged (same class as the spool fix; predates step 6, not fixed here).
  - Validate (standard profile, merged tree 39360714): check, Rust tests, both Vitest shards (355 and 354 files, 2770 and 3172 tests), and editor check passed; bundle contracts failed on the module ceilings, fixed in a8cd032b, and `pnpm run bundle-contracts` then passed. Before the merge, validate failed only on the overlay command test, fixed in a631d6bc.
- [x] Milestone 7 review fixes.
  - Branch `fix/sync-milestone-7-review` from feat/sync 3258767c, merged with feat/sync f900d1c2 in 6da3183e.
  - Finding 1 (high, Android Guardian batch stall) and low 5: journal events settle one at a time, so one refused event no longer stalls the batch; `drain_and_flush` flushes totals even when a spool step fails and returns the first error afterwards. Commit 50845bb0 `fix(distractions): settle Guardian journal events one at a time`.
  - Finding 2 (medium, headless sync does not reschedule reminders or refresh rules): documented as a limitation. Decision: the background Android pass (`HeadlessHost`) has no `AppHandle`, and alarms are built in the frontend with localized copy and plugin calls, so the pass cannot reschedule them; reminders, Android alarms, and Guardian rules it changes refresh when the app next opens, and Guardian keeps enforcing the rules it last applied. Commits 85e08551 `docs(sync): note background Android pass refresh limits` and 307f61dd (docs/data/sync.md).
  - Finding 3 (medium, `.ics` re-import regenerates child row ids): import now plans each event as a diff against stored rows (`crates/ganbaru-calendar/src/import/diff.rs`). Child rows matching a stored row by identity keep that row and its id, unchanged events are not written, and preservation objects are replaced (with relinking) only when the source differs. Remaining limitation: a changed source still mints new component ids, so its link columns are rewritten. Commit 154da71d `fix(calendar): diff re-imported event rows against stored rows`; doc in docs/interop/icalendar/preservation-and-export.md.
  - Finding 4 (low): `rule_store::on_sync_changes` republishes only for rule table changes. Commit 31fba03c `fix(distractions): republish rules only for rule table changes`.
  - Finding 5 (docs): Focus ownership wording, background sync limits, crate map. Commit 307f61dd `docs: align Focus ownership wording, background sync limits, and crate map`.
  - Finding 6 (tests): commit 98b76a6b `test(sync): cover Calendar re-import, duplicate repair, and restore into Local`. Unchanged re-import keeps an alarm another replica added; duplicate repair test; Restore into Local of a calendar removed while another device added an event to it.
  - Deviation (finding 6): duplicate repair moves Focus runs to the kept event, but task links to the hidden duplicate stay hidden (the event is part of the link identity) and Music assignments keep the hidden event id (owners are opaque ids). The test pins this, and docs/data/schema/calendar.md now states it instead of "references follow the redirect". Making links and assignments follow needs an engine or Calendar adapter decision.
  - Device checklist: after a background Android sync pass changes a reminder or a distraction rule, confirm alarms and Guardian rules refresh on the next app open and Guardian keeps the last applied rules until then. Import the same `.ics` on two devices with a task link and a Music override on each copy, and confirm which survive. Re-import an unchanged `.ics` while another device adds an alarm, and confirm the alarm survives.
  - Delta review: land with follow-ups (merge f5db2ce7). Low doc overclaim on re-import fixed on feat/sync. Follow-up before any tombstone compaction or user-capable release: duplicate repair must move task links (insert survivor link, delete loser link in the same sealed write) and Music assignments to the survivor through an adapter callback in `repair.rs` (engine) implemented by the Calendar adapter (domain); interim alternative is offering the hidden links and assignments as recovery entries.
- [ ] Step 8: physical acceptance.
