# Milestone 6: Projects core and working folder bindings

**Status: Partial.** Steps 1 to 6 are implemented in source on `feat/sync`, with review fixes and docs merged; physical acceptance (step 7) is pending. Part of the [concurrent sync goal](README.md). Projects and working folder bindings become concurrently editable on every linked device. The contract lives in [Device linking and synchronization](../../docs/data/sync.md), [Sync merge rules](../../docs/algorithms/sync/README.md), [Sync schema](../../docs/data/schema/sync.md), [Notes and Projects schema](../../docs/data/schema/notes-and-projects.md), and the [Projects feature docs](../../docs/features/projects/README.md).

## Outcome

- Groups, projects, sections, statuses, priorities, tags, tasks, checklist items, custom fields and their values, dependencies, tag links, views and their portable preferences, custom emojis, task history, and working folder bindings replicate through the engine, and any linked device edits them offline.
- Concurrent edits converge with no silent loss: task titles and descriptions surface conflicts, a project deleted on one device while edited on another becomes a recovery entry, and a dependency cycle created by two devices is repaired the same way everywhere and shown to the user.
- External folder paths, filesystem identity, trust, and last selection stay device-local. A received project gets its managed folder on first use.
- Projects no longer create Chat rows through SQLite triggers, and Chat FKs into Projects no longer block replicated deletes.

## Scope

In:

- The tables listed in [Manifest](#manifest), table ids 96 to 114.
- Order keys for every Projects sort order, soft deletion for statuses and priorities, derived keys with existence registers for dependencies, tag links, custom field values, and view preferences, and fixed-point custom field numbers.
- Removing the Chat-creating triggers on `project_working_folders`, lazy Chat defaults, and rebuilding every `NO ACTION` FK into Projects tables as `CASCADE` or `SET NULL`: the Chat FKs that point at `projects` and `project_working_folders`, and `project_tasks.section_id` and `status_id`.
- `sync_key` on the four composite-key tables and the Rust conversion pass that fills it.
- The `project-icons` asset class, Projects conflict and recovery presentation, and Projects write paths on replicas.

Out:

- `project_task_event_links` and the Calendar writes in scheduling commands ([milestone 7](milestone_7.md)). `calendar_events.project_id` stays a Calendar column.
- Chat organization ([milestone 9](milestone_9.md)). Chat tables stay Unconverted; this milestone only changes their FK actions and stops creating their rows from triggers.
- Notes project folders and project history ([milestone 8](milestone_8.md)).
- Managed folder contents (README decision: device-local working copies, [milestone 10](milestone_10.md)).
- Removing the vault owner for actions that cascade into guarded domains (milestone 10).

## Prerequisites

- [Milestone 2](milestone_2.md): adapter registry and data-driven conformance, the Local table and column class with a declared materialization value per Local column, and a per-domain replica write flag in place of the single `replicated_writes` boolean (step 1); derived keys with existence registers and the composite row key convention, `sync_key` filled by `derived_key` in `ganbaru-sync-contracts` (step 4); Tree with cycle repair and a maximum depth, Acyclic edge sets for derived-key edge tables, and cascade hiding with repair tombstones and recovery against the cause's context (step 5); `seed_captures` at vault open and batched sealing (step 6); generic owned children keyed `(owner, ordinal)`, coupled groups that contain owned tables and Reference columns, and post-apply hooks (step 7); asset classes and `asset_refs` with streamed transfers (step 8); dependency-ordered apply, the guard skip while applying, and conflict and recovery presentation. Projects edits no shared engine file; a missing shape found later goes back to the engine track.
- [Milestone 5](milestone_5.md): Music tables are Replicated, so `focus_playlist_id` and `break_playlist_id` can be References, and `music_context_assignments` written by `update_project` is captured as a Music operation.

## Current state

Verified on 2026-10-21. `B` is `crates/ganbaru-db/migrations/20261004220000_baseline_schema.sql`.

- **Tables.** `project_groups` B:15, `projects` B:29 (pomodoro default CHECK spans six columns, playlist, ruleset, and work environment ids have no FK), `project_sections` B:159, `project_statuses` B:172, `project_priorities` B:185 (primary key `(project_id, id)`), `project_tags` B:197 (`UNIQUE (project_id, lower(name))`), `project_tasks` B:209 (`section_id` and `status_id` `NO ACTION` and `NOT NULL`, `priority` free text with no FK, `parent_task_id` `CASCADE`, REAL `section_sort_order` and `status_sort_order`, a CHECK coupling `start_date` and `target_end_date`, time CHECKs coupling each time to its date), reminder columns from `20261011200000_project_task_reminders.sql` (each coupled to its date), `project_checklist_items` B:262, `project_task_change_events` B:273, `project_task_dependencies` B:298 (random `id`, unique triple), `project_task_tag_links` B:319, `project_custom_fields` B:327 and options B:356 (case-insensitive unique names), option values B:370, values B:380 (CHECK requires exactly one scalar, so select types store only option value rows; `number_value REAL`), `project_working_folders` B:400, `project_primary_working_folders` B:440 (composite FK to folders `NO ACTION`), `project_custom_emojis` B:452. `project_views` and the rebuilt `project_view_preferences` come from `20261013120000_project_views.sql:6-45`, with notes and chat views unique per project (:16) and seeded as `view-<project id>-<type>` (:19). `insert_default_views` (`crates/ganbaru-projects/src/views.rs:22-50`) writes that form for new projects, but `create_view` (`views.rs:77-113`) stores the client's random id (`apps/client/src/lib/stores/projects/actions.ts:1137`) for every type, and any view, notes and chat included, can be removed (`views.rs:188`) and added again.
- **Custom field numbers.** `number_value` is a finite `f64` in Rust (`models.rs:422`, `:1087`, checked at `validation.rs:469`, bound at `custom_fields.rs:291-310` and `:359`), sorted at `task_views.rs:382`, and aggregated as REAL at `task_views.rs:664-667`. The frontend carries a JSON number (`apps/client/src/lib/api/projects.ts:251`, `:676`, `lib/projects/types.ts:403`, `:586`), parses drafts with `Number` (`lib/projects/tasks/detail.ts:320-334`), and formats and compares it in `lib/projects/list/presentation.ts:148`, `lib/projects/custom-fields.ts:67`, `lib/projects/tasks/view.ts:359-360`, and `components/projects/list/ProjectListColumnCell.svelte:206`, `:233-246`, `:758`.
- **Receipts.** `project_dependency_cascade_receipts`, `project_reorder_receipts`, `project_task_bulk_receipts` are idempotency caches.
- **Triggers.** Self-bumping revision triggers on tasks, custom fields, and options (B:470, B:477, B:483). `chat_channels_create_project_general` (B:489) and `project_working_folders_create_chat_environment` (B:552) create a conversation and channel with random ids, a membership, the primary folder row, and a `current-folder:<folder id>` environment on folder insert. `project_primary_working_folder_prevent_archive` (B:542) and the two `primary_working_folder_active_*` triggers (B:589, B:601) abort cross-row states. `protect_managed_delete` and `protect_managed_update` (B:570, B:577) protect managed folders. `project_working_folders_managed_relative_path_shape_insert` and `_update` (`20261021012432_stored_relative_path_shape.sql`) abort managed relative paths with an invalid shape.
- **Seeds.** `group-routine`, twelve `project-routine-*` projects, managed folders `working-folder-routine-*` (B:6555), sections, statuses, priorities, and views. `ensure_built_in_routine_defaults` (`crates/ganbaru-projects/src/routine.rs:294`) re-inserts and renormalizes them on every `load_workspace` (`workspace.rs:54`).
- **Commands** (`crates/ganbaru-projects/src`). New projects get `working-folder:{project_id}` (`project_commands.rs:69`). `update_project` (116-224) forces built-in name, group, and order and writes `music_context_assignments` in the same transaction (205-218). Status and priority deletion refuse the last or a used item (`mutations.rs:197-288`). Subtasks are one level deep (`mutations.rs:371`, `:392`). No command deletes a project, a section, or a task; projects go only through group deletion (`structure_commands.rs:112`, Routine refused). `update_task` relies on the revision trigger and resets `completed_at` from the status's terminal flag (`task_commands.rs:76-175`). Custom field and option reorders renumber siblings (`reorder.rs:207-248`). Value writes delete and reinsert (`custom_fields.rs:318-420`). Dependencies are deleted by random id (`relationship_commands.rs:68-81`). Event linking writes `calendar_events.project_id` (`relationship_commands.rs:205-228`). Client ids are random UUIDs (`apps/client/src/lib/stores/projects/actions.ts`); structure drafts send client `sortOrder` (`lib/projects/settings/structure-draft.svelte.ts:61`).
- **Working folders.** Chat owns external folder writes (`crates/ganbaru-chat/src/repository/workspaces.rs`, insert with `sort_order` 10, explicit revision bumps, delete of `kind = 'external'`); nothing updates `sort_order`. Removal, archive, and restore commands live at `apps/client/src-tauri/app/src/chat/workspace/commands.rs:575-624`. Device state lives in `crates/ganbaru-working-folders/src/lib.rs` (`app-state.json`). The managed directory is created on resolve (`managed_folder.rs:57-72`).
- **Chat FKs `NO ACTION` into Projects.** Twelve tables, fourteen FKs: B:4067, B:4286, B:4293, B:4374, B:4451, B:4512, B:5107, B:5173, B:5337, B:5362, B:5440, B:5482, B:5576, B:5625. `chat_threads`, `chat_attachments`, `chat_resources`, and `chat_assignment_authorization_revisions` are themselves FK parents of other Chat tables.
- **Other readers of `sort_order`.** Chat `reads.rs:82`, Notes `agent_bridge_export.rs:662-663`, app `chat/revocation.rs:640`, Calendar test fixtures.
- **Music.** Playlist deletion rewrites `projects.focus_playlist_id` and `break_playlist_id` (`crates/ganbaru-music-library/src/writes.rs:377-388`); milestone 5 step 1 removes that rewrite.
- **Icons.** `referenced_project_icons` (`apps/client/src-tauri/app/src/projects/icons.rs:687`) finds live icons by scanning every text column.
- **Sync.** `Engine::vault()` registers only Quick notes (`crates/ganbaru-sync/src/lib.rs:123-128`). Only Quick notes commands accept replica writes (`quick_notes.rs:48`, `vault/ownership.rs:223`); the frontend shows the ownership prompt when linked and not writable (`ownership-prompt.ts:37`).

### Re-verification on 2026-10-22

The Current state bullets above still hold apart from moved line numbers. The schema gaps are unchanged: integer `sort_order` and REAL task sort orders, no `deleted_at` on statuses or priorities, no `sync_key`, REAL `number_value`, the unique name indexes, and the revision and Chat-creating triggers (B:489-541, B:552-568). The engine as merged differs from this plan's assumptions in the following ways, decided here:

- **Nullable integers.** The engine had no nullable integer column type; milestone 5 step 4 adds `ColumnType::OptionalInteger`. Step 4 waits for that merge, which also brings the Music tables that the playlist References need.
- **Task tree position.** `TreeSpec` requires a `Position` group, but tasks order only inside the coupled section and status groups. The engine validates and summarizes `position_group` and never reads it in repair, so step 4 makes it `Option<GroupId>` in `manifest.rs` and the manifest summary (one scoped first commit with tests), and tasks declare none. No schema change; the `parent_task_id <> id` CHECK stays and is covered by the Tree's cycle repair.
- **Dependency keys.** The pending-key rekey only applies to tables keyed by `sync_key`, so a `pending:` dependency `id` could never become its final key. Dependencies get `sync_key TEXT NOT NULL UNIQUE` like the four composite-key tables, keyed by `derived_key` over `(task_id, depends_on_task_id)`. Migration 1 fills both `sync_key` and `id` with the placeholder, the conversion pass rewrites both to the same derived value, commands write `id` equal to `sync_key`, and `id` sits in the immutable identity group, so commands keep deleting by `id`.
- **Option value rows.** The generic owned table may carry only its parent key, its ordinal, and its declared references, so `project_custom_field_option_values` is rebuilt in migration 1 as `(value_key, ordinal, option_id)` with `option_id` a single-column reference into options. `task_id`, `field_id`, and the composite `(field_id, option_id)` FK are dropped; readers join through the value row, and an option row whose option belongs to another field is ignored on read.
- **Icon sizes.** An asset reference needs the exact file size. Migration 1 adds nullable `icon_byte_size` beside `project_groups.icon` and `projects.icon` and `asset_byte_size` beside `project_custom_emojis.asset_path`, each required exactly when the path names a `project-icons` file (the Music `artwork_override_byte_size` CHECK is the pattern). The icon writers fill them, and the step 1 conversion pass fills existing rows from file metadata, clearing a reference whose file is missing.
- **Priority creation time.** The derived-key identity group holds exactly the primary key, so `created_at` on priorities goes in its own Register group, like the tag link `created` group.
- **Multi-column CHECKs.** Every CHECK must read columns of one group (`WithinGroup`). Step 4 regroups columns to satisfy this where the plan's coupled groups do not already; a CHECK that cannot fit one group (the folder CHECK, the unique sets) is replaced by command validation, recorded in the step entry.
- **Primary folder key.** `project_primary_working_folders` is keyed by `project_id`, which is also its FK. Step 4 declares it as a plain-id table whose key is its project, hidden with the project; if conformance refuses a key that is an FK, step 4 may relax that check in conformance and manifest validation only (not `apply.rs`, `merge.rs`, `materialize.rs`, `seal.rs`, or `triggers.rs`), as one scoped commit with tests.
- **Chat attachment files.** The project deletion that removes Chat attachment rows arrives with step 3's `CASCADE` FKs, so step 3, not step 4, collects the project's attachment paths before the transaction and removes the files after commit.

## Design

### Manifest

`src/manifest/vault/projects.rs` and `src/domains/projects.rs`. References always point to a lower table id. Every table has `updated` as `Max` where it has `updated_at`, and `created` as Immutable (`created_at` and the owning parent column). Columns not listed are Immutable. On the derived-key tables (106, 107, 110, 113) the Immutable group holds only the key inputs, because a re-add seals every group again and must write the same value; their `created_at` goes in a `created` Register.

| Id | Table | Key | Groups |
| --- | --- | --- | --- |
| 96 | `project_groups` | `id` | `name`; `icon` (asset reference); `color`; `order` (Position); `lifecycle` coupled `{hidden_at, archived_at}`. `collapsed` is a Local column. |
| 97 | `projects` | `id` | `group` (Reference to 96, hiding parent); `name` (surfaced, recoverable); `icon`; `color`; `order` (Position, scope group); `status`; `event_defaults` coupled `{duration, name, time_mode}`; `pomodoro_defaults` coupled six columns; `idle_defaults` coupled three columns; `focus_playlist` and `break_playlist` (References to Music, missing resolves to none); `work_environment` and `blocker_ruleset` (Registers, missing resolves to none); `notes_settings` coupled two columns. |
| 98 | `project_working_folders` | `id` | `created` `{project_id, kind, managed_relative_path, sort_order}`; `display_name`; `repository` coupled `{kind, identity}`; `archived_at`. `revision` Local. |
| 99 | `project_primary_working_folders` | `project_id` | `folder` (Register, resolved by the read rule below). `revision` Local. |
| 100 | `project_sections` | `id` | `name`; `order`; `lifecycle` coupled `{hidden_at, archived_at}`. `collapsed` Local. |
| 101 | `project_statuses` | `id` | `name`; `category`; `terminal`; `color`; `order`; `deleted_at` (new). |
| 102 | `project_priorities` | `sync_key`, derived from `(project_id, id)`; plain row key, no existence group | `name`; `color`; `order`; `deleted_at` (new). |
| 103 | `project_tags` | `id` | `name`; `color`; `order`. |
| 104 | `project_tasks` | `id` | `parent` (Tree parent, max depth 1); `section_place` coupled `{section_id, section_order_key}`; `status_place` coupled `{status_id, completed_at, status_order_key}`; `title`, `description` (surfaced, recoverable); `priority` (Register); `kind` coupled `{task_type, milestone}`; `estimate`; `schedule` coupled seven columns (`start_date`, `start_time`, `start_reminder_minutes`, `due_date`, `due_time`, `due_reminder_minutes`, `target_end_date`); `archived_at`; `blocker_reason`. `revision` Local. |
| 105 | `project_checklist_items` | `id` | `title` (surfaced); `completed_at`; `order`. |
| 106 | `project_task_dependencies` | `id`, derived as `derived_key("dependency", [blocking, blocked, type])` | Immutable key inputs `{blocking_task_id, blocked_task_id, dependency_type}`; `created` Register `{created_at}`; `exists` (a Register with no domain column and its own group id); Acyclic edge set. |
| 107 | `project_task_tag_links` | `sync_key`, derived from `(task_id, tag_id)` | Immutable key inputs `{task_id, tag_id}`; `created` Register `{created_at}`; `exists`. |
| 108 | `project_custom_fields` | `id` | `name`; `icon`; `order`. `field_type` Immutable. `revision` Local. |
| 109 | `project_custom_field_options` | `id` | `name`; `order`. `revision` Local. |
| 110 | `project_custom_field_values` | `sync_key`, derived from `(task_id, field_id)` | Immutable key inputs `{task_id, field_id}`; `value` coupled `{text, number, date, checkbox}` (number in millionths, see [Custom field numbers](#custom-field-numbers)) plus the generic owned table `project_custom_field_option_values` (the option id set; `option_id` is a reference column into 109, and a missing option drops that owned row); `exists`. |
| 111 | `project_task_change_events` | `id` | Immutable, append only. |
| 112 | `project_views` | `id` (plain) | `name`; `order`. |
| 113 | `project_view_preferences` | `sync_key`, derived from `(view_id, preference_key)` | Immutable key inputs `{view_id, preference_key}`; `value`; `exists`. |
| 114 | `project_custom_emojis` | `id` | `name`; `asset_path` (asset reference); `order`. |

Local tables: the three receipt tables. Local columns (`collapsed`, `revision`) declare their materialization value for new rows (`collapsed` false, `revision` 1); `prepare_snapshot_copy` empties the receipt tables and resets both columns. Dependencies store their derived key as the `id` text, so commands keep addressing them by id (`relationship_commands.rs:68-81` deletes by id); the hash helper keeps them under the 128-byte bound. Views are not a derived-key table: user views carry random client ids for every type, and a derived-key table needs a derived key on every row, so views keep plain ids with ordinary tombstones, and default views keep `view-{project_id}-{type}` as plain keys created with their project. The four tables keyed only by a composite primary key (priorities, tag links, values, view preferences) follow milestone 2's composite row key convention: each gains `sync_key TEXT NOT NULL UNIQUE`, the row key, and keeps its composite primary key. Commands fill `sync_key` with `derived_key(prefix, parts)` from `ganbaru-sync-contracts` over the primary key columns. Tag links, values, and view preferences are derived-key tables with an `exists` group, and deleting a row writes existence 0. Priorities are not: their key parts are the project id and a fixed seed id (`defaults.rs:12-17`) or a random client id (`actions.ts:578`), deletion is soft, so a key is never removed and added again, and only the project cascade tombstones it. Migration 1 fills existing rows (and existing dependency ids) with the placeholder `'pending:' || rowid`; a Rust conversion pass in `ganbaru-projects`, run at vault open before capture seeding, rewrites every placeholder to the derived value, which is deterministic on every device because all replicas descend from one database, and a startup assertion refuses any remaining placeholder. The non-engine triggers that remain (`protect_managed_delete`, `protect_managed_update`, the two managed relative path shape triggers, and migration 1's empty order key reject triggers) only abort and are declared `ApplierVisible`; the self-revision triggers are dropped, so Projects declares no `SameRow` trigger.

### Merge rules specific to Projects

- **Coupled groups exist wherever a CHECK spans columns** (pomodoro defaults, schedule dates and times, repository kind and identity, lifecycle pairs) and wherever one column is derived from another in the same command (`completed_at` from the status's terminal flag). A move is one value: a task is in exactly one section place and one status place, and the latest move wins whole.
- **Subtask depth.** Local commands keep the one-level rule, and the manifest declares the Tree maximum depth 1. A merge that yields depth 2 (A sets T under U while B sets U under V) is repaired by milestone 2's depth rule: the deepest row beyond the maximum moves to the root, ties among equally deep rows going to the greatest `(clock, writer)` of the winning parent version, and a sealing replica writes the repair. No UI change.
- **Dependency cycles.** Dependencies declare milestone 2's Acyclic edge set: in each cycle, the edge whose creating version has the greatest `(clock, writer)` is hidden and surfaced as a conflict the user can resolve. Projects presents it as "dependency removed to break a cycle", with keeping it removed always available and restoring it disabled while the cycle would return.
- **Statuses and priorities are never tombstoned except by project cascade.** Deletion sets `deleted_at`. A deleted status or priority is hidden from settings and pickers but still shown where a live task uses it; when every status of a project is deleted concurrently, the first by order key is shown. Together with the `CASCADE` rebuild of `project_tasks.section_id` and `status_id` (migration 3), this removes the `NO ACTION` failure when one device moves a task into a status another deletes; the "used item" refusal stays an explicit command query. Sections already work this way (no delete exists).
- **Dependency cascade.** A cascade date shift writes each shifted task's `schedule` group as an ordinary coupled write; its receipts stay Local.
- **Group deletion on replicas.** The applied delete cascades into Unconverted children (Notes project folders and history, Chat conversations, channels, and the rebuilt FKs, `calendar_events.project_id` set null) through milestone 2's guard skip while applying; the next refresh replaces those rows anyway.
- **Priority** is a Register; a value naming no live or deleted priority displays as the project's first priority by order. **Archived parents.** A subtask of an archived task reads as archived; no repair write.
- **Duplicate names are allowed.** The case-insensitive unique indexes on tags, custom fields, and options are dropped; commands still refuse a duplicate locally. Two devices creating "Bug" at once keep two tags. Duplicate repair with redirects (milestone 1 tags) would have to re-key links and values, and two fields with the same name can differ in type, so they cannot be merged. `ux_project_views_single_hosted` is dropped for the same reason: `create_view` still refuses a second notes or chat view locally, and two devices adding one concurrently keep two views that open the same pages and channels.
- **Revision.** The three self-bump triggers are dropped. Commands bump `revision` explicitly, and the applier bumps it when a content group materializes, never for order keys or `updated_at`, as in milestone 1.
- **Recovery.** Tombstones arise only from group deletion (projects and their subtree) and from tag, field, option, view, emoji, and checklist item deletion; dependency, tag link, value, and view preference deletion writes their `exists` group to 0 instead. A dependency hidden by cycle repair is a conflict, not a tombstone. Recoverable groups are project name and task title and description. Restoring a project restores it with its retained subtree into its group if live, otherwise into the first group by order.

### Custom field numbers

- Replicated columns never use REAL, so `number_value` becomes `INTEGER` in millionths (six decimal places). The API carries the value as a decimal string in both directions, so no value passes through `f64`: commands parse it in Rust with integer arithmetic and refuse more than six decimals or a scaled value outside `i64` (`validation.rs:469`), rows bind `i64` (`models.rs:422`, `:1087`, `custom_fields.rs:291-310`, `:359`), and readers format the canonical decimal string.
- `task_views.rs:382` sorts the integer unchanged, since integer order equals decimal order. `task_views.rs:664-667` sums with `TOTAL` (SQLite `SUM` raises on integer overflow) and divides sum, average, minimum, and maximum by 1,000,000 as read-only REAL results.
- The frontend types `numberValue` as a decimal string (`api/projects.ts:251`, `:676`, `projects/types.ts:403`, `:586`), validates drafts with the same rule instead of `Number` (`projects/tasks/detail.ts:320-334`), formats through the locale number helpers (`projects/list/presentation.ts:148`, `ProjectListColumnCell.svelte`), and compares scaled `bigint` values (`projects/tasks/view.ts:359-360`, `projects/custom-fields.ts:67`).

### Working folders

- The folder id is a plain key. New managed folders keep `working-folder:{project_id}`, created in the same transaction as their project, and `protect_managed_delete` prevents removal while the project exists, so the key is never removed and re-added and needs no existence register. Seed folders keep `working-folder-routine-*`; seeds are identical on every replica, so renaming them buys nothing. Lookups use `kind = 'managed'`.
- `sort_order` stays an Immutable column on this table only; nothing updates it and Chat contracts expose it, so converting it buys nothing.
- The primary folder table is rebuilt without the composite FK, the three cross-row triggers (B:542, B:589, B:601) are dropped, and `create_project` inserts the primary row. Commands keep the rules (a primary must belong to the project and be active; the primary cannot be archived). Reads resolve the effective primary as the stored folder if it exists, belongs to the project, and is not archived, otherwise the managed folder. Concurrent "set primary F" and "archive F" therefore converge without an apply failure.
- Received folders never touch the filesystem during apply. The managed directory is created by the existing resolve path on first use; an external folder on a device with no binding shows the existing "locate" state.

### Chat defaults and FKs

- `chat_channels_create_project_general` and `project_working_folders_create_chat_environment` are dropped. `ensure_project_chat_defaults(project_id)` in `ganbaru-chat` creates, idempotently, the default conversation and channel with ids from `derived_key("conversation-default", [project_id])` and `derived_key("channel-default", [project_id])` in `ganbaru-sync-contracts` (only when the project has no default channel, so existing random ids stay), the owner membership by its natural composite key (memberships have no id column; Chat tables stay Unconverted here, so the row has no `sync_key` until milestone 9's conversion pass fills it with the same helper), and `current-folder:{folder_id}` for each folder. Chat organization load and the two places that build the environment id (`chat/organization/context.rs:892`, `send/coordinator.rs:435`) call it first. It runs only where Chat is writable (the owner until milestone 9); a replica shows the project's Chat view behind the ownership prompt until refresh brings the owner's rows.
- The fourteen FKs become `CASCADE`, or `SET NULL` when every FK column is nullable and no CHECK requires a value. Composite FKs with a `NOT NULL` column cascade. Removing an external folder keeps refusing while Chat references it, through an explicit in-use query, since the FK no longer refuses. `project_tasks.section_id` and `status_id` become `CASCADE` in the same rebuild (both are `NOT NULL`).

### Migrations

Three migrations, so each lands with its own step.

1. `{ts}_projects_order_keys.sql` (normal transaction). Every Projects table with `CASCADE` children is converted additively, never rebuilt: drop the sort index, add `order_key` (`section_order_key` and `status_order_key` on tasks) filled with rank keys by `(sort_order, name, id)` per scope, drop the old column, recreate indexes, add the empty-key reject trigger. Add `deleted_at` to statuses and priorities. Add `sync_key` to priorities, tag links, and view preferences (a column with a non-null default, filled with the `'pending:' || rowid` placeholder, then a unique index); the values rebuild below includes it. Drop the three unique name indexes and the three revision triggers. Rewrite dependency ids to the placeholder for the conversion pass and drop `ux_project_views_single_hosted`. Rebuild `project_custom_field_values` with an "at most one scalar" CHECK, `number_value INTEGER` copied as `CAST(ROUND(number_value * 1000000) AS INTEGER)` behind a guard that aborts when a scaled value leaves the `i64` range, `idx_project_custom_field_values_number` recreated, and a parent row for every select value. Rebuild `project_custom_field_option_values` as a generic owned table with primary key `(value_key, ordinal INTEGER NOT NULL)`, where `value_key` references the value's `sync_key` `ON UPDATE CASCADE ON DELETE CASCADE` (so the conversion pass carries it), and `task_id`, `field_id`, and `option_id` stay as data columns carried in the row value, so readers are unchanged and `(field_id, option_id)` keeps its composite FK to options; neither table is a parent of other tables. Clear the receipt tables. The rank guard aborts above 238,328 rows per scope.
2. `{ts}_projects_folder_triggers.sql` (normal transaction). Drop the two Chat-creating triggers and the three primary folder triggers; rebuild `project_primary_working_folders` without the composite FK.
3. `{ts}_project_fk_actions.sql` (`-- no-transaction`). `PRAGMA foreign_keys = OFF`, then one `BEGIN`, the standard rebuild (create, copy, drop, rename) of the twelve Chat tables and of `project_tasks`, recreation of every index and trigger on them (including `chat_threads_project_matches_working_folder_*` and the path-shape triggers of `20261021012432_stored_relative_path_shape.sql`), a guard that inserts the row count of `pragma_foreign_key_check` into a temporary table whose CHECK requires zero, so any violation aborts before `COMMIT`, and `PRAGMA foreign_keys = ON`. With foreign keys off, dropping a parent table deletes none of its children, and the renamed table keeps the name the children's FK text binds to. One transaction means a crash leaves the old schema and the rerun starts clean. The in-transaction save and restore of milestone 1 is not practical here: `chat_threads` alone has 22 child references.

The manifest migration then installs capture, key immutability, and re-create guard triggers and seeds no captures; milestone 2's `seed_captures` publishes every existing row at the next vault open, after the conversion pass, in batches of 256 operations.

### Commands and write paths

- Built-in Routine repair on load is removed. Seeds descend from one database, no command deletes a project, and commands already refuse renaming, moving, or reordering built-ins (`update_project` 121-128, `structure_commands.rs:50`, `:109`). An insert by key on load would also trip the re-create guard for a published key.
- Structure and reorder commands take a placement (before or after a sibling id, or end) and compute order keys in Rust; the API exposes `orderKey` instead of `sortOrder`. Custom field and option reorders move one row.
- Value writes become upserts; clearing deletes the row, which seals existence 0, so a select value row exists only with at least one option. Dependency and tag link writes use derived keys; deletion succeeds when the row is already gone.
- Projects tables leave the guarded set, and the Projects domain joins the per-domain replica write flag. Commands whose writes cascade into or write guarded Unconverted tables return a typed `owner_required` error on replicas, and the frontend shows the ownership prompt for them: group deletion and external folder removal (Chat and Notes cascades) until milestone 10, and the commands that write `calendar_events` or `project_task_event_links` until milestone 7 lifts them.
- `update_project` keeps its `music_context_assignments` write in the same transaction; it is one operation group across two domains.
- Readers outside the crate move to `order_key`: Chat `reads.rs:82` keeps `sort_order` (unchanged on folders), Notes `agent_bridge_export.rs:662-663` orders by `order_key`, `chat/revocation.rs:640` is checked, Calendar fixtures drop `sort_order`.

### Assets and post-apply hooks

- Projects declares `AssetClass { name: "project-icons", max_bytes: 3 MiB }`. Icon files are already `{sha256}.{ext}` (`icons.rs:199`), so the engine's path function matches them. `asset_refs` parses references from `project_groups.icon`, `projects.icon` (`asset:project-icons/...`), and `project_custom_emojis.asset_path` (`project-icons/...`).
- Retained versions keep their icons because milestone 10's sweep counts every stored version. Before milestone 10 the only Projects icon deletion is `projects_icon_delete_assets_if_unreferenced` (`icons.rs:746`), which removes only discarded uploads this process created and no text column mentions; the plan relies on that and does not change it.
- The post-apply hook receives the connection and milestone 2's change sink, not an `AppHandle`. It reports the changed Projects tables and keys to the sink and rebuilds nothing else; the host maps them to the existing Projects change event so open windows refresh and the task reminder scheduler reloads.

### Conflicts and recovery presentation

- Task title and description conflicts use the milestone 2 presentation inside the task detail view; project name conflicts show in project settings. Checklist titles are surfaced but resolved inline by picking a value.
- Recovery entries for projects appear in the shared recovery list, and dependency edges hidden by cycle repair appear as conflicts, both with Projects previews.
- Strings go in `apps/client/src/lib/i18n/messages/{en,es}/projects/sync.ts`.

## Testing

- `ganbaru-db`: each migration on a seeded vault and on the dense dataset: row counts, rank order equals old order, select values keep their options, numbers equal `ROUND(value * 1000000)` and no REAL column remains on tables 96 to 114, `sync_key` placeholders unique, `foreign_key_check` empty, `project_tasks.section_id` and `status_id` cascade, every Chat and `project_tasks` index and trigger present after the rebuild (compared by name against the pre-migration schema), and the rebuild rolled back intact when the guard fires.
- `ganbaru-projects`: the conversion pass rewrites every placeholder (dependency ids and `sync_key`) to the `derived_key` value and is idempotent, the startup assertion refuses a remaining placeholder, commands write the same key two devices would derive, placement to order key, status soft delete visibility, used status and priority refusal by query, priority fallback, explicit revision bumps, value upsert and clear, number parsing (six decimals, `i64` bounds, canonical formatting) and aggregates (`tests/custom_fields.rs`, `tests/task_views.rs`), duplicate name and single-hosted view refusal only locally, built-in protection without load repair.
- `ganbaru-chat`: `ensure_project_chat_defaults` idempotence, existing default channel reuse, in-use refusal for external folder removal.
- `ganbaru-sync`: conformance for tables 96 to 114 (including `sync_key` on the four composite-key tables), codec round trips, simulation seeds covering concurrent moves, depth repair, cycle repair, status delete against move, group delete against task edit, select value against option delete, value clear against edit, concurrent notes view adds keeping two, and primary against archive.
- Frontend: placement mapping, number draft validation, formatting, and comparison, effective primary display, the `owner_required` prompt path, conflict and recovery previews.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-projects-order-keys` | Migration 1, order keys in commands and API, soft deletes, explicit revision bumps, value upserts, `sync_key` and derived dependency ids written through `derived_key`, the conversion pass and startup assertion, fixed-point custom field numbers in commands, readers, and the frontend, Routine repair removal with the `first_use_contracts.rs` and `tests/routine.rs` updates, cross-crate readers, frontend placement. | `cargo test -p ganbaru-db --lib -j 4 projects -- --test-threads=4`, `cargo test -p ganbaru-projects --lib -j 4 order_key -- --test-threads=4`, `cargo test -p ganbaru-projects --lib -j 4 sync_key -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/projects/settings src/lib/projects/tasks src/lib/projects/list --maxWorkers=1` |
| 2 | `feat/sync-projects-folders` | Migration 2, primary folder read rule and command checks, `ensure_project_chat_defaults` and its callers. | `cargo test -p ganbaru-chat --lib -j 4 default -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 working_folder -- --test-threads=4` |
| 3 | `feat/sync-projects-fk-actions` | Migration 3, the external folder in-use check, and the used status and priority queries. | `cargo test -p ganbaru-db --lib -j 4 chat -- --test-threads=4`, `cargo test -p ganbaru-db --lib -j 4 projects -- --test-threads=4` |
| 4 | `feat/sync-projects-manifest` | Manifest and adapter registration (the next vault open seeds them through `seed_captures`), conformance, simulation, post-apply hook, `VAULT_MANIFEST_VERSION` bump. | `cargo test -p ganbaru-sync --lib -j 4 projects -- --test-threads=4` |
| 5 | `feat/sync-projects-writes` | Replica write flag for Projects, `owner_required` gates, `project-icons` asset class and `asset_refs`. | `cargo test -p ganbaru-tauri-app --lib -j 4 projects -- --test-threads=4` |
| 6 | `feat/sync-projects-conflicts` | Conflict and recovery presentation, i18n. | `pnpm --dir apps/client exec vitest run src/lib/projects --maxWorkers=1`, `pnpm --dir apps/client run check` |
| 7 | `feat/sync-projects-docs` | Docs, acceptance results, goal README status. | `pnpm -w run validate` |

Steps 1 to 3 need only milestone 2 step 4 and can land before the rest of milestone 2: step 1 uses the `derived_key` helper in `ganbaru-sync-contracts` for `sync_key` (which is `NOT NULL` from migration 1 onward, so commands fill it at once) and dependency ids, and step 2 uses it for the default Chat ids.

## Documentation

- `docs/data/schema/notes-and-projects.md`: Projects classification, order keys, soft-deleted statuses and priorities, explicit revision bumps (replacing the trigger text in Native project mutations), value rows, fixed-point custom field numbers, derived keys and `sync_key`, task section and status FKs as `CASCADE`, primary folder read rule, and removal of trigger-created Chat rows from Working folders.
- `docs/data/schema/chat.md`: lazy default channel and environment creation, and the FK actions into Projects.
- `docs/data/schema/sync.md`: "Currently replicated" adds Projects and working folder bindings.
- `docs/data/sync.md`: the delivery row for other domains and storage ownership for Projects and folder bindings.
- `docs/algorithms/sync/README.md`: a Projects worked example of milestone 2's depth repair and Acyclic edge sets, and soft deletion instead of tombstones for referenced settings rows.
- `docs/features/projects/README.md`: the Routine group no longer restores on load; editing on replicas.
- `docs/features/projects/tasks-and-views.md` and `settings-and-scheduling.md`: duplicate names and notes or chat views after sync, number fields limited to six decimals, deleted statuses still shown while used, working folder primary fallback.
- `docs/features/README.md` Projects row and the `AGENTS.md` sync sentence ("Concurrent sync covers Quick notes so far").

## Manual acceptance

Two desktops (A owner and coordinator, B linked) and one Android phone (P). Offline edits are made with the network disabled, then reconnected.

| Scenario | Expected |
| --- | --- |
| P creates a project offline | It appears on A and B with default sections, statuses, priorities, and views; its managed folder appears on A only when opened; A's Chat view gets the default channel. |
| A and B edit one task title offline | Conflict shown on all three; resolution converges. |
| A and B reorder tasks in one section | Same order everywhere. |
| A adds T blocks U, B adds U blocks T | One edge remains everywhere and a conflict names the hidden one. |
| A puts T under U, B puts U under V | Depth 1 everywhere, same result. |
| A deletes status S, B moves a task into S | The task stays in S, S stays visible while used. |
| A and B set one single-select field to different options | One value everywhere; no orphan option rows. |
| B sets a number field to 12.345678, P enters 0.1234567 | 12.345678 shows the same on A and P; P's entry is refused. |
| B adds an external folder | A sees the binding as "locate", sets its own path; B's path unaffected. |
| A deletes a group while B edits a task inside it | Group gone everywhere; a recovery entry restores the project with B's edit. |
| B sets a custom emoji project icon | Icon bytes reach A and P. |
| A deletes playlist L, B assigns L as focus playlist | Project shows no focus playlist everywhere. |
| B tries to delete a group | Ownership prompt, no partial write. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Migrations 1 to 3 on the dense dataset | Under 5 s desktop, under 15 s Android |
| First seal of the dense dataset's Projects rows | Under 10 s desktop |
| Apply of a 256-operation Projects batch | Under 50 ms desktop, under 150 ms Android |

## Risks

| Risk | Mitigation |
| --- | --- |
| The Chat and `project_tasks` rebuild loses an index, trigger, or row | Schema-by-name comparison and row count tests; one transaction with an FK check guard. |
| A placeholder `sync_key` or dependency id reaches capture | The conversion pass runs at vault open before capture seeding, and the startup assertion refuses any remaining placeholder. |
| Readers assume the default channel or environment exists | Every builder of those ids calls `ensure_project_chat_defaults`; tests cover a project created without triggers. |
| Users dislike duplicate tags after a merge | Rare for one person; rename and delete fix it; documented. |
| Owner-only gates feel arbitrary on replicas | Only group deletion, external folder removal, and Calendar-writing commands; the prompt explains it; milestones 7 and 10 lift them. |
| Order key rank capacity per scope | Migration guard; real projects are far below it. |

## Deferred

- Calendar-writing Projects commands on replicas ([milestone 7](milestone_7.md)), Notes project rows ([milestone 8](milestone_8.md)), Chat rows and the Chat side of project defaults ([milestone 9](milestone_9.md)), owner removal ([milestone 10](milestone_10.md)).
- Restoring new built-in Routine content into existing vaults; a future change adds it as a migration.
- Folder content transfer (README decision for milestone 10).

## Implementation checks

Settled from source:

- `-- no-transaction` is supported by the pinned SQLx (`sqlx-sqlite` 0.9.0 `migrate.rs:165`), and `ganbaru-db` sets `foreign_keys(true)` only in connect options (`crates/ganbaru-db/src/lib.rs:172`), so the explicit `PRAGMA foreign_keys = OFF` in migration 3 holds for the whole file on its single connection.
- The only `current-folder:` builders are `chat/send/coordinator.rs:435` and `chat/organization/context.rs:892`; both feed `verify_folder_environment`, so step 2 calls `ensure_project_chat_defaults` before each. Default channels are created only by the dropped trigger today, so `ensure_project_chat_defaults` is their only builder.
- Folder ids allow 1,024 bytes (`crates/ganbaru-working-folders/src/lib.rs:11`). Step 2 lowers `MAX_IDENTIFIER_BYTES` to 64, and the manifest migration aborts with a clear message on a longer existing id.
- No Projects frontend code reads a replica or read-only state (a search for `isReplica`, `isReadOnly`, and `vaultReadOnly` finds only `stores/calendars.svelte.ts`), so step 3 adds the per-domain flag check in the Projects store, not per component.

Decided at step 3, where the `CASCADE` FKs make project deletion remove Chat rows: Chat attachment rows have no file sweep today (the only sweep is Chat preview data, `chat/preview/data_store.rs:131`), so the delete collects the project's attachment paths before the transaction and removes the files after commit.

## Progress

- [x] Step 1: order keys, soft deletes, derived ids and `sync_key`, conversion pass, fixed-point numbers, revision bumps, Routine repair removal
  - 2026-10-22, branch `feat/sync-projects-order-keys` (based on feat/sync 1652c89f; feat/sync is now fb3d1d07, not rebased per brief)
  - Commits: 6de11608 feat(db): add projects order keys, sync keys, and fixed-point values; 4def9891 feat(projects): add order keys, sync keys, and conversion pass; 02f79e7f feat(projects): use order keys and placements in the client; dbbb627b docs(projects): describe order keys, soft deletes, and exact numbers
  - Checks: cargo tests pass for ganbaru-projects, ganbaru-db, ganbaru-calendar, ganbaru-notes, ganbaru-music-library, ganbaru-sync, ganbaru-sync-replica, ganbaru-tauri-app (1002); fmt and clippy -D warnings clean on touched crates; `pnpm --dir apps/client run check` 0 errors; projects vitest scope passes
  - Conversion runs in db.rs on the first read-write registry open of a vault path (covers vault and benchmark databases), then asserts; sync `Session::start` refuses placeholder keys; guarded replicas never convert (owner converts before handoff)
  - An icon whose asset is missing or over the size limit converts to `MISSING_ICON_FALLBACK` ("none")
  - Local deletes of in-use statuses and priorities stay rejected; soft-deleted rows matter only after concurrent edits
  - Music benchmark seed group uses order key "czzz"; first-use Projects contract is now 6 reads, 0 writes
  - Next step: headless sync test fixtures call the conversion explicitly after migrating; new fixtures that migrate a vault directly need the same
- [x] Step 2: folder triggers, primary read rule, lazy Chat defaults
  - 2026-10-23 UTC, branch `feat/sync-projects-folders` from `feat/sync` (01bebf83), commits 18ce535b (code), ef847903 (docs), and 6c344257 (fix), all signed.
  - Migration `20261023045510_projects_folder_triggers.sql` drops the five working-folder and project triggers that created Chat rows and rebuilds `project_primary_working_folders` with `projects` as its only FK parent, so a stored primary may name a folder that has not arrived and archiving it is allowed at schema level; existing primary rows are copied unchanged. Test covers triggers gone, rows kept, no Chat rows on folder insert, dangling primary allowed, cascade on project delete.
  - Implementation check held: inserting projects and folders creates no Chat rows; `create_project` writes the primary row itself.
  - `ensure_project_chat_defaults`, `ensure_working_folder_chat_defaults`, and `ensure_all_project_chat_defaults` live in `ganbaru_chat::project_defaults`, not under `repository`, because `repository` is gated on `native-runtime` and mobile lists channels too. Default conversation and channel ids are `derived_key("conversation-default" | "channel-default", [project id])`; environments are `current-folder:{folder id}`, skipped when an archived row with that id or an active current-folder environment exists.
  - Callers: channel list (project), navigation channel list (all projects), execution environment list, assignment targets (channel project), send target resolution, and context resolution inside its existing Chat write transaction when the selected environment is the folder's current-folder one. List callers go through `ensure_chat_defaults`, which skips silently when `active_status(app).can_write` is false and treats a missing project or folder as nothing to create. Adding an external folder creates nothing; its environment appears on the next listing or send.
  - Primary read rule: stored folder if it exists, belongs to the project, and is not archived, else the managed folder; no stored row reads revision 0, and setting with expected revision 0 inserts. Setting refuses a missing, foreign, or archived folder. The stored primary cannot be archived or removed (Conflict) until another folder is promoted; the frontend draft already promotes first, so no frontend change.
  - Folder id limit lowered to 64 bytes (sync row keys); `create_project` refuses a project id whose managed folder id exceeds it, before any row or directory is created.
  - Pool closure audit: no gap. Backup restore, rollback restore, Android restore, and handoff replacement all close the registry pools under the exclusive vault gate (reserved quiescence) and reopen lazily through `connect_with_access`, which runs Projects conversion on the first ReadWrite open. Noted, not fixed: `reset_database` closes pools without `begin_vault_exclusive`, but the app exits right after, so the next launch converts. Handoff archive creation opens the registry without conversion; the receiver converts on first open. Guarded and ReadOnly opens skip conversion by design.
  - Follow-up commit 6c344257 makes the default channel insert conditional like the conversation and membership inserts. Transactions are deferred, so two windows ensuring the same fresh project at once can still make one listing fail with a transient busy error; the next listing succeeds. Accepted.
  - Mobile manages `VaultOwnershipManager` in `runtime/mobile.rs`, so the `active_status` gate in the cross-platform channel listings cannot panic there.
  - Checks: ganbaru-chat, ganbaru-projects, ganbaru-working-folders lib tests; ganbaru-db lib (119); ganbaru-tauri-app lib `chat` (339) and `working_folder` (10); clippy `--all-targets -D warnings` on the five touched crates plus ganbaru-chat `--no-default-features`; `cargo fmt --check`. All pass. No Android target build was run.
- [x] Step 3: FK actions for Chat and task sections and statuses
  - 2026-10-23, branch `feat/sync-projects-fk-actions`, commits c64451f5 (migration and schema tests), 74fbf219 (external folder in-use check, priority refusal tests), 3fe5d42e (Chat file sweep on group deletion), 83c168f2 (docs), merge cb9eaece of local `feat/sync`
  - Implementation check held on the CASCADE branch: every changed FK is NOT NULL or composite, so all fourteen Chat FKs plus task `section_id` and `status_id` became `ON UPDATE CASCADE ON DELETE CASCADE`; authorization revision `working_folder_id` cascades rather than SET NULL so a revision never outlives the folder its scope digest names
  - Migration `20261023071334_project_fk_actions.sql` uses `-- no-transaction`, foreign keys off and `legacy_alter_table` on around the rebuild (the rename otherwise reparses other tables' triggers), and a vault-wide `foreign_key_check` guard that aborts and rolls back on any violation
  - Plan-silent: scratch promotion `destination_channel_id` and `scratch_generation_id`, and scratch cleanup job scope and generation FKs, also cascade, since they otherwise block project deletion
  - Plan-silent: five delete-denied audit triggers now deny only while their parents exist, so the cascade can remove them; direct deletes stay refused
  - Plan-silent: the in-use query ignores drafts and execution environments, which already cascaded with the folder; the check and delete share one transaction
  - Plan-silent: the file sweep runs in the `projects_delete_group` command, removes only paths no remaining attachment or resource row names, and logs failures without failing the committed delete; remote project deletions applied by sync have no file sweep yet (milestone 10 collection covers them)
  - Residual risk: cross-project Chat references (for example channel reference targets naming another project's channel) still use NO ACTION and could block a project delete; deferred to milestone 9
  - The Implementation-checks bullet about a Projects store flag belongs to step 5 and was not done here
  - Landing fix 2ce16c8a: the tauri-app test that expected a direct project delete to be refused now relies on the cascade
- [x] Step 4: manifest, adapter, conformance, simulation
  - Landed on `feat/sync-projects-manifest` (worktree agent-af6c0b89ad8fa1dc7). The Projects domain covers tables 96 to 114 and has `VAULT_MANIFEST_VERSION` 7, `replica_writable: false`, and no asset classes. The adapter does codec and value validation and presentation. Migration `20261023094450_projects_replication.sql` does three things: it rebuilds dependencies with `CHECK (sync_key = id)`, rebuilds working folders with gated managed triggers, and adds the seeded priority keys and the rendered sync triggers. Other pieces:
    - Conformance runs over every Projects table, including owned option values.
    - `ProjectsHook` reports each changed row as (table name, key).
    - The host maps those reports to the existing `projects-changed` event, which carries a `sync` key. On that key, open windows drop caches for projects they do not show and reload task reminders.
    - Refusing restorers are registered for the recovery roots: groups, projects, sections, statuses, and tasks.
    - Eleven simulation tests cover every plan seed, project deletion against a concurrent task insert, and hook notices.
    - Docs: `docs/data/sync.md` and `docs/data/schema/notes-and-projects.md` each got a few sentences.
  - Commits:
    - 7d95f7fa: tree without a position group, implied self-parent check.
    - e63badfa: conformance accepts parent-keyed cascades and unique sets that hold the key.
    - dfdde213: manifest, adapter, and migration.
    - cba00cf6: post-apply hook and host event.
    - 17ed4d0d: simulation.
    - 8a27bf60 and 74686ed4: docs.
    - 757a9d27: signed merge of `feat/sync`.
    - 9d5c143f: clippy fix.
    - All commits are signed.
  - Re-verification items:
    - Task `position_group` became `Option<GroupId>` (7d95f7fa).
    - Cross-column CHECKs became coupled groups plus adapter validation (dfdde213).
    - The primary folder table is keyed by its project (dfdde213).
  - Implementation checks:
    - Folder ids at 64 bytes held. Managed ids `working-folder:<project>` stay within the row key bound.
    - Self-revision triggers were dropped, so Projects declares no `SameRow` trigger.
    - The `ApplierVisible` abort-only triggers held, with protect-managed gated by `applying`.
    - Engine files (`apply.rs`, `merge.rs`, `materialize.rs`, `seal.rs`, `triggers.rs`) were not edited. The manifest-shape changes landed in `manifest.rs` and the conformance tests.
  - Plan-silent decisions:
    - Priorities are keyed by `sync_key` with no derived key.
    - The dependency key has two inputs (blocking, blocked), and `UNIQUE(blocking, blocked)` replaces the type-including index.
    - Seeded priority keys are computed in the migration.
    - The folder CHECK split into gated insert and update triggers. The partial managed index was dropped because the key implies uniqueness.
    - The primary folder is a plain register.
    - Playlist, environment, and ruleset references are opaque registers.
    - `MAX_SELECTED_OPTIONS` is 1000, and the adapter refuses null option ids.
    - Recovery roots include sections and statuses.
    - Under the engine's existence rule, a cleared value and a concurrent edit settle on the newest existence write (an edit reseals existence), not on a guaranteed clear.
    - Test fixtures do three things: they toggle `applying` for immutable test moves, drop the value trigger for the byte-limit cascade test, and use valid order keys.
  - Deferrals:
    - Step 5: the replica write flag, `owner_required` gates, and the `project-icons` asset class (the manifest only needs the flag flipped and the class added).
    - Step 6: Projects restore from recovery offers (restorers currently refuse, discard works) and conflict presentation.
- [x] Step 5: replica writes, owner-only gates, icon assets
  - Status: done on `feat/sync-projects-writes` (HEAD 2edfec59, merged with feat/sync 14e92d06 at 69212e19), not merged into feat/sync. 2edfec59 `docs(security): list project icons among replica-writable asset directories` is a docs-only follow-up after the checks.
  - Landed: Projects `replica_writable: true`, the `project-icons` asset class (3 MiB, png/jpeg/webp) with `asset_refs` over group icons, project icons, and emoji assets, strict `project-icons/{sha256}.{ext}` icon paths, a synced content check for project icons, replicated asset paths for icon writes, and owner gates.
  - Commits: 2e0a68a6 `feat(sync): admit projects replica writes with the project icon asset class`, 037f9fc8 `feat(projects): refuse owner-only commands on replicas`, 4c6380fd `feat(projects): restore recovered projects rows`, 7f78ba14 `feat(projects): show the ownership prompt for owner-only and read-only writes`, ca16166e `docs(projects): describe replica writes and recovery restore`, 69212e19 signed merge of feat/sync 14e92d06 (clean, no compile fixes needed in this branch's code).
  - Recovery restore: groups, projects, sections, statuses, and tasks restore as new rows with new ids for every descendant. A project whose group is gone moves to the first group; a section, status, or task whose project is gone is refused and stays discardable; a task whose section or status is gone falls back to the first one of its project, and a subtask whose parent is gone becomes top-level. Managed working folders are recreated rather than copied; external folders come back with new ids and must be rebound on each device. Descendants that can no longer be inserted (dependency to a gone task, value of a gone field) are left out on purpose. Ten exchange tests in ganbaru-sync-replica cover each root with live and deleted parents.
  - Gated commands (typed `owner_required:` error): `projects_delete_group` (Chat and Notes cascades, Calendar event SET NULL), `projects_remove_working_folder` (Chat cascades, wrapped as Chat `Permission`), `projects_link_task_event` and `projects_unlink_task_event` (Calendar tables). `projects_icon_delete_assets_if_unreferenced` is unchanged and stays owner-only.
  - Frontend: `lib/vault/owner-required.ts` recognizes the `owner_required:` prefix (string, Error, or Chat error object) and requests the ownership prompt; Projects API mutations and folder removal wrap with it. The Projects store checks `canWriteSyncDomain(status, "projects")` before each mutation and opens the prompt instead of writing. `VaultOwnershipPrompt` listens for requests and shows an owner-required description (en and es).
  - Docs: docs/data/sync.md (Projects mixed-mode bullet, asset classes, status row) and docs/data/schema/notes-and-projects.md.
  - Final checks after the merge, all passing: ganbaru-projects lib 92, ganbaru-sync `projects` 22, ganbaru-sync-replica `projects` 10, ganbaru-tauri-app `projects` 35 and `sync` 44, clippy `--all-targets -D warnings` on ganbaru-projects, ganbaru-sync, ganbaru-sync-replica, ganbaru-tauri-app, `cargo fmt --check`, client check 0 errors, vitest (projects, stores, vault, two API tests) 1050.
  - Plan-silent decisions: every non-owner, including unlinked read-only devices, gets the same `owner_required` error; `VAULT_MANIFEST_VERSION` stays 7; the store treats a failed ownership status read as writable (logged) and lets the native guard decide.
  - Known caveat: restore fallbacks (first group, first section) can pick a migration-seeded row the engine never captured only in unseeded test vaults; real vaults seed those rows through `seed_captures`.
- [x] Step 6: conflict and recovery presentation
  - Landed: Projects-specific conflict commands (`projects_conflicts`, `projects_resolve_conflict`) because no shared conflict path exists; task title and description, project name, and checklist title conflicts as a shared card with keep or use choices; cycle-hidden dependencies labelled as removed to break a cycle (keep removed always, add back disabled while it would recreate the cycle); Projects recovery titles and previews for groups, projects, sections, statuses, and tasks; en and es catalog `projects/sync.ts`; docs note in the Projects README and sync status row.
  - Commits: 7fbd113a feat(projects): add sync conflict commands and recovery titles; 26f55495 feat(projects): present sync conflicts and recovery; fix(projects): refresh conflicts after dependency edits (see git log)
  - Decisions: Keep removed on a hidden edge is INSERT then DELETE of the dependency row (sealed as an existence write of 0), since `force_group` needs a live row; the hidden edge's tasks come from `retained_group_values`. No engine edits. Adapter recovery previews show titles and a task description preview only, because the pure presentation function has no parent context.
  - Deferrals: parent names (project of a restored section, status, or task) in recovery previews; no component tests for the conflict cards (no Svelte component test harness used for these).
  - Checks: vitest focused (projects, sync, i18n), client check, editor-check, Rust tests and clippy for ganbaru-sync, ganbaru-projects, ganbaru-tauri-app.
- [ ] Step 7: docs and physical acceptance
  - Docs done: 45977229 (features summary, replica editing and owner-only commands), earlier step docs, review fixes 67dbee24; the Documentation list is covered. 8043d98f lazy-loads the dependency conflict panel and raises the Android bundle ceilings by the 11 Projects sync UI modules. `pnpm -w run validate` passed on 8043d98f.
  - Budgets: apply of a 256-operation Projects batch measured 464 ms in release on the headless build host against 50 ms (Quick notes apply 229 ms and seal of 100 notes 73 ms miss their milestone 1 budgets on the same host). Decision: one engine-track profiling step on apply and seal after milestone 11 step 4; take cheap wins, else lower the per-transaction batch cap so one write transaction stays within budget. Migrations 1 to 3 and first seal on the dense dataset were not measured; that profiling step measures the first seal.
  - Physical acceptance pending: the scenario table under Manual acceptance (two desktops and one Android phone).
