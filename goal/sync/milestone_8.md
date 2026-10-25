# Milestone 8: Notes

**Status: Planned.** Part of the [concurrent sync goal](README.md). This file is the implementation plan for converting Notes: folders, pages, blocks, databases with their properties, values, relations, and views, templates, comments and suggestions, and managed Notes assets. The owning contracts are [Device linking and synchronization](../../docs/data/sync.md), [Sync merge rules](../../docs/algorithms/sync/README.md), [Sync schema](../../docs/data/schema/sync.md), [Notes and Projects schema](../../docs/data/schema/notes-and-projects.md), and the [Notes feature docs](../../docs/features/notes/README.md). Each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- Folders, pages, blocks, databases, rows, property values, relations, views, templates, comments, suggestions, page icons, covers, and attached files are editable on every linked device offline and converge.
- Concurrent edits to different blocks of one page, or to different properties of one row, all survive with no conflict. Concurrent edits to the text of the same block, a page title, a comment body, or one property value show a conflict with keep this, use other, and keep both.
- Moving a page or block on two devices at once never creates a cycle or a lost subtree; deleting a page on one device while another edits inside it leaves a recovery entry.
- Search, backlinks, rollups, mention reminders, and page history stay correct on every device after remote changes, maintained by post-apply hooks and per-device history.
- Block text is one register per block, the exact field group a later Yrs Text kind replaces in the sharing goal. No Yrs dependency and no `.yjs` directory exist.

## Scope

In:

- Classification of every `notes_*` table as Replicated, Derived, or Local, with groups, keys, and bounds.
- Schema changes that make the data mergeable: block text and attributes split, order keys, page placement as one group, property definitions, options, and values as rows, canonical relation edges, a row parent column, and device view state moved out of payloads.
- A Notes structure repair pass for cross-table containment cycles and pairing rules the generic tree repair cannot see.
- Post-apply hooks for every Notes index, page history capture of remote changes, and mention reminders.
- Asset classes for page icons, covers, and files, carried by milestone 2's streamed transfers up to 64 MiB, which cover the 50 MB Notes file limit.
- Editor rebase and conflict UI for blocks, titles, properties, comments, and templates.
- Removal of `notes_collaboration_operations`, the `.yjs` vault directory, `sync_version` columns, and the local user display name fan-out.

Out:

- Character-level text merging (Yrs) and multi-person authorship, which belong to the [sharing goal](../sharing/README.md).
- Notes favorites, sidebar orders, and custom orders: milestone 4 moves them to preferences.
- Replication of page history: every device keeps its own.
- Manual database row order, which does not exist today.

## Prerequisites

- Milestone 2: adapter registry, `Local` tables and columns, guard skip during apply, derived keys with an existence register and the composite row key helper, trees with cycle repair, cascade hiding with cause tombstones, generic owned children keyed `(owner, ordinal)`, capture seeding and batched sealing (step 6), `MaterializeHook`, `AssetClass { name, max_bytes }` and `AssetStore` with streamed transfers, conflict and recovery presentation with `open_conflicts`, winner elision, and declared domain triggers, plus the shapes listed under [Engine shapes used](#engine-shapes-used).
- Milestone 6: `projects` is Replicated, so folders and pages can reference it.
- Milestone 4 is not required. Until it lands, favorites, orders, and `profile.displayName` stay in `config.json` and keep working with replicated page ids.

## Current state

Baseline lines refer to `crates/ganbaru-db/migrations/20261004220000_baseline_schema.sql` (B). Notes owns TableIds 288 to 351.

- 48 `notes_*` tables: 45 in B:2134 to B:3558 plus `notes_comment_reactions`, `notes_comment_thread_mutes`, and `notes_page_notification_settings` from `20261011120000_notes_comment_reactions_and_notifications.sql`. The trigger block B:3566 to B:3703 holds folder parent, folder and page project membership, and the row hierarchy detach on page move.
- `notes_pages` B:2248: no project column; the project is `properties.__ganbaru_project_id` (`PAGE_PROJECT_ID_PROPERTY`, `crates/ganbaru-notes/src/lib.rs:31`) checked by triggers B:3639 to B:3687. A four-way parent CHECK over `parent_type`, `parent_page_id` (self CASCADE), `parent_block_id` (no FK), and `parent_data_source_id` (CASCADE); `folder_id` SET NULL. All property values for a row live in the `properties` JSON column.
- `notes_blocks` B:2426 (indexes B:2497 to B:2499): `page_id` CASCADE, a parent pair with a self CASCADE, `has_children`, `in_trash`, a Notion-shaped `payload` JSON with text and attributes together, `plain_text`, REAL `sort_order`, and 37 types. Device view state (toggle open, editing lock) and `__ganbaru_trash_owner` (`writes/database_lifecycle.rs:108,224`) live inside `payload`. 24 Rust queries in `crates/` use `json_extract` on `payload`.
- `update_block_tx` (`writes/block_commands.rs:128-203`) records a page snapshot, rewrites type and payload, copies a `child_page` title into `notes_pages`, and refreshes mentions, asset references, and `touch_page`. Its conflict error is "Notes edit conflict: block {id} changed; reload before retrying".
- Editor deletion is soft (`in_trash`). Hard deletes are page history restore, which deletes every block of the page and reinserts them (`page_history.rs:277`), and Trash purge (`writes/page_lifecycle.rs:264`). Cross-page moves exist (`writes/block_moves.rs`, `writes/compound.rs` `destination_page_id`), guarded by recursive CTEs that are not cycle-safe.
- `notes_databases.id` is an FK to `notes_blocks(id)` CASCADE and duplicates the block's parent columns, refreshed by `refresh_moved_database_parents` (`block_moves.rs`). `notes_data_sources.properties` B:2587 holds the schema JSON including select options. `notes_data_source_relation_links` B:2606 duplicates relation values that `data_sources/relations.rs` writes on both sides of a dual relation. `notes_data_source_row_hierarchy` B:2640 is keyed by `row_page_id`. Data source templates use `is_default` with a partial UNIQUE index.
- Templates are snapshots: `update_page_template` (`templates.rs:161-230`) replaces all template blocks from the page; data source templates do the same (`data_sources/templates.rs`).
- Comments B:2717 to B:2796 and suggestions B:2502 carry `sync_version`; thread and suggestion status are coupled to their actor and time columns by CHECKs. `notes_comments.created_by` defaults to `'local-user'` with a `display_name` JSON copy; `update_local_user` (`local_user.rs:21-72`) rewrites that copy in every comment and suggestion, called from `ProfileSection.svelte:153`. `notes_comment_reactions` (primary key `comment_id, created_by, emoji`, `CASCADE` from comments, the same `display_name` copy) is written by `toggle_comment_reaction` (`comments.rs:504`).
- `notes_collaboration_operations` B:2146 is appended by `collaboration_operations.rs:19` `record_tx` from ten call sites in three modules (comments, suggestions, `project_history/restore`) and is part of JSON export and restore.
- `notes_assets` B:3002 has a random id and a UNIQUE `asset_path` restricted to `notes/page-icons/`, `notes/page-covers/`, and `notes/files/`. `notes_page_cover_assets` B:3490 and `notes_page_icon_assets` B:3514 duplicate it. History asset pins B:3464 reference `notes_assets` with `NO ACTION`.
- History: bundles, chunks, snapshots, settings, maintenance, project versions, pins, and dirty markers (B:3332 to B:3558).
- Derived indexes: search B:2934, backlinks B:2817, unresolved links B:2965, asset references B:3090, link facts B:3179, rollup cache B:2623, their `_state` tables, and mention notifications B:2895.
- `.yjs` is created by `apps/client/src-tauri/app/src/vault.rs:532` (asserted at :1319), carried by `crates/ganbaru-sync-replica/src/backup.rs:40`, and listed in `AGENTS.md:152` and `docs/data/sync.md:49,198`. Nothing reads it.
- Transport (milestone 2 step 8): managed assets stream by class through chunked staged files up to `MAX_SYNC_ASSET_BYTES = 64 MiB` (`crates/ganbaru-handoff/src/protocol/sync.rs:237`, chunks `SYNC_ASSET_CHUNK_BYTES` at :232), above the 50 MB Notes file limit; a pass is capped at 64 MiB (`crates/ganbaru-sync-replica/src/assets.rs:44`). `PROTOCOL_VERSION` is 12 (`protocol.rs:19`) and `VAULT_MANIFEST_VERSION` is 11; both take the next free number at merge.
- Engine: seal limits are 1,024 changes and 512 KiB per operation (`crates/ganbaru-sync-contracts/src/bounds.rs:9,15`) and a single change that does not fit errors (`seal.rs:604`); capture triggers are per group with `UPDATE OF` and OLD and NEW guards (`triggers.rs:239-268`). Post-apply hooks are registered per table (`Hooks` holds at most one per table), so `NotesHook` registers once for each Notes table it watches. Notes TableIds 288 to 351 are free; distractions end at 287.

## Design

### Engine shapes used

All from milestone 2; this milestone edits no shared engine file. Local, derived keys with composite row keys, Tree (self Reference plus Position), cascade hiding, owned children, hooks, asset classes, presentation, and winner elision, plus these shapes Notes depends on:

1. Reference cycles across tables (milestone 2 step 3). Notes containment is cyclic by nature: pages contain blocks, a `child_database` block contains a database, its data sources contain row pages. Those tables materialize together under deferred foreign keys, and cascade hiding walks with a visited set.
2. Register groups with several reference columns (milestone 2 step 3), for page placement and block placement.
3. Hook-maintained derived columns (milestone 2 step 7): never captured, recomputed by `NotesHook`, excluded from the summary fixture. Adapter `derived` stays for values computable from the row alone.
4. Unchanged-group elision at seal (milestone 2 step 10). Block saves rewrite whole rows today.
5. Streamed asset transfers up to 64 MiB (milestone 2 step 8), for 50 MB Notes files.

### Classification

Device state that must commit with domain writes is Local; indexes rebuilt from replicated rows are Derived and refreshed by the Notes hook; everything else a user authors is Replicated.

| Class | Tables |
| --- | --- |
| Replicated | 288 to 309 below |
| Derived (hook) | `notes_search_index` and FTS, `notes_backlink_index`, `notes_unresolved_link_index`, `notes_link_facts`, `notes_asset_references`, `notes_data_source_rollup_cache`, all `_state` tables |
| Local | `notes_undo_state`, `notes_edit_receipts`, `notes_comment_thread_reads`, `notes_comment_thread_mutes`, `notes_page_notification_settings`, `notes_mention_notifications`, new `notes_block_view_state`, every history table including `notes_page_history_settings`, new `notes_page_history_pending` |
| Dropped | `notes_collaboration_operations`, `notes_page_cover_assets`, `notes_page_icon_assets`, `notes_data_source_row_hierarchy`, `notes_data_source_relation_links` (renamed), every `sync_version` column, the B:3566 to B:3703 triggers |

### Replicated tables

| Id | Table | Key | Groups |
| --- | --- | --- | --- |
| 288 | `notes_local_users` | Seed id | created (`id`, `created_time`) immutable, the only group. |
| 289 | `notes_folders` | Random | created (immutable); name; tree (`parent_folder_id` SetNull, Tree with cycle repair); project (`project_id` Reference Cascade to `projects`). |
| 290 | `notes_pages` | Random | See [Pages](#pages). |
| 291 | `notes_page_aliases` | Random | alias (`alias`, `normalized_alias`), duplicate repair on `normalized_alias`; `page_id` immutable Cascade. |
| 292 | `notes_blocks` | Random | See [Blocks](#blocks). |
| 293 | `notes_databases` | Equals its block id | created; `block_id` immutable Reference Cascade to the `child_database` block (replaces the FK on `id`); title; description; appearance; inline; lifecycle; touched (Max). Parent columns are hook-derived. |
| 294 | `notes_data_sources` | Random | created with `database_id` immutable Cascade; title (`title`, `title_rich_text`); description; icon; lifecycle; touched; default template (`default_template_id` Reference SetNull, replaces `is_default`). |
| 295 | `notes_data_source_properties` (new) | Derived `(data_source_id, property_id)` | identity (`data_source_id`, `property_id`) Immutable; exists; name; config (type and type options except select options), surfaced; position (`order_key`). |
| 296 | `notes_data_source_property_options` (new) | Derived `(data_source_id, property_id, option_id)` | identity (the three key columns) Immutable; exists; name; color; position. Concurrent option adds both survive. |
| 297 | `notes_page_property_values` (new) | Derived `(page_id, property_id)` | identity (`page_id`, `property_id`) Immutable; exists; value (`value_type`, `value` JSON), surfaced. Title and relation properties are not stored here. |
| 298 | `notes_relation_edges` (renamed) | Derived `(source_page_id, source_property_id, target_page_id)` | identity (the three key columns) Immutable Cascade to both pages; exists. Cascade to both pages. |
| 299 | `notes_database_views` | Random | parent (`database_id`, `data_source_id`) Immutable with reference columns, `Cascade`; name; type and config, Coupled; position. |
| 300 | `notes_page_templates` | Random | source (`source_page_id`) Reference, `SetNull`; name; content (properties, icon, cover) with `notes_page_template_blocks` (301) as owned children, surfaced. |
| 302 | `notes_data_source_templates` | Random | parent (`data_source_id`) Immutable with a reference column, `Cascade`; source (`source_page_id`) Reference, `SetNull`; name; content with `notes_data_source_template_blocks` (303) as owned children, surfaced. |
| 304 | `notes_comment_threads` | Random | created (`parent_type`, `parent_page_id`, `parent_block_id`, `created_time`) immutable; status (`status`, `resolved_at`, `resolved_by`) Coupled, surfaced. `page_id` is hook-derived. |
| 305 | `notes_comment_thread_anchors` | `thread_id` | block (`block_id` Reference Cascade); range (offsets and quote) silent. |
| 306 | `notes_comments` | Random | created (thread, author, time) immutable; body (`rich_text`, `attachments`) surfaced; deleted (`deleted_at`) Coupled, set wins. |
| 307 | `notes_suggestions` | Random | content immutable; status with actor and time columns Coupled by priority accepted, then rejected, then open, because acceptance already changed the block text. |
| 308 | `notes_assets` | `id` is `{class}:{sha256}` from `asset_path` (for example `notes/page-covers:{sha256}`) | Immutable group holding only values computed from the bytes (class, digest, sniffed content type, byte size), so two devices adding the same bytes write equal values; created time and original file name are a Register. Never deleted, so no existence group. `storage_state` and `missing_at` are Local columns. |
| 309 | `notes_comment_reactions` | Derived `(comment_id, created_by, emoji)` | identity (the three key columns) Immutable with `comment_id` a reference column, `Cascade` to 306; created (`created_time`, `display_name`) Register; exists. Toggling off writes existence 0, so a concurrent remove and add resolve by the existence register. Local reactions follow the table 288 display name rule: step 1 stops `toggle_comment_reaction` writing `display_name`, and the column stays for imported authors. |

310 to 351 stay reserved for Notes.

Decisions behind the less obvious rows:

- Property schemas and values become rows because one JSON column per row or data source turns every concurrent edit to different properties into a conflict. Values merge per `(page, property)`. After a concurrent type change, a value whose `value_type` no longer matches the property is kept and rendered with Convert and Discard instead of being dropped.
- Tables 295 to 298 and 309 have composite primary keys, so each gains `sync_key TEXT NOT NULL UNIQUE` written through the `ganbaru-sync-contracts` `derived_key` helper on every insert. The step 2 migration fills existing rows with the `'pending:' || rowid` placeholder; the Notes Rust conversion pass at vault open rewrites every placeholder to the derived value before capture seeding, and a startup assertion refuses any remaining placeholder.
- Relation edges are canonical; relation values in page properties are gone. A dual relation stores each edge once under the property with the smaller `(data_source_id, property_id)`, and the reverse side is read from edges by target, so the two sides can never disagree and `relations.rs` stops writing reverse values.
- Templates use owned children because a template is edited as a whole snapshot; delete and reinsert is exactly that, and a conflict shows two complete snapshots. Template content is bounded to 256 KiB encoded and rejected on save above it. Step 2 rekeys both template block tables to `(template_id, ordinal)`, keeps `id` under `UNIQUE (template_id, id)` for the parent link, and replaces REAL `sort_order` with ordinal order. Owned rows materialize under deferred foreign keys, so parent links inside one template need no order. Conformance refuses foreign keys into an owned table from outside its owner; the baseline has none (B:2338, B:2665), and if one appears, template blocks stay Replicated with immutable snapshot ids instead of owned.
- Row hierarchy becomes a `parent_row_page_id` column on pages, so it merges as a register with cycle repair instead of a second keyed table.
- Every `NO ACTION` foreign key into a Notes replicated table is rebuilt in step 1: suggestion `created_by` (B:2506) becomes `CASCADE` and `accepted_by` and `rejected_by` (B:2516, B:2518) become `SET NULL` against `notes_local_users`; the Local history snapshot `created_by` (B:3384) and asset pins (B:3464) store ids without an FK.
- Table 288 stays Replicated because replicated tables reference it and may not reference Unconverted tables. Step 1 drops its `display_name` and `last_edited_time` columns; readers take the local user's name from the replicated `profile.displayName` preference (milestone 4), and local comments and suggestions no longer copy it, so renaming writes one preference instead of every comment. Imported authors keep their stored `display_name`.

### Pages

| Group | Columns | Kind |
| --- | --- | --- |
| created | `created_time`, source and template provenance | Immutable |
| title | `title` | Register, surfaced |
| placement | `child_block_id`, `parent_data_source_id`, `folder_id` | Register with three references |
| project | `project_id` (new, nullable, replaces `__ganbaru_project_id`) | Reference SetNull to `projects` (decided at step 1 review: a project delete must not delete Notes content, so its pages become unassigned workspace pages); null on nested pages, whose project is derived through the bounded ancestor walk, and on unassigned workspace pages, which the parent CHECK and `reads.rs:386` already allow |
| row parent | `parent_row_page_id` (new) | Self Reference SetNull, Tree |
| appearance | `icon`, `cover` | Register |
| lifecycle | `in_trash`, `trashed_time`, `trash_owner` (new column) | Coupled |
| archived | `archived` | Register |
| touched | `last_edited_time` | Max |

- A page nested in another page has `child_block_id = id`, a Reference Cascade to its `child_page` block, which follows the existing convention that the block and the page share an id. Where the page sits is therefore decided only by its block's placement and order key; the page and its block cannot disagree.
- Placement is one group so the CHECK "at most one of `child_block_id` and `parent_data_source_id`, and `folder_id` only when both are null" holds for every winning value.
- `parent_type`, `parent_page_id`, and `parent_block_id` stay as hook-derived columns so existing reads keep working. The four-way CHECK becomes a CHECK over replicated columns only.

### Blocks

| Group | Columns | Kind |
| --- | --- | --- |
| created | `id`, `created_time`, source columns | Immutable |
| placement | `parent_page_id`, `parent_block_id` | Register with two references, both Cascade, exactly one set; Tree on `parent_block_id` |
| position | `order_key` (replaces REAL `sort_order`) | Position |
| kind | `type`, `attrs` JSON | Register, silent |
| text | `text` JSON (new) | Register, surfaced |
| lifecycle | `in_trash`, `trash_owner` (new column) | Coupled |
| touched | `last_edited_time` | Max |

- `text` holds the type's text keys only (`rich_text`, `cells`, `caption`) as canonical JSON; `attrs` holds everything else. `payload` is dropped and composed in one row mapper from `type`, `attrs`, `text`, and `notes_block_view_state`. This split is the Yrs door: a later Text kind replaces exactly the `text` group, keyed by the stable block id.
- `kind` is silent: concurrent type changes resolve by the register winner and keep the text group, so turning a paragraph into a heading on one device and editing its words on another keeps both.
- `page_id` is hook-derived without an FK; `plain_text` is adapter-derived from `text`. `has_children` is dropped and computed with `EXISTS`. The `child_page` title is no longer copied into the block; reads join the page.
- Bounds: `text` 256 KiB and `attrs` 64 KiB encoded, checked in validation and in the adapter. Import and paste split oversized text into consecutive blocks.

### Notes post-apply hook

One `NotesHook` in `ganbaru-notes`, registered by the app. For the changed keys of an apply it:

1. Fills hook-derived columns: `page_id` for moved blocks and their subtrees, page parent columns, database parent columns, thread `page_id`.
2. Refreshes search and FTS, backlinks, unresolved links, link facts, asset references, rollups that read changed rows, and reminder candidates in `notes_mention_notifications` (rebuilt by `20261011190000_notes_property_reminders.sql`: mentions and date property values with `remind_minutes_before`), refreshed from changed blocks and changed `notes_page_property_values` rows. Candidates already in the past when first seen on a device are marked delivered without notifying.
3. Inserts changed page ids into `notes_page_history_pending` and marks touched projects in `notes_project_history_dirty`. The existing snapshot job captures pending pages as snapshots labeled as coming from another device.

The hook writes only Derived and Local rows and hook-derived columns, so it never captures.

### Structure repair

The generic tree repair handles self-references (folders, blocks, row parents). Cross-table rules run in a Notes repair pass after each apply commit, as an ordinary captured local transaction started by the app sync service. It reads only the keys of the batch, their ancestors, and their containers, never the whole vault. Every rule is deterministic, so concurrent repairs on two devices write equal values and conflicts on equal values do not exist.

- Container cycle (page through `child_page` blocks or databases back to itself): the page with the smallest id in the cycle becomes a root page of its project.
- Pairing: a `child_page` block whose page has `child_block_id` null sets it; a page whose block is gone is already hidden by cascade.
- Nested and row pages take their container's project; a folder in another project is cleared from the page.
- Row parent pointing to another data source, or deeper than 32, is cleared.
- A value whose property no longer exists is deleted.

Every recursive CTE in `ganbaru-notes` becomes cycle-safe (`UNION`, not `UNION ALL`) and depth-bounded, so a transient cycle before repair cannot hang a read.

### Restore, trash, and undo

- Page history restore becomes a diff against the current page: changed blocks are updated in place, blocks missing from the snapshot are trashed, and snapshot blocks whose keys are tombstoned get fresh ids. It never deletes and reinserts, so other devices see ordinary edits and concurrent remote edits conflict normally. Project restore follows the same rule.
- Trash purge runs per device from replicated `trashed_time`; every device reaches the same decision, and a restore concurrent with a purge becomes a recovery entry. Purging a nested page deletes its `child_page` block and the page in one transaction; folder, root, and row pages delete the page row directly. Remote cases are covered by cascade hiding.
- Notes uses no deletion holds and no publication predicates: deletion is soft, and Undo writes ordinary edits. An Undo entry whose target changed remotely since it was recorded is skipped with a notice instead of overwriting.

### Editor

- Remote applies refresh blocks the editor has not changed. A block with unsaved local text keeps it.
- On a revision mismatch at save, the milestone 1 three-way rebase runs per group: groups only the editor changed keep the local value, groups it did not change take the canonical value. When both sides changed `text`, that block stops saving and shows the inline choice use mine, use theirs, or keep both (mine saved, theirs inserted as the next sibling). Other blocks keep saving.
- A surfaced conflict, read through `open_conflicts`, shows an inline banner on the block with keep this, use other, and keep both, listing device and time. Page title conflicts show in the page header, property value conflicts in the property cell, template conflicts in the template editor, comment body and thread status conflicts in the comment panel. Strings go in the `notes` catalogs.

### Assets

- Classes `notes/page-icons` (3 MB), `notes/page-covers` (8 MB), and `notes/files` (50 MB), with files named by the engine's `{sha256}.{ext}` path function. Every device fetches every class eagerly. The `notes_assets` id is `{class}:{sha256}`, so the same bytes added on two devices are one row and every reference (block `attrs`, page `icon` and `cover`, comment attachments) agrees. Step 1 rewrites existing ids and references.
- Transfers use milestone 2's streamed asset path (step 8), which already allows 64 MiB per asset; this milestone changes no wire format and no protocol version.
- Asset rows are immutable and kept forever like tombstones, so no deletion can race a new reference. Bytes are collected by milestone 10's sweep, which counts every live row and stored version. Page history is per device: `notes_project_history_asset_pins` is a Local table that stores asset ids without an FK and declares them as references, so the sweep keeps what that device's history needs.

### Removals

- `notes_collaboration_operations`, `record_tx`, and their use in JSON export and restore; history and sync replace them.
- `sync_version` columns; revisions come from the engine.
- The `.yjs` directory in `vault.rs`, `backup.rs`, `AGENTS.md`, and `docs/data/sync.md`. Existing development vaults keep an empty folder that nothing reads.

### Migrations

One migration per conversion step, named with the UTC time of writing. The step 1 and 2 migrations reshape tables and move data (payload split, order keys from `sort_order` ranks, values and options out of JSON, row hierarchy into the column, project property into the column). Conversion steps install capture triggers and engine registrations only.

## Testing

- Conformance, codec round-trip, and crash-boundary tests for tables 288 to 308, driven by the manifest.
- Simulation scenarios: concurrent edits to different blocks of one page; the same block's text on two replicas giving one surfaced conflict everywhere; type change against text edit keeping both; concurrent moves of two blocks under each other; a page moved into its own descendant through a `child_page` block on another replica, repaired to the same root everywhere; page deletion against an edit inside it; concurrent select option adds; a property type change against a value write; dual relation edits from both sides; template edits on two replicas; comment delete against edit; suggestion accept against reject; history restore against a remote edit; the same file attached on two replicas.
- Hook: index output equals a full rebuild after every simulated apply; hook-derived columns equal a recomputation.
- `ganbaru-notes`: payload composition round-trips every block type fixture; text and attrs bounds; import splitting; cycle-safe CTEs on a planted cycle; restore as diff; relation reverse reads; value type mismatch reads; repair rules; `sync_key` placeholder conversion and the startup assertion.
- Assets: a 50 MB `notes/files` asset syncs through the streamed transfer; class limits, `{sha256}.{ext}` naming, and the derived `notes_assets` id agree on two replicas that add the same bytes.
- Frontend: Vitest for the block rebase helper, the conflict banner mapping, and the property cell mismatch state.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `refactor/notes-sync-structure` | No sync code. Block `text` and `attrs` split with the payload mapper, `notes_block_view_state`, order keys, `trash_owner` columns, page `project_id` and `child_block_id`, Rust validation replacing B:3566 to B:3703, `NO ACTION` foreign key rebuilds, cycle-safe CTEs, restore as diff, bounds and import splitting, `notes_local_users.display_name` dropped with readers on `profile.displayName`, drops of collaboration operations, `sync_version`, cover and icon asset tables, and `.yjs`. Needs nothing. | `cargo test -p ganbaru-notes --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 vault -- --test-threads=4` |
| 2 | `refactor/notes-sync-databases` | No sync code beyond the `derived_key` helper. Property, option, and value tables with `sync_key` and the placeholder conversion pass, canonical relation edges with reverse reads, `parent_row_page_id`, `default_template_id`, `notes_databases.block_id`, template block tables rekeyed to `(template_id, ordinal)`, value type mismatch reads, frontend API mappers. Needs 1 and milestone 2 step 4. | `cargo test -p ganbaru-notes --lib -j 4 data_sources -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/api/notes/databases.test.ts --maxWorkers=1` (new) |
| 3 | `feat/sync-notes-structure` | Tables 288 to 292, `notes.rs` manifest and adapter, Local and Derived classification for every Notes table, `NotesHook`, repair pass, history pending capture. Needs 1, milestone 2, and milestone 6. | `cargo test -p ganbaru-sync --lib -j 4 notes -- --test-threads=4`, `cargo test -p ganbaru-notes --lib -j 4 repair -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 notes -- --test-threads=4` |
| 4 | `feat/sync-notes-databases` | Tables 293 to 303 with owned template blocks, rollup and relation hook paths. Needs 2, 3. | `cargo test -p ganbaru-sync --lib -j 4 notes -- --test-threads=4`, `cargo test -p ganbaru-notes --lib -j 4 data_sources -- --test-threads=4` |
| 5 | `feat/sync-notes-comments` | Tables 304 to 307 and 309, comment and suggestion presentation. Needs 3. | `cargo test -p ganbaru-sync --lib -j 4 notes -- --test-threads=4`, `cargo test -p ganbaru-notes --lib -j 4 comments -- --test-threads=4` |
| 6 | `feat/sync-notes-assets` | Table 308, the three asset classes over milestone 2's streamed transfers, history asset pins as a Local table declaring references, without an FK. Needs 3. | `cargo test -p ganbaru-sync --lib -j 4 notes -- --test-threads=4`, `cargo test -p ganbaru-notes --lib -j 4 assets -- --test-threads=4` |
| 7 | `feat/sync-notes-editor` | Editor refresh on apply, block rebase, conflict banners for blocks, titles, values, templates, and comments, catalogs. Needs 3; the value and comment parts need 4 and 5. | `pnpm --dir apps/client exec vitest run src/lib/notes/editor/block-rebase.test.ts --maxWorkers=1` (new), `pnpm --dir apps/client run check` |
| 8 | `docs/sync-milestone-8` | Remaining docs, acceptance results mapped to automated coverage, goal README. | `pnpm -w run validate` |
| 9 | None | Physical acceptance on the devices below, recorded here. | Manual |

Steps 3 to 7 ship in one release. Step 1 is useful on its own and may merge while milestone 2 is still landing; step 2 waits only for milestone 2 step 4.

## Documentation

- `docs/data/sync.md`: remove `.yjs` from the archive layout (47) and the cleanup sentence (180); replace the Yrs paragraph (142) with block text as a surfaced register per block and Yrs moved to the sharing goal; storage ownership row (172): Notes content, templates, comments, suggestions, Trash, and assets replicate, while page history, undo, receipts, read state, mention delivery, and block view state are device-local; replace the move-op sentence (196) with trees as parent references plus order keys with deterministic cycle repair; rewrite the Notes editing section (203 to 205) with per-block text registers, the editor rebase, and conflict choices; delivery row (243) drops Notes and managed assets from the planned list. This closes the README row for Notes block text.
- `docs/data/schema/sync.md`: the currently replicated list.
- `docs/data/schema/notes-and-projects.md`: classification, the new and dropped tables, block text and attrs, page placement, property rows, canonical relation edges, row parent, the repair rules, and the history and asset storage rules.
- `docs/features/notes/editor.md` (63): replace the planned Yrs paragraph with the shipped conflict behavior. `links-and-collaboration.md`: remove operation records (63), state the stale decision rule as shipped (65), replace "Planned encrypted CRDT sync" (77). `history-and-recovery.md`: history is per device and captures remote changes; restore writes new edits; remove the collaboration operation sentences and the planned paragraph. `databases.md`: concurrent property and value behavior and the mismatch state. `import-export.md` (81): JSON graph export no longer contains collaboration operations.
- `docs/architecture/integrations.md` (51): Yrs belongs to multi-person collaboration.
- `docs/features/README.md` Notes and sync status rows.
- `AGENTS.md`: remove `.yjs/` from the folder structure (151).

## Manual acceptance

Two desktops (A is the coordinator, B is linked) and one Android phone (P).

| Scenario | Expected |
| --- | --- |
| Edit different blocks of one page on A and P offline, then reconnect | Every edit present on all devices, no conflict. |
| Edit the same paragraph on A and B offline | One conflict banner on every device; keep both leaves two blocks. |
| Type in a block on B while A's edit to it arrives | B keeps typing; the use mine, use theirs, keep both choice appears for that block only. |
| Turn a paragraph into a heading on A while P edits its words | Heading with P's words. |
| Move page X under Y on A and Y under X on B offline | One of them becomes a project root on every device; nothing disappears. |
| Delete a page on A while P edits inside it offline | P's edits become a recovery entry; Restore brings the page back with them. |
| Add select options to one property on A and P offline | Both options exist everywhere. |
| Change a property from text to number on A while B writes text into it | B's value shows the mismatch state with Convert and Discard. |
| Link two rows through a dual relation on A and unlink on B | Both sides of the relation agree on every device. |
| Comment on a block on P, resolve the thread on A | Comment and resolution everywhere; reads stay per device. |
| Attach a 45 MB file on B | Opens on A and P; desktop memory stays flat during transfer. |
| Restore an older page version on A | B and P see ordinary edits; their own history lists the change as from another device. |
| Search on P for a word typed on A | Found after sync. |
| Rename the profile on A | Comment authors show the new name everywhere without rewriting comments. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Block save to sealed operation, 2 KiB text | Under 50 ms desktop |
| Apply of a 1,000-block page including hook and repair | Under 2 s desktop, under 6 s Android |
| Seal after pasting or importing 5,000 blocks | Under 5 s desktop |
| First publication of a 100,000-block vault | Under 60 s desktop |
| Repair pass when the batch violates nothing | Under 20 ms |
| Memory per asset transfer | One chunk plus hashing state |

## Risks

| Risk | Mitigation |
| --- | --- |
| The step 1 and 2 migrations touch every Notes row and could lose data | Fixture vaults covering all 37 block types and every property type; payload composition round-trip tests; documented development reset for unsupported vaults. |
| Composed payload drifts from the old payload shape the frontend expects | One mapper with round-trip tests against the current fixtures before the column is dropped. |
| Cross-table cycles slip past the repair pass and hang reads | Cycle-safe, depth-bounded CTEs everywhere, and a simulation invariant of no reachable cycle after repair. |
| Large pastes produce operations that do not fit | 256 KiB text bound with import splitting; seal splits by change count and size. |
| Hook cost on Android for large applies | Hook work limited to changed keys and their pages, measured on P. |
| Repair writes from two devices ping-pong | Rules are deterministic functions of replicated state; simulation asserts a fixed point within one round. |

## Deferred

- Yrs Text kind for block text, in the sharing goal.
- Replicated or named page history versions.
- Manual database row order.
- On-demand asset fetch, out of scope for this goal; every device fetches every class eagerly.

## Implementation checks

Settled from source:

- Pages without a project exist: only folder pages must carry `__ganbaru_project_id` (B:3647), nested pages never do, and `reads.rs:386` reads it as optional. `project_id` is therefore nullable (table above).
- Block and page ids are shared: `templates.rs:618` binds the page id as the `child_page` block id, and `databases.rs:35` joins `notes_blocks.id = notes_data_sources.database_id`. `child_block_id = id` stays an explicit reference column, so a future path that breaks equality fails the foreign key instead of misplacing a page.
- There are 24 `json_extract` reads in `crates/` touching `payload` (some read `__ganbaru_trash_owner`). Step 1 moves each to `attrs`, `text`, or a column; none may read a composed payload in SQL.

## Progress

- [x] Step 1: Notes structure preparation.
  - Landed (eadaf8cd): migration `20261024075934_notes_sync_structure.sql` part A drops `notes_collaboration_operations`, local user `display_name` and `last_edited_time`, comment thread `sync_version`; rebuilds comments, reactions, suggestions (local user FKs become actions), and page history snapshots (author id without FK). Local authors read `profile.displayName`; `notes_update_local_user` command removed; `.yjs` removed from vault creation, backup, AGENTS.md, and docs/data/sync.md.
  - Checks at eadaf8cd: ganbaru-notes lib 404 ok, ganbaru-db lib 132 ok, ganbaru-tauri-app lib 1022 ok, ganbaru-sync-replica guarded test ok (81 total), client check ok, focused Vitest 93 ok.
  - Plan-silent decisions: `notes_comments.created_by` has no default (the old 'local-user' default named a row that may not exist); the sync-replica guard test inserts a local user by id only because the name column is gone (test-only edit in a sync crate).
  - Landed (4f97afc7): managed asset ids become `{class}:{sha256}` (class is the managed directory, for example `notes/page-icons`); the migration merges rows that shared bytes into one row (available before missing, then earliest; review fix 26a50a8a), rewrites references and stored paths, clears derived link facts and search state for rebuild, and drops `notes_page_icon_assets` and `notes_page_cover_assets`. Recording an asset returns the row it resolves to. Checks: ganbaru-notes lib 404 ok, ganbaru-db lib 130 ok.
  - Plan-silent decisions: a path that already names a row with other bytes resolves to that row (payloads name paths); historical project bundles still carry path ids and are normalized at restore (phase F).
  - Landed (d71d3336): page `project_id` column (FK projects CASCADE, dangling values nulled) and `trash_owner` columns on pages and blocks replace `__ganbaru_project_id` and `__ganbaru_trash_owner`; the six baseline Notes triggers are dropped and their rules (folder cycles, folder page placement and project, row hierarchy detachment) run in Rust in `structure.rs`; the DTO and create request carry `project_id`; requests that put either reserved key in page properties are rejected. Checks: ganbaru-notes lib 404 ok, ganbaru-db lib 130 ok, ganbaru-tauri-app lib 1022 ok, client check ok, Vitest notes, stores, and api 2097 ok.
  - Plan-silent or deviating decisions (revised by review fixes 85ae5f3a and a56c538e): only workspace pages store `project_id`; nested pages store NULL and derive the project from the nearest ancestor that stores one through a bounded walk; the rule is enforced in Rust (no `project_id` and `parent_type` CHECK, so step 3 can derive it without a rebuild); the FK is ON DELETE SET NULL, so deleting a project keeps its pages as unassigned workspace pages; page history does not version `project_id` (restore keeps the current project).
  - Landed (ca9b8815): `notes_blocks` stores `text` (rich_text, cells, caption) and `attrs` (everything else) through one mapper in `block_storage.rs`; `ganbaru_open` and `editing_locked` move to device-local `notes_block_view_state`; `payload`, `has_children` (now EXISTS), and `sort_order` are gone; siblings order by `(order_key, id)` with migrated rank keys; `child_page` titles are read from the page. Bounds: text 256 KiB, attrs 64 KiB; imports split oversized rich text into continuation blocks; template tables keep `payload` and `sort_order`. Tests: 37-type round trip, bounds, rich text splitting, Markdown import splitting, migration fixture with every block and property type, fresh-vault invariants. Checks: ganbaru-notes lib 413 ok, ganbaru-db lib ok, clippy workspace all targets ok. `json_extract` on block `payload` in non-test code: 0.
  - Plan-silent decisions: the block `page_id` FK is dropped (Rust writes it from the placement); outline `sort_order` is a ROW_NUMBER rank; history bundles with `sort_order` are unparseable (no compatibility); the editing lock is view state and no longer records history; scans read `json_patch(attrs, text)`; template copies renumber each sibling group from visiting order; import continuation blocks carry no source id; Markdown unsupported source above 32 KiB imports as a paragraph of its text, and other unsupported blocks above the attrs bound drop `raw` with a warning; frontend paste does not split yet (step 2 should split in `planNotesPlainTextPaste` or rely on the backend refusal).
  - Landed (172e4cc8): page `child_block_id` (Reference CASCADE to `notes_blocks`, DEFERRABLE INITIALLY DEFERRED, CHECK equal to the page id, set exactly when the page is nested under a page or block, at most one of it and the data source parent, folder only when both are null); every writer sets it; deleting a `child_page` block deletes its page. Migration: nested pages whose block is gone become workspace pages; non-workspace folder ids are cleared. Tests: fresh-vault reference, CHECK, and cascade test; migration fixture covers a paired and an orphan nested page. Checks: ganbaru-notes lib 413 ok, ganbaru-db notes tests ok.
  - Plan-silent decisions: the tie between `child_block_id` and `parent_type` is a CHECK (the plan lists only the three-column CHECK) so the derived columns cannot disagree before step 2 derives them; index `idx_notes_pages_child_block` serves the cascade.
  - Landed (aac64309): page history restore is a diff: snapshot blocks whose row is still on the page (trashed or not, and for `child_page` still holding its nested page) are updated in place with snapshot order keys, current blocks the snapshot omits are trashed individually (with their nested pages), and only blocks whose row is gone or lives elsewhere get fresh ids. Device view state of kept blocks is untouched. This removes the page-scoped block delete, which the `child_block_id` cascade had turned into deletion of nested pages. Test: kept ids, nested content survives, missing block and page trashed, deleted block fresh id. Checks: ganbaru-notes lib 414 ok.
  - Landed (9f1d2ad6): project history restore is a diff: bundle rows upsert by primary key only when they differ (`ON CONFLICT DO UPDATE`, never delete and reinsert), omitted current pages, blocks of bundle pages, databases, and data sources are trashed, omitted folders are deleted, omitted leaf rows are deleted only when their owner is in the bundle; bundle pages and blocks outside the current project graph (moved or gone) and everything nested under them restore as copies with fresh ids (a page or database shares its id with its block, so pairs map together) and every bundle row is remapped to the copies; the temporary page-history preserve table and page deletes are gone; undo state, relation links, and rollup values of touched pages are cleared as the old cascade did; bundles without `attrs` or `child_block_id` are refused before the safety version. Preview "removed" counts untrashed pages and reads "moved to the trash" (en, es). Feature doc updated. Tests: in-place rowids for page, block, nested page, and nested block, later block and page trashed, page history snapshot survives, outdated bundle refusal. Checks: ganbaru-notes lib 416 ok, clippy ganbaru-notes ok.
  - Plan-silent decisions: changed aliases are deleted and reinserted (unique normalized alias could collide mid-write); relation links are cleared, not rebuilt, since a rebuild fails on trashed relation targets; old bundles are refused, so asset id normalization at restore (phase F note above) is moot.
  - Landed (cda541d3): all 36 recursive CTEs in ganbaru-notes use `UNION` and a depth bound: block-only walks bind `MAX_BLOCK_DEPTH`, page and folder ancestry binds `MAX_TREE_DEPTH` (literal 64s and `PAGE_LOCATION_MAX_DEPTH` replaced), and walks across pages, blocks, databases, and data sources that must be complete (permanent delete, descendant checks, agent bridge export) bind the new `MAX_CONTENT_WALK_DEPTH` (64 × 67). The page descendant check had no bound before. Results that a depth column could repeat read `DISTINCT` or `IN`. Tests: planted block cycle (moves into and of the cycle fail with the nesting error and roll back) and planted page cycle (location reads, descendant move refused, permanent delete removes both pages once). Checks: ganbaru-notes lib 418 ok, clippy ganbaru-notes all targets ok.
  - Plan-silent decisions: a cyclic block subtree fails the move with the nesting limit error instead of being repaired.
  - Landed (01108a36): docs/data/schema/notes-and-projects.md describes the text and attrs split with bounds, order keys, device-local view state, `child_block_id`, `trash_owner`, page `project_id`, `class:sha256` asset ids, Rust-owned structural rules with bounded recursive queries, and diff restore.
  - Merged feat/sync (4cd1238a, signed, no conflicts; the Notes migration still sorts last).
  - Review fixes (signed, no amend):
    - 85ae5f3a: page `project_id` FK is ON DELETE SET NULL; test that deleting a project keeps its pages.
    - a56c538e: nested pages store NULL `project_id` and derive it from ancestors (reads, shell filter, dirty marking, retention); `adopt_block_project` removed; move, template apply (now takes `project_id`), duplicate, import, and history restore store the project only on workspace pages; the migration nulls nested pages. Snapshot child page restore clears `trash_owner` and keeps existing properties, replacing only the title. Tests: moves across nesting, templates, project restore of nested pages, child restore.
    - 26a50a8a: asset merge prefers `storage_state = 'available'`, then `created_at` (migration test covers the merge, the reference rewrite, and the property URL rewrite); pins name their asset without an FK; `idx_notes_pages_visible` recreated; old history refusal (below).
    - c290ca74: project restore never copies a page or database still in the current project graph, nor its paired block, even when that block is gone (for example a purged trashed `child_page` block after un-nesting); test.
    - efd0fe93: split import blocks keep text order (continuations follow the first chunk, the last continuation takes the children and `is_toggleable`); continuations carry only text, `color`, indent, and code `language`, with neutral `checked` and `caption`; project restore block writes and database view replacement run the block size bounds; tests.
    - 8992a232: the a56c538e checks missed the tauri-app lib, where the empty Notes shell contract failed because shell page reads now embed the derived project walk (naming `notes_blocks`, `notes_databases`, `notes_data_sources`). Workspace pages now read their stored column through a CASE and the walk stops at the first workspace page; the contract drops those three containers from its forbidden list with a comment. Cost of the review's High 2: nested page project reads are a per-row bounded walk instead of a column read.
  - Old history behavior: page history snapshots and project versions saved before this migration stay in the history lists; opening or restoring them fails with "This version was saved in an older Notes format and can no longer be opened or restored". Project history `HISTORY_SCHEMA_VERSION` is 2, so older manifests are refused before any row is read; the outdated bundle check uses the same message.
  - Decision on oversized pre-migration blocks: no split and no length CHECK. A CHECK would fail the migration on existing vaults, and splitting in SQL or at startup is a compatibility shim that AGENTS.md rules out (no external users). Such blocks stay stored, readable, exportable, and in history; an edit, duplicate, or restore that still exceeds the bound is refused with the byte limit message, and the user can shorten or split the text.
  - Note for step 3: the CHECK on `folder_id` and `child_block_id` (migration around line 604) and the kept four-way parent CHECK read columns that step 3 derives through the hook, so step 3 must rebuild `notes_pages` and drop them.
  - Final checks at 8992a232: ganbaru-notes lib 427 ok, ganbaru-db lib 134 ok (run at efd0fe93; 8992a232 touched no db inputs), ganbaru-sync-replica lib 81 ok (1 ignored), ganbaru-tauri-app lib 1022 ok (empty Notes first-use contract included), clippy workspace all targets ok, fmt ok, desktop bin check ok, client check 0 errors, focused Vitest (notes, stores/notes, components/notes, api/notes) 207 files 1821 tests ok, editor-check ok, bundle-contracts ok.
- [x] Step 2: Notes databases preparation.
  - Landed (75347bb1): step 1 delta review fixes. Split callout and button continuations keep their required `icon` and `actions`; the agent bridge export binds the content walk depth on the descendant query (it was bound on the exact page query); the ganbaru-db Notes migration tests run through the step 1 migration; history doc states that old-format versions stay listed but cannot be opened. Checks: ganbaru-notes lib 429 ok, ganbaru-db lib 134 ok, fmt ok.
  - Landed (b2464ebd): migration 20261024161650 part one. Template block tables keyed by `(template_id, ordinal)` from a preorder walk in sort order (unreachable blocks dropped), unique `(template_id, id)`, deferred self parent cascade; 256 KiB template content bound checked when blocks are replaced from a page; `notes_data_sources.default_template_id` (SetNull) replaces `is_default`; `notes_databases.block_id` (equals id, Cascade to the `child_database` block). Checks: ganbaru-notes lib 429 ok, ganbaru-db lib 136 ok, fmt ok.
  - Decision: `default_template_id` must name a template of the same data source; enforced in Rust (set and clear helpers), not by a composite FK.
  - Decision: template content bound is the summed UTF-8 byte length of the template row and block columns.
  - Decision: the wire DTO keeps `is_default`, derived from `default_template_id`, so the frontend contract is unchanged.
  - Landed (8355198c): data source properties and options as rows. `notes_data_source_properties` (order_key, config object) and `notes_data_source_property_options` (position, status_group) with derived `sync_key` and `pending:` placeholders; `notes_data_sources.properties` dropped; reads compose the schema object in SQL, writes diff composed against stored rows; `sync_keys` module with `convert` and `assert_converted` (not wired yet); history restore covers both tables and rederives keys. Checks: ganbaru-notes lib 433 ok, ganbaru-db notes 25 ok, tauri-app notes 12 ok, fmt ok.
  - Decision: `sync_key` non-empty and bounded through CHECK constraints, not triggers (Notes tables have no triggers by invariant).
  - Decision: migrated property order is stored JSON key order (serde_json maps are sorted); new properties and options are placed through order placement; status group option order follows option position.
  - Decision: the property name is the schema key, with no unique name index; option extra fields are dropped and option ids are required; non-object schema entries are dropped by the migration.
  - Decision: derived prefixes `notes-property` and `notes-property-option`; `exported_property_schema_count` counts property rows.
  - Decision: historical page DTO data sources omit composed properties; trash journals live only on block roots (see the review fix round).
  - Decision: template blocks keep `has_children`; `HISTORY_SCHEMA_VERSION` is bumped once to 3 for all of step 2; project history restore replaces a template's whole block snapshot when it differs.
  - Landed (f2d6bdc6): page property values as rows (`notes_page_property_values`, keyed `(page_id, property_id)`, `value_type`, derived `sync_key`) and canonical relation edges (`notes_relation_edges` stored once under the canonical side, `notes_relation_links` symmetric read view, `notes_relation_pairs` view); `notes_pages.properties` dropped; reads compose properties in SQL; mismatch reads; search cursor rank as an exact decimal string. Checks: ganbaru-notes lib 436 ok, ganbaru-db notes 27 ok, fmt ok.
  - Decision: two-way pairing comes from `notes_relation_pairs`. One-sided dual config is allowed, a mutual pair wins, otherwise the smallest namer wins; relation scope widens to namers of the canonical side.
  - Decision: a value whose `value_type` differs from the current property type reads with its own type in get_page and every window (normalization keeps it instead of the default); filters, sorts, and reminders treat it as empty; the next edit of that cell replaces it with the current type.
  - Decision: title is `notes_pages.title` only, never a value row; relation values are edges only; formula, rollup, and button produce no value rows. Values for properties absent from the schema (orphans) stay stored but are not composed; rows leaving a data source keep their values and edges.
  - Decision: update_page and restore write value rows only; copies rewrite edges to copied targets inside the copy and keep external targets; the copy budget counts value rows and edges.
  - Decision: relation order is edge `created_time`, then target id; search cursor rank is an exact decimal string to avoid float ULP drift across pages.
  - Decision: trash roots journal keeps value rows and edges of trashed rows so restore is lossless.
  - Landed (cb969cb2): `ganbaru_notes::sync_keys::convert` and `assert_converted` run at vault open after Projects and Calendar; sync service start asserts Notes keys; headless sync tests convert Notes. Checks: tauri-app sync::service 8 ok.
  - Landed (5edbd0a6): frontend `notesDatabasePropertyCell` exposes empty, value, and mismatch cell states; table, list, timeline, and property display use matching payloads only (a mismatch renders empty until step 7 adds Convert and Discard); new `api/notes/databases.test.ts`. Checks: client check ok, vitest notes database 30 files ok.
  - Landed (bdfd9cba): `notes_pages.parent_row_page_id` (SetNull, no self parent, partial index) replaces `notes_data_source_row_hierarchy`; the migration keeps only edges whose child and parent are rows of the edge's data source; reads, set parent, copies, duplicates, detach, and project history use the column. Checks: ganbaru-notes lib 436 ok, ganbaru-db lib 140 ok.
  - Decision: the column means something only while both pages are rows of the same data source; writers clear it when either page leaves (detach on move and history page restore), deleting the parent clears it by FK; trash and archive keep it.
  - Decision: project history restores the column with the page row; restore rejects a parent outside the restored page set and validates cycles, depth, and same-source ownership before commit.
  - Merged feat/sync 0dacb4e4 (fdd45140, signed).
  - Landed (869e242a): clippy `explicit_auto_deref` fixes in ganbaru-notes after the merge.
  - Landed (ede8a49d): cell state helpers moved from `property-cell.ts` into `notes/database/table.ts` so the Android bundle stays within its source module baseline (no baseline bump).
  - Final checks after the merge: ganbaru-notes lib 436 ok, ganbaru-db lib 147 ok, tauri-app lib 1024 ok, workspace clippy all targets clean, fmt ok, ganbaru bin check ok, client check 0 errors, vitest notes database 30 files 209 tests ok, editor-check ok, bundle-contracts ok.
  - Review fix round:
    - Landed (6ecf5d60): `delete_link` deletes the reverse edge only for self-synced and peer pairings, so a one-way self relation keeps the separate edge; database copy keeps only links to active rows (archived or trashed targets no longer fail the copy). Tests cover one-way and self-synced removal and copying with an archived target.
    - Migration: values and relation references resolve their property by id, and by name only when the entry has no id; a stale id stays an orphan under its own id. Duplicate property ids in schema JSON resolve deterministically (first in stored JSON order wins). Extra values on non-row pages are kept as orphan values (hidden, since reads resolve values through the parent data source schema). Relation references only page JSON held are kept as edges when the source is a row and the target page exists, trashed or archived included; references to pages that do not exist, and relation entries on non-row pages, are dropped. JSON-only links take the source page's last edit time as created time. Tests cover these plus `unique_id` and `created_time` values.
    - Decision (item 6): `update_page` keeps the full replacement contract it had at 0dacb4e4 (the old code replaced the whole properties JSON, and no frontend caller sends `properties`; the frontend sends only title, icon, and cover). Omitted values are deleted; relation entries are ignored and links change only through row property and relation commands. Documented in the rustdoc and tested (full round trip, partial replacement, links untouched).
    - Item 4: duplicate names could arise through writers that skip `canonical_properties` (management, property actions, copy, Notion import, databases). `decompose_schema` now refuses duplicate property ids and names for every writer. Step 4 follow-up: names are unique per data source only by this Rust check, with no unique index, and the composed schema is keyed by name. Concurrent renames or adds on two devices can collide after merge, so step 4 must resolve a name collision deterministically (for example suffix the later property by order key or sync key, or key the composed schema by id) and must not let the diff writer delete the hidden property.
    - Item 3 step 4 follow-up: relation pairing is derived from property config at read time through `notes_relation_pairs`; when configs sync independently, a pairing change can move which side is canonical, so step 4 must rekey or re-canonicalize affected edges deterministically on merge (or store the pairing explicitly).
    - Frontend: filter evaluation and the timeline resize date read cells through `notesDatabaseMatchingPayload`, so a mismatched stored type reads as empty there too.
    - Import split: a split button keeps one actionable button (the first part, with its icon and actions); its continuations are paragraphs, while callout continuations stay callouts with the icon (presentational type). Tested.
    - Trash: only block roots keep a journal column; database and source objects have no journal column (they are never roots; the description array would have panicked on journal write).
    - Orphan values on non-row pages convert normally: the value sync key derives from `page_id` and `property_id` only.
    - Landed: b68a722d (migration and storage), 99af2e6c (page update contract, import split, trash, frontend helpers), 77813bfa (signed merge of feat/sync 97192d32, default git merge message), 7584ef60 (migration renamed to 20261024234744 so it sorts after `20261024202359_sync_chain_ends.sql`), 210cc1ad (test formatting).
    - Final checks after the merge: ganbaru-notes lib 440 ok, ganbaru-db lib 148 ok, tauri-app lib 1024 ok, workspace clippy all targets clean, fmt failed once on two test files and passed after 210cc1ad (formatting only; tests and clippy ran before it), ganbaru bin check ok, client check 0 errors, vitest notes database 30 files 209 tests ok, editor-check ok, bundle-contracts ok.
- [ ] Step 3: structure conversion, hook, and repair.
- [ ] Step 4: databases and templates conversion.
- [ ] Step 5: comments and suggestions conversion.
- [ ] Step 6: assets conversion.
- [ ] Step 7: editor rebase and conflict UI.
- [ ] Step 8: docs, validation, and automated acceptance.
- [ ] Step 9: physical acceptance.
