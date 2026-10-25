# Milestone 9: Chat organization

**Status: Planned.** Part of the [concurrent sync goal](README.md). Chat organization (teammates, access profiles, channels, memberships, grants, the message timeline, attachments, scheduled messages, and work assignments) becomes concurrently editable on every linked device, while execution stays on one device per assignment. The contract lives in [Device linking and synchronization](../../docs/data/sync.md), [Sync merge rules](../../docs/algorithms/sync/README.md), [Sync schema](../../docs/data/schema/sync.md), [Chat schema](../../docs/data/schema/chat.md), [Chat security](../../docs/data/security/chat.md), and the [Chat feature docs](../../docs/features/chat/README.md).

## Outcome

- Teammates, access profiles, channels, memberships, folder grants, messages and their revisions, mentions, read positions, attachments, scheduled messages, and work assignments replicate through the engine, and any linked device reads and writes them offline, Android included.
- Messages posted offline on two devices interleave the same way everywhere, edits of one message on two devices both survive as revisions with one current revision shown, and nothing is silently lost.
- A revocation (membership removal, grant revoke, capability or safety reduction) concurrent with a widening always ends restrictive, and the executing device stops affected runs.
- Every assignment and scheduled message has exactly one executing device. Receiving one never starts execution elsewhere, and a mention posted from the phone runs on the teammate's home desktop.
- Execution (threads, events, runs, worktrees, scratch, approvals, checkpoints, previews, terminals), provider configuration, credentials, paths, and provider homes stay on their device.

## Scope

In scope:

- Random organizational ids, the Local class for every execution and lifecycle table, FK rebuilds, and the Chat post-apply hook.
- Conversion of 29 tables in four groups: A identity and policy, B conversation shell, C timeline, D assignments and scheduled messages.
- Order keys replacing ordinals, with every integer comparator converted.
- Attachments as an engine asset class.
- The `chat` branch of `config.json` split into portable preferences and the device preference file.
- Conflict and recovery presentation, replica writes, docs, and physical acceptance.

Out of scope:

- Approvals and user-input answers from a device other than the executing one (deferred).
- Remote control of runs, terminals, or previews.
- Replicating review comments, checkpoints, drafts, or execution history.
- Multi-person Chat (the [sharing goal](../sharing/README.md)).
- Managed project folder contents ([milestone 10](milestone_10.md)).

## Prerequisites

- [Milestone 2](milestone_2.md): `Classification::Local`, `DomainTrigger` policies (`SameRow`, `Gated`, `ApplierVisible`, `DerivedIndex`) with parent-first apply, reference cycles materialized together under deferred foreign keys (reply threads and items reference each other), cascade hiding with grouped recovery, the composite row key convention with the `derived_key` helper in `ganbaru-sync-contracts` (step 4), generic owned children keyed `(owner, ordinal)` inside `Immutable`, `Register`, and `Coupled` groups, surfaced and silent `Coupled` groups with an adapter priority (step 7), `seed_captures` after every domain conversion pass (step 6), `MaterializeHook` (receives the connection and a change sink, never an `AppHandle`), `AssetClass` and `ContentAddressedStore` with streamed transfers up to 64 MiB (step 8), `RecoveryRestorers`, the frontend renderer registry, and the per-domain replica write flag.
- [Milestone 4](milestone_4.md): `vault_preferences` with its key registry, and the device preference file `<app_config_dir>/vault-preferences/<vault_id>.json`.
- [Milestone 6](milestone_6.md): `ensure_project_chat_defaults` (deterministic default rows built with `derived_key`, created only where Chat is writable), Projects and working folder bindings replicated (tables 96 to 114), and the fourteen Chat FKs into Projects rebuilt as `CASCADE` or `SET NULL`.
- [Milestone 7](milestone_7.md): `stable_device_id(app)` in `apps/client/src-tauri/app/src/sync/devices.rs` (milestone 7 step 4), which returns the sync membership device id paired with this installation, never the writer id.

Step 1 needs only milestone 2 step 4's `derived_key` helper. Step 2 needs no milestone 2 API and can land before milestone 2 finishes; it gates trigger bodies on milestone 1's `sync_apply_state.applying` directly.

## Current state

Re-verify against the source when the milestone starts.

- Organizational ids are `format!("{prefix}:{nanos:032x}{sequence:016x}")` in three copies: `apps/client/src-tauri/app/src/chat/organization/common.rs:553-560`, `chat/channels.rs:630-637`, and `chat/scratch/commands/cleanup.rs:404`. Two devices can mint the same id. `ganbaru_atomic_files::fill_random` (`crates/ganbaru-atomic-files/src/lib.rs:455`) is available and `ganbaru-chat` already depends on the crate. The frontend mints teammate ids `participant:{uuid}` (`teammates/editor-controller.svelte.ts:689`), channel ids `channel:{uuid}` (`ChatChannelSetupDialog.svelte:146`), client command ids, scheduled ids, and reference ids with `crypto.randomUUID()`.
- Agent output is projected into the timeline under derived ids `organizational-item:{sha256}` and `organizational-revision:{sha256}` (84 to 88 bytes) at `crates/ganbaru-chat/src/coordination/projection.rs:299,323` and `crates/ganbaru-chat/src/agent_runs.rs:203-204`, with no size bound beyond the SQL CHECK.
- The baseline (`crates/ganbaru-db/migrations/20261004220000_baseline_schema.sql`) defines Chat from line 3849. Composite primary keys without an id column: memberships (3974), teammate channel memberships (3991), grants (4039), and both read cursors (4074, 4128).
- Timeline order is an integer `ordinal` on `chat_conversation_items` (4166) with three unique indexes, message revisions carry `revision` with `UNIQUE (message_item_id, revision)` (4214), and `chat_communication_message_revision_ordinals` (4229) is an AUTOINCREMENT global order used for revision cutoffs. Read cursors store `last_read_root_ordinal` and `last_read_reply_ordinal`; teammate channel memberships store `history_from_ordinal`; channel reference targets (4256) store `source_lower_ordinal`, `source_high_ordinal`, `source_revision_cutoff_id`, and `destination_audience_revision`.
- Integer comparators over ordinals and revision numbers: `agent_runs.rs:206`, `coordination/projection.rs:316`, `organization.rs:595`, `scratch.rs:507`, `channels.rs:194,305`, `internal_mcp_tools/authorization.rs:170,202,285,342`, `internal_mcp_tools/channels.rs:86`, `organization/reads.rs:204,331,343,574,622,652`, `access/retained_references.rs:265,387,399`, `workflow.rs:150,655,895,939,974` and the cutoff computation at 660-720, `context.rs:106,138,552,1030,1139,1172,1214,1256` and the source inserts at 310 and 358-369, `dispatch.rs:640`, `access/channels.rs:30,165,453`, `access.rs:324` (`latest_policy_revision + 1`), and `send/coordinator.rs:705,788`. Also `organization/access/profiles.rs:194,254` (`latest_revision.revision + 1`) and `:325-336` (`history_from_ordinal`), `scratch/commands.rs:139`, `organization/common.rs:344-373` (history boundary `lower_ordinal`), and `internal_mcp_tools/audit.rs:141-158`; the Local `chat_assignment_authorized_channel_sources` (5160) and `chat_scratch_generation_sources` (5408) store `lower_ordinal` and `high_ordinal` item bounds, which become order key bounds. Turn ordinals in `repository/events/projections.rs:73`, `send/persistence.rs:308`, and `restore_commands/persistence.rs:14,31,43` are execution state and stay integers.
- Uniques that two devices can violate concurrently: teammate display name (participants), custom profile name (3921), channel name per project (4091), default grant per teammate (4039), one active assignment per reply thread (5061), revision number per message, reply thread `root_item_id` (4114), item ordinals, input ordinals (5267), and `client_command_id` on scheduled messages (4138).
- Triggers 5873 to 5990 enforce channel kind, membership identity, grant identity (reads mutable `removed_at`), audience monotonicity, default channel name and protection, reply root existence (reads items, which reference reply threads), scheduled and item reply thread consistency, author snapshot immutability, and current revision existence (reads child revisions). Publish triggers at 5718 and 5789 maintain `latest_*`; 5665 and 5812 to 5864 create and bump access and audience state; 5991 and 5997 maintain FTS; 6019 fills revision ordinals. Group A also carries the policy identity check (5672), revision immutability (5684, 5712, 5800, 5806, and 6026 on message revisions), builtin profile protection (5735, 5778), revision monotonicity (5728, 5751, 5758, 5901), and the audience bump on profile revisions (5765); 5684 refuses unless the teammate is archived and unreferenced, and 5800 refuses every delete.
- Hard deletes: `teammate_lifecycle.rs:101-160` `delete_unused_teammate` (desktop only, with a local "no durable history" check), `access.rs:360-378` deletes the teammate channel membership on removal, and `assignments.rs:76-175` `recover_stranded_follow_up_assignments` deletes and reinserts inputs on a new assignment. Scheduled messages are deleted on send or cancel (`organization/scheduling.rs:332,402,505`); capture turns those into tombstones, which is their intended lifecycle. The remaining `DELETE` sites (`chat_threads` in `repository/lifecycle.rs:167`, drafts, follow-up queues, activities, preview audit) touch Local tables.
- Assignment dispatch spawns only under `#[cfg(desktop)]` (`organization.rs:494-510`). Android registers post, schedule, scheduled dispatch, channel create, update, archive, and read commands (`runtime/mobile.rs:60-84`) and creates assignment rows it never executes. Approvals resolve through desktop commands against Local `chat_pending_requests` (`runtime/desktop.rs:473-474`).
- The revocation loop (`revocation.rs:140-222`) drains `chat_access_revocation_jobs`, which commands enqueue in the same transaction.
- Message bounds: `MAX_MESSAGE_BYTES = 128 * 1024` (`common.rs:13`, checked at :71); rich content is unbounded (:215-220); the SQL CHECK counts characters (`length(normalized_markdown) <= 131072`).
- Attachments: `MAX_ATTACHMENT_BYTES = 50 MiB` (`crates/ganbaru-chat/src/repository/attachments.rs:14`), SQL `byte_size BETWEEN 0 AND 52428800` (5480), file name `{sha256[..16]}-{safe(id)}.{ext}` (:137-143), written with `create_new` and no rename (:450-461). Reference rows point at `chat_attachments(id)` with `NO ACTION`.
- The engine already moves assets in chunks: `SYNC_ASSET_CHUNK_BYTES` and `MAX_SYNC_ASSET_BYTES = 32 MiB` (`crates/ganbaru-handoff/src/protocol/sync.rs:162-167`), and rejects `byte_size == 0` (:207). Milestone 2 step 8 streams transfers to temporary files and raises the bound to 64 MiB.
- `config.json` holds a native-owned `chat` branch (`apps/client/src-tauri/app/src/vault/config.rs:17`): `ChatVaultConfig` (`crates/ganbaru-chat-contracts/src/config.rs:128-138`) with providers (launch arguments, environment, credential references, models), automatic setup opt-outs, remembered selections, folder provider preferences, panels, and behavior.
- `merge.rs:192` resolves `Max` by comparing group fields in order, text by bytes (`compare_fields`, :147-154), so a two-field group `(order_key, item_id)` is a valid `Max` value.

### Re-verification (2026-10-24, feat/sync 12f3f0c2)

Stale citations: `fill_random` is `crates/ganbaru-atomic-files/src/lib.rs:577`; the `new_id` copies are `chat/organization/common.rs:553`, `chat/channels.rs:688`, and `chat/scratch/commands/cleanup.rs:404`; `ganbaru-chat` already depends on `ganbaru-sync-contracts` and `project_defaults.rs` uses `derived_key`; milestone 2 step 8 landed, so `MAX_SYNC_ASSET_BYTES` is 64 MiB (`protocol/sync.rs:237`, chunk size :232, zero-size refusal :301) and assets stream; merge citations are `Max` at `merge.rs:344`, Coupled priority at :335-340, `compare_fields` at :299-306; the config `chat` branch is `vault/config.rs:21`; mobile Chat registration is `runtime/mobile.rs:71-90`; desktop dispatch spawns are `chat/organization.rs:461-480`; `stable_device_id` (`sync/devices.rs:22`) returns the installation device id; `20261023071334_project_fk_actions.sql` already rebuilt Project FKs, so recount the fourteen Chat FKs into Projects against it. `RecoveryRestorers` and `ContentAddressedStore` live in `ganbaru-sync-replica`. `DomainAdapter::priority` returns `u8`.

Engine constraints the design must follow (decided at start):

- A derived-key table cannot declare `DuplicateRepair` (`manifest.rs` `derived_key_problems`) and is never tombstoned. The composite-key tables and reply threads drop `DuplicateRepair` and rely on the existence group; the unique on reply thread `root_item_id` is satisfied by keying the thread from its root item.
- An `ApplierVisible` dependency cannot lie on a reference cycle (`manifest/order.rs` `order_problems`). Items and reply threads reference each other, so the item reply thread trigger is `Gated`, not `ApplierVisible`.
- Immutable references must cascade (`manifest.rs` `reference_problems`), and Immutable groups of derived-key tables hold only key columns. `previous_assignment_id` and message `author_participant_id` (SET NULL) move to Register or Coupled groups; reply thread `conversation_id` is a Register group.
- Local queued attachment references are invisible to the shipped asset sweep (`Engine::referenced_assets` walks replicated tables only; the app sweep at `sync/asset_collection.rs` keeps only that set). Step 6 needs a keep set for Local references: an engine-track change (a domain hook returning extra kept digests) or queued attachments counted through a replicated row. This is an engine request for the engine track, not a domain edit.

## Design

### Classification

Replicated is what the person authored or decided. Local is what a device executes, derives, or caches.

| Group | Replicated tables (`TableId`) | Local |
| --- | --- | --- |
| A identity and policy | `chat_participants` 352, `chat_teammates` 353, `chat_teammate_policy_revisions` 354, `chat_access_profiles` 355, `chat_access_profile_revisions` 356 | `chat_teammate_access_state` |
| B conversation shell | `chat_conversations` 360, `chat_conversation_memberships` 361, `chat_teammate_channel_memberships` 362, `chat_teammate_working_folder_grants` 363, `chat_channels` 364 | `chat_conversation_audience_state` |
| C timeline | `chat_reply_threads` 370, `chat_conversation_items` 371, `chat_communication_messages` 372, `chat_communication_message_revisions` 373, `chat_message_references` 374, the five reference target tables 375 to 379, `chat_conversation_read_cursors` 380, `chat_reply_thread_read_cursors` 381, `chat_communication_attachment_references` 382 (owned by 373) | FTS, `chat_attachments`, `chat_queued_attachment_references`, `chat_drafts` |
| D work | `chat_scheduled_messages` 390, `chat_scheduled_message_attachment_references` 391 (owned by 390), `chat_scheduled_message_references` 392 (owned by 390), `chat_work_assignments` 395, `chat_work_assignment_inputs` 396, `chat_work_semantic_updates` 397 | New `chat_scheduled_message_dispatch` |

Every other Chat table is Local: execution environments, worktrees, threads, events, pending requests, review comments, chat messages, authorization revisions, revocation jobs, channel and folder sources, host tool invocations, returned revisions, context packages and sources, dispatch jobs, scratch scopes, generations, cleanup jobs, generation sources, promotions, agent runs, resources, browser artifacts, resource thread references, cleanup jobs, organizational command receipts, preview tool invocations, and terminal attachment contexts.

Decisions behind the less obvious rows:

- Drafts are Local and not synced. A draft is typing in progress on one device; syncing it would create conflicts with no value, and the `docs/data/sync.md` draft sentence changes.
- Review comments are Local because they anchor to Local checkpoints and worktrees.
- `chat_communication_message_revision_ordinals` is dropped, FTS is rebuilt by the hook, and audience and access state become device-local counters bumped by the hook.
- Execution environments are Local. M6 derives `current-folder:{folder_id}` per device, and worktree and scratch environments are paths. Mentions of an environment keep an unchecked id plus their label snapshot.

### Identifiers

- New `ganbaru_chat::ids::new_id(prefix) -> String` returns `{prefix}:{32 lowercase hex}` from 16 bytes of `fill_random`, replacing all three copies. The frontend keeps `crypto.randomUUID()` for the ids it mints, including teammate and channel ids in their UUID form.
- New `ganbaru_chat::ids::derived_id(prefix, parts: &[&str])` is a thin wrapper over `ganbaru_sync_contracts::derived_key` (`{prefix}:` plus the first 32 hex digits of SHA-256 over the NUL-joined parts), for rows whose identity is content: reply threads from `root_item_id`, inputs from `message_item_id`, scheduled receipts from the scheduled id, and the `sync_key` of composite-key tables. `ganbaru-chat` gains a dependency on `ganbaru-sync-contracts`.
- Organizational id newtypes validate 1 to 128 bytes, so every row key fits the engine bound. Local ids keep their 1024-byte checks. Derived agent ids stay as they are.
- Migrated rows keep their ids. All replicas descend from one database, so existing ids are already equal everywhere.

### Composite keys

Memberships, teammate channel memberships, grants, and both cursors follow milestone 2's composite row key convention (step 4): each gains `sync_key TEXT NOT NULL UNIQUE`, the manifest row key. Every insert, including the owner membership in `ensure_project_chat_defaults`, sets `derived_id(kind, primary key columns)`, so two devices adding the same pair produce the same row; grants need three parts that exceed 128 bytes in plain text, which the hash keeps under the bound. All five tables are `WITHOUT ROWID`, so the step 4 and step 5 rebuilds fill existing rows with the placeholder `'pending:' || ROW_NUMBER() OVER (ORDER BY <primary key columns>)` in the copy instead of `'pending:' || rowid`. The Chat conversion pass in `ganbaru-chat`, called by the app at vault open before milestone 2's `seed_captures`, rewrites every placeholder to the derived value, which is deterministic on every device because all replicas descend from one database, and a startup assertion refuses to open the vault while any placeholder remains. Membership rows that milestone 6 created before this milestone get their `sync_key` from the same pass. The composite primary key stays, with `DuplicateRepair` declared; it is unreachable for derived keys. Each of the five carries its primary key columns in an Immutable identity group (milestone 2 step 4).

### Merge rules

- Teammates and profiles: participants are archive-only; `delete_unused_teammate` is removed. Display name, avatar, and instructions are surfaced registers. `current_policy_revision_id` and the profile `current_revision_id` replace `latest_policy_revision` and `latest_revision`. Each is a silent coupled register whose value carries a copy of the referenced revision's restrictiveness, `{current_policy_revision_id, safety_mode, max_capability}` (and `{current_revision_id, max_capability}` for profiles), because the adapter's `priority` sees only the value (`merge.rs:181-184`). Priority is a scalar score from those fields alone: stricter safety mode first, then lower maximum capability. Concurrent policy edits therefore resolve to the more restrictive one and the other stays in history. Policy and profile revision rows are Immutable with random ids; `UNIQUE (teammate_id, revision)` and `UNIQUE (access_profile_id, revision)` are dropped and display numbers come from `(created_at, id)` order.
- Revocation wins: membership `{removed_at, membership_role}`, grant `{capability, capability_inherits_profile, runtime_approval_policy, revoked_at}`, and channel membership `{read_history, participate, history_boundary, history_from_order_key, approval policies}` are silent coupled registers whose priority is a restrictiveness score computed from the value alone: removed or revoked first, then capability rank inverted (`none` highest, `publish` lowest), then approval policy rank (`ask` highest, `unattended` lowest), then cleared `read_history` and `participate` flags. The more restrictive concurrent value wins. Priority applies only among concurrent versions, so a re-grant made after seeing the revocation still wins. `removed_at` and `revoked_at` are the user-facing lifecycle registers. The five composite-key tables, reply threads, and inputs declare `derived_key` with an existence group (a columnless `Register` with its own group id), so a cascade delete followed by the same pair again works. On reply threads and inputs, whose keys derive from one parent item, the Immutable group holds only that item id and `created_at` is a Register.
- Channels: name and topic are a surfaced register; `archived_at` is a lifecycle register with archive priority. Duplicate names after a merge are allowed; commands refuse a duplicate locally and readers sort ties by id.
- Messages: `{current_revision_id, edited_at}` is a surfaced register, so two concurrent edits keep both revisions and surface a conflict; `deleted_at` is a coupled register with deletion priority. Revisions, items, references, and targets are Immutable.
- Reply threads: `last_activity_at` is `Max`; `reply_count` is dropped and readers count with the timeline index. Conversation `last_activity_at` is `Max`.
- Cursors: `{last_read_order_key, last_read_item_id}` is one `Max` group; `''` replaces ordinal 0.
- Assignments: `{state, state_reason, settled_at}` is a coupled register with priority settled (completed, failed, cancelled) above `ready_for_review` above active states. Inputs carry a routing register `{assignment_id, routing_kind}` and a delivery register `{delivery_state, delivered_at}`.
- Scheduled messages: content register `{request_data, scheduled_for}` with owned references and attachment refs, and a failure register `{failed_at, last_error}` written only by the executing device.
- Integer `revision` columns on mutable rows stay as Local optimistic-concurrency counters, bumped by a `SameRow` trigger on apply.

Unique indexes after conversion:

| Index | Resolution |
| --- | --- |
| Teammate display name, custom profile name, channel name, default grant, one active assignment, item ordinals, revision numbers, input ordinals | Dropped; commands refuse locally, readers choose the smallest key (or the newest active assignment) |
| Default channel per project, builtin profile key, reply thread `root_item_id`, composite primary keys, `client_command_id` | Kept with `DuplicateRepair`; derived or seeded ids make them unreachable |
| Local user participant | Replaced by `CHECK (participant_kind != 'local_user' OR id = 'participant:local-owner')`, the id seeded by the baseline (lines 526 and 6473) |

### References

| Column | Action |
| --- | --- |
| Policy and profile revisions, memberships, channel memberships, grants, channels, reply threads, items, messages, revisions, references, targets, cursors, inputs, semantic updates, assignment `teammate_id` and `triggering_message_item_id` | `Reference` with `CASCADE`; a key column is a declared reference column of its `Immutable` identity group (milestone 2 step 3) |
| Message `author_participant_id` (made nullable), `previous_assignment_id` | `SET NULL`; display falls back to `author_label_snapshot` |
| Channel membership `access_profile_id` | `Reference` with `SET NULL`, read as the builtin default profile |
| Mention targets (participant, channel, conversations, folder, workspace path folder, environment, revision cutoff) | FK dropped; unchecked Immutable values that render the label snapshot when missing |
| `current_policy_revision_id`, `current_revision_id` | Register values without an FK; readers fall back to the newest revision by `(created_at, id)` |
| Scheduled message environment | FK dropped (environments are Local) |

Every Local table that references a converted table is rebuilt with `CASCADE`, or `SET NULL` when the column is nullable. Step 2 generates the list with `pragma_foreign_key_list` over Local tables whose parent is in the table above and whose action is `NO ACTION` or `RESTRICT`. Agent runs, dispatch jobs, and context sources cascade from assignments and items.

### Triggers

| Trigger | Policy |
| --- | --- |
| Channel conversation kind, item reply thread, scheduled reply thread, channel membership identity | `ApplierVisible`; parent-first apply satisfies them |
| Reply root existence (5936), current revision existence (5980), grant identity (5883) | `Gated`; the command checks the same rule; cross-row cycles make them unenforceable during apply |
| Default name and default protection | `Gated` |
| Publish `latest_*` (5718, 5789), revision ordinals (6019), author snapshot immutability | Dropped; Immutable groups and registers replace them |
| Access state create (5665), audience create and bumps (5765, 5812 to 5864), FTS (5991, 5997) | `Gated`; the Chat `MaterializeHook` does the same work for applied rows |
| Policy identity (5672), revision immutability (5684, 5712, 5800, 5806, 6026), builtin profile protection (5735 rewritten for `current_revision_id`, 5778) | `ApplierVisible`; participants and profiles are archive-only once `delete_unused_teammate` is removed, so 5684 becomes an unconditional refusal and no apply or cascade deletes or updates a revision row: tables 352 to 356 reference only each other, nothing tombstones them, and their unique keys are seeded or derived, so cascade and duplicate hiding never reach them; seeded builtin profiles materialize with equal values |
| Revision monotonicity (5728, 5758, 5901) | `ApplierVisible`; they guard Local counters that only commands and the hook bump. 5751 is dropped with `latest_revision` |
| Mutable-row `revision` bumps | `SameRow` |

The Chat hook is `ganbaru_chat::sync_effects::after_materialize(conn, table, changed_keys)`, plain SQL with no `ganbaru-sync` dependency, registered by `ganbaru-tauri-app` as the Chat `MaterializeHook`; it receives the connection and milestone 2's change sink, never an `AppHandle`, so milestone 3's headless Android pass can drive it. It runs in the engine transaction and, for changed keys only: rebuilds FTS rows, creates access state, bumps audience revisions, enqueues `chat_access_revocation_jobs` when a membership, grant, policy, or profile change narrowed access, upserts a Local `chat_attachments` row for each new attachment digest, and marks pending inputs and claims for this device's dispatcher. It reports revocation and dispatch work through the change sink; after commit, the app's sink wakes the existing revocation loop and dispatcher through the same notifiers commands use.

### Timeline order

- Items gain an Immutable `order_key TEXT NOT NULL` set by `key_after(last key in scope)`, where the scope is `(conversation_id, reply_thread_id)`. Messages posted offline land after everything their device had seen, and concurrent posts with equal keys order by id.
- Index `(conversation_id, reply_thread_id, order_key, id)` replaces the three unique ordinal constraints and `idx_chat_conversation_items_page`. Every comparison uses row values `(order_key, id) > (?, ?)`.
- `history_from_ordinal` becomes `history_from_order_key`, set to `key_after(current last key)` when access is granted; an item is visible when `order_key >= history_from_order_key`.
- Channel reference ranges become `source_lower_order_key`, `source_lower_item_id`, `source_high_order_key`, `source_high_item_id` (both pairs null for an empty range). `source_revision_cutoff_id` stays and is compared through `(created_at, id)` of the cutoff revision.
- `destination_audience_revision` leaves the replicated target. The executing device records the audience revision it observed in its Local context sources.
- Revision display numbers are derived from `(created_at, id)` per message.

### Message size

- `normalized_markdown` is bounded to 131072 bytes (CHECK on `length(CAST(normalized_markdown AS BLOB))`), matching `MAX_MESSAGE_BYTES`. `rich_content_data` is bounded to 192 KiB with a new `RichContentTooLarge` validation error, below the 256 KiB text field bound. A revision row therefore stays under 330 KiB and always fits one operation.
- Agent projection sites (`projection.rs:295-365`, `agent_runs.rs:203`) truncate at a character boundary with a localized elision marker; the full text stays in Local events.

### Attachments

- Chat declares `AssetClass { name: "chat/attachments", max_bytes: 50 MiB }`; the engine names every file `{sha256}.{ext}`, with `.bin` when the type is unknown. The command limit and the SQL CHECK keep 50 MiB, which fits milestone 2's 64 MiB streamed transfers.
- New files are written to the class path with `ganbaru-atomic-files`. The Chat conversion pass renames existing files to the class path (one file per digest) and rewrites `managed_relative_path` (digest paths pass the `20261021012432` shape triggers, which stay on the Local tables as `ApplierVisible`), so the engine's `ContentAddressedStore` serves every attachment.
- Attachment references carry `{sha256, mime_type, byte_size, kind, original_display_name}` by value. Zero-byte attachments carry no asset reference.
- Chat never deletes attachment files. The Chat adapter's `asset_refs` reports the digests in both reference tables and in the Local queued references, so milestone 10's sweep keeps a file while any live row or stored version names it. Local cleanup removes a `chat_attachments` row only when no reference names its digest.

### Assignments and execution

- Teammates gain a `home_device_id` register (nullable), set by the teammate editor on a device that has the teammate's provider instance ("Runs on this device"), defaulting to the creating desktop.
- Assignments gain an Immutable `executing_device_id`. Posting a mention creates the assignment in the posting transaction with `executing_device_id = home_device_id`; when that is null, with the posting device if it can execute; otherwise the post records the mention only and capable desktops show a Start action that creates the assignment for themselves.
- Only the device whose stable device id (milestone 7's accessor) equals `executing_device_id` dispatches, writes active states, delivers inputs, and answers approvals. Any device may write settled states; the executing device's hook stops a run whose assignment settled remotely.
- Inputs get `derived_id("assignment-input", [message_item_id])`. A follow-up posted on any device routes to the newest active assignment of the reply thread, and the executing device delivers it. `recover_stranded_follow_up_assignments` updates the routing register instead of deleting and reinserting.
- `#[cfg(desktop)]` stays the guard for execution; Android never dispatches assignments.

### Scheduled messages

- `executing_device_id` is the creating device. Claims, attempts, `available_at`, and claim tokens move to a Local `chat_scheduled_message_dispatch` table keyed by scheduled id.
- Sending posts the item under `derived_id("item", ["scheduled", id])` and deletes the scheduled row in the same transaction, so a duplicate send merges into one message. An edit that races the send becomes a recovery entry on the editing device.
- A device that is not the executor shows the message as scheduled and never dispatches it.

### Configuration

| `ChatVaultConfig` field | Destination |
| --- | --- |
| `schema_version`, `unknown_fields` | Dropped |
| `behavior` | `vault_preferences` portable keys `chat.behavior.*` |
| `providers`, `automatic_provider_setup_disabled`, `remembered_selections`, `working_folder_provider_preferences`, `panels` | The device preference file under `chat` |

Step 8 moves the values on first open and deletes the `chat` branch handling. `config.json` deletion follows the README rule (whichever of milestones 7 and 9 empties it last deletes it and its loader). Provider trust, credentials, launch arguments, environment, paths, and homes never replicate.

### Migrations

One migration per conversion step, named `{UTC timestamp}_chat_<topic>.sql`, each with `-- no-transaction`, explicit `BEGIN`, `PRAGMA foreign_keys = OFF`, table rebuilds, a `foreign_key_check` guard that aborts the transaction, and `PRAGMA foreign_keys = ON`. Order keys are filled per scope by `ROW_NUMBER() OVER (PARTITION BY conversation_id, reply_thread_id ORDER BY ordinal, id)` encoded as the milestone 1 rank key (`c` plus three base-62 digits); a scope above `RANK_KEY_CAPACITY` (238,328) aborts with a message asking for a development reset. Each step that changes the manifest bumps `VAULT_MANIFEST_VERSION`.

### Presentation

- Conflicts: message edits (both revisions, choose current), teammate profile and instructions, channel name and topic.
- Recovery entries: edits inside a deleted channel or project, scheduled message edits that raced the send, and assignments settled while another device's input was in flight. Restorers are keyed by table in `RecoveryRestorers` and the frontend renderer registry.
- Strings go in `apps/client/src/lib/i18n/messages/{en,es}/chat*`.

## Testing

- `ganbaru-db`: each migration on a seeded vault and the dense dataset: row counts, order keys preserve ordinal order per scope, cursors and boundaries map to the same visible items, `sync_key` placeholders filled and unique on the `WITHOUT ROWID` rebuilds, `foreign_key_check` empty, every kept index and trigger present by name, and a guard failure rolling back intact.
- `ganbaru-chat`: `new_id` format and length, and `derived_id` equal to `derived_key` for the same parts; the conversion pass turning placeholders on a migrated dense fixture into the values the writers compute for the same rows, a second run changing nothing, and the startup assertion refusing a vault with a remaining placeholder; paging, unread counts, history boundaries, and channel reference ranges over order keys with equal-key ties; revision display numbers; rich content bound; projection truncation at a multibyte boundary; attachment write, digest naming, the conversion rename of existing files, and `asset_refs` covering queued references; input rerouting without deletes; scheduled send with a derived receipt id.
- `ganbaru-tauri-app`: executing-device gates for dispatch, approvals, and scheduled sends; home device selection; the hook enqueuing revocation jobs and stopping a remotely settled run; config split on first open.
- `ganbaru-sync`: conformance for tables 352 to 397; codec round trips; simulation seeds for concurrent posts in one scope, concurrent edits of one message, revoke against widen on a grant and a membership, concurrent policy edits of different safety, channel delete against post, assignment cancel against active progress, duplicate reply thread start on one root, and attachment asset transfer, including one above 32 MiB.
- Frontend: message list ordering with equal keys, revision chooser, Start action on mentions without an assignment, conflict and recovery previews.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-chat-random-ids` | `ids` module, the three generators replaced, id newtypes bounded to 128 bytes, the `ganbaru-sync-contracts` dependency and `derived_id` over `derived_key` for reply threads and inputs. | `cargo test -p ganbaru-chat --lib -j 4 ids -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 chat -- --test-threads=4` |
| 2 | `feat/sync-chat-local-tables` | FK rebuilds of Local tables into tables that will convert, trigger bodies gated on `sync_apply_state.applying`, `sync_effects::after_materialize` as a tested function, Local scheduled dispatch table, drop of `delete_unused_teammate` and the channel membership hard delete. | `cargo test -p ganbaru-db --lib -j 4 chat -- --test-threads=4`, `cargo test -p ganbaru-chat --lib -j 4 sync_effects -- --test-threads=4` |
| 3 | `feat/sync-chat-identity` | Local class declarations, trigger policies, and hook registration (milestone 2 APIs); group A manifest (352 to 356), `current_*_id` registers with restrictive priority, revision display numbers, `home_device_id`. | `cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4`, `cargo test -p ganbaru-chat --lib -j 4 teammate -- --test-threads=4` |
| 4 | `feat/sync-chat-conversations` | Group B manifest (360 to 364), `sync_key` on memberships and grants with every insert (including `ensure_project_chat_defaults`) setting it, the Chat conversion pass and startup placeholder assertion, revocation-wins priorities, unique index changes, access comparators (`access/channels.rs`, `access.rs:324`, `internal_mcp_tools/authorization.rs`). | `cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 access -- --test-threads=4` |
| 5 | `feat/sync-chat-timeline` | Group C manifest (370 to 381), order keys and every listed comparator, cursors with `sync_key` through the same conversion pass, reply thread changes, size bounds, projection truncation, FTS through the hook. | `cargo test -p ganbaru-db --lib -j 4 chat -- --test-threads=4`, `cargo test -p ganbaru-chat --lib -j 4 order_key -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 reads -- --test-threads=4` |
| 6 | `feat/sync-chat-attachments` | Asset class at the 50 MiB limit, digest naming, conversion rename of existing files, owned attachment references (382, 391), `asset_refs` over reference and queued rows, file deletion removed from Chat cleanup. | `cargo test -p ganbaru-chat --lib -j 4 attachment -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4` |
| 7 | `feat/sync-chat-assignments` | Group D manifest (390, 392, 395 to 397), executing device gates, mention routing and Start action, input rerouting, scheduled receipts. | `cargo test -p ganbaru-chat --lib -j 4 assignment -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 scheduled -- --test-threads=4` |
| 8 | `feat/sync-chat-config` | Config split into `vault_preferences` and the device file, removal of the `chat` branch, and `config.json` deletion under the README rule. | `cargo test -p ganbaru-tauri-app --lib -j 4 vault_config -- --test-threads=4` |
| 9 | `feat/sync-chat-presentation` | Conflict renderers, recovery restorers, i18n, Chat replica write flag, frontend gates removed. | `pnpm --dir apps/client exec vitest run src/lib/chat --maxWorkers=1`, `pnpm --dir apps/client run check` |
| 10 | `feat/sync-chat-docs` | Docs, acceptance results, goal README status. | `pnpm -w run validate` |

Steps 3 to 7 rebase in order because each bumps `VAULT_MANIFEST_VERSION`. Chat writes stay owner-only until step 9.

## Documentation

- `docs/data/schema/chat.md:98-104`: classification table, order keys, `sync_key`, dropped ordinal and publish machinery, attachment naming; replace "Remote synchronization is not implemented" (:104).
- `docs/data/sync.md:176`: the Chat row replicates organizational content and assignments; device-local adds drafts, review history, execution, and provider configuration. Line 180 drops the origin-attributed drafts sentence. Line 243 moves Chat to implemented.
- `docs/data/security/chat.md:23,27`: provider configuration and runtime values are device-local, not portable; :100 attachment naming; a new paragraph for revocation wins and the executing device.
- `docs/algorithms/sync/README.md`: a worked example of revocation priority, and order keys for append-only timelines.
- `docs/features/chat/README.md:39`, `conversations.md` (offline posting, duplicate channel names, edit conflicts), `agents-and-coordination.md` (home device, Start action, settled states from any device), `execution-and-workspace.md` (execution and approvals stay on the executing device).
- `docs/features/README.md` Chat row and the `AGENTS.md` sync sentence.

## Manual acceptance

Two desktops (A coordinator, B linked, both with Codex installed) and one Android phone (P). Offline edits are made with the network disabled, then reconnected.

| Scenario | Expected |
| --- | --- |
| A, B, and P each post two messages in one channel offline | Same order on all three; each device's messages stay in its own posting order. |
| A and B edit one message offline | Conflict on all three; both revisions in history; the chosen one shows everywhere. |
| P mentions a teammate whose home device is A | Assignment runs on A only; B and P show progress. |
| B creates a teammate without setting a home device, P mentions it | No run starts; A and B show Start; Start on B runs it on B only. |
| A widens a grant to edit while B revokes it offline | Revoked everywhere; A stops a running assignment that used it. |
| A schedules a message, B edits its text before it is due | A sends B's text once; no duplicate on any device. |
| B cancels an assignment running on A | A stops the run; state cancelled everywhere. |
| P attaches a 5 MiB image | Bytes reach A and B; preview opens on both. |
| B attaches a 45 MiB file | Bytes reach A and P; the file opens on both. |
| A deletes a channel while P posts in it offline | Channel gone; recovery entry on P restores the post. |
| Read up to a message on P | A and B show the same unread position. |
| Approval request from a run on A | Visible on B and P as waiting; only A can answer. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Conversion migrations on the dense dataset | Under 10 s desktop, under 30 s Android |
| Timeline page of 100 items over order keys | Within 10% of the ordinal query |
| Apply of a 256-operation Chat batch including the hook | Under 80 ms desktop, under 200 ms Android |

## Risks

| Risk | Mitigation |
| --- | --- |
| Two devices execute one assignment | Immutable `executing_device_id`, dispatch gate tested, settled priority stops stragglers. |
| A missed comparator mixes ordinals and keys | The ordinal columns are dropped, so a missed site fails to compile or query; the Current state list is the checklist. |
| Clock skew misorders revision cutoffs across devices | Cutoffs only bound shared history; deterministic everywhere; accepted for one person's devices. |
| Revocation reaches a running agent late | The hook enqueues revocation jobs in the apply transaction; runs on the executing device re-check authorization per tool call. |
| Rebuilds lose an index or trigger | Schema-by-name comparison and row counts; guard rolls back. |
| A phone mention with no home device never runs | Start action on capable desktops; teammate editor prompts for a home device. |

## Deferred

- Approvals, user-input answers, and run control from devices other than the executor.
- Draft sync and review comment sync.
- Moving an assignment to another device.
- Removal of owner-only gates outside Chat ([milestone 10](milestone_10.md)).

## Implementation checks

Settled from source:

- Teammate channel membership removal deletes the row today (`organization/access.rs:370`), so every reader relies on deletion: the six joins in `internal_mcp_tools/authorization.rs` (25, 113, 177, 218, 260, 333), `organization/reads.rs:101`, and `organization/access/profiles.rs:133,348`. Step 2 adds `removed_at IS NULL` to each and replaces the delete with the `removed_at` write.
- `chat_agent_runs.assignment_id` is `NOT NULL` with no delete action (B:5335), so a remote assignment tombstone would fail the foreign key during apply. The group D step makes it nullable with `ON DELETE SET NULL`, and the post-apply hook cancels a run whose assignment became null.
- No cap on references per scheduled message exists. The scheduling command caps them at 256, which keeps a row with its owned children far below 448 KiB.
- Hard-delete sites are listed under Current state; none needs a lifecycle register beyond the membership change above and `delete_unused_teammate`'s removal.
- `history_from_ordinal` is inclusive (`scratch.rs:507`, `internal_mcp_tools/authorization.rs:170,202,285` compare `history_from_ordinal <= ordinal`), so an item is visible when its order key is at or after `history_from_order_key`; the migration test asserts identical visibility before and after.

## Progress

- [x] Step 1: random and derived ids.
  - Landed: `ganbaru_chat::ids` with `new_id(prefix)` (16 random bytes from `ganbaru_atomic_files::fill_random`, hex) and `derived_id(prefix, parts)` (via `ganbaru_sync_contracts::derived_key`); the three timestamp and counter `new_id` copies (organization/common.rs, channels.rs, scratch/commands/cleanup.rs) are gone. Organizational newtypes in ganbaru-chat-contracts are bounded 1..128 bytes (`MAX_ORGANIZATIONAL_IDENTIFIER_BYTES`). Reply thread ids derive from `root_item_id`; work assignment input ids derive from `message_item_id` (`reply-thread:` and `assignment-input:` prefixes).
  - Commit: b84bf8b2 `feat(chat): add random and derived organizational ids`.
  - Checks held: ganbaru-chat ids tests; tauri-app chat tests 339 passed; the step 1 commit checked alone (`cargo check -p ganbaru-tauri-app --lib --tests`).
  - Plan-silent decisions: `new_id` returns `ChatResult` (random source failure surfaces as an error instead of a panic). Device-local ids (`ChatCommandId`, `ChatAttachmentId`, provider ids) keep the 1024-byte bound. `recover_stranded_follow_up_assignments` keeps delete and reinsert, now with the derived input id.
  - Notes for step 3: scheduled message receipts still use random ids; their derived id belongs to step 7. Any new organizational insert must use `ganbaru_chat::ids` and stay within 128 bytes.
- [x] Step 2: FK rebuilds, gated triggers, hook function, hard deletes removed.
  - Landed: migration `20261024142304_chat_local_tables.sql` rebuilds 8 Local tables (revocation jobs, authorization revisions, agent runs, authorized channel sources, context packages, host tool returned message revisions, scratch generation sources, scratch scopes) with CASCADE to organizational parents behind a temp `pragma_foreign_key_check` guard; parent-guarded delete triggers deny only while every parent exists. 16 derive or validate triggers are gated on `sync_apply_state.applying = 0`. `chat_teammate_channel_memberships.removed_at` added; removal is an update (`access::remove_teammate_from_conversation`), upsert clears it, readers filter `removed_at IS NULL`. Local `chat_scheduled_message_dispatch` table (no writers yet). `ganbaru_chat::sync_effects::after_materialize(conn, table, changed_keys)` returns `ChatSyncEffects`; tested, not registered. Revocation SQL moved to `ganbaru_chat::coordination::revocation`. `delete_unused_teammate` removed end to end (command, permission, runtime registration, `has_durable_history` contract field, frontend caller, i18n keys).
  - Commit: 0d26a2a4 `feat(chat): cascade local chat rows and gate sync triggers` (also carries rustfmt fixes for step 1 files common.rs and workflow.rs).
  - Checks held: ganbaru-chat lib 155 passed; ganbaru-db lib 136 passed (one older test reseeded, see below); tauri-app lib 1021 passed; clippy workspace all targets; fmt; desktop bin check; client check; focused Vitest (6 files, 29 tests); editor-check; bundle-contracts.
  - Plan-silent decisions: `chat_agent_runs.authorization_revision_id` is CASCADE. Revocation jobs keep SET NULL on the authorization; execution environment, scratch generation, and provider thread references keep their actions. A profile revision that narrows its predecessor (read history, participate, entire to from_grant, folder capability rank) bumps access revisions and revokes all of the teammate's active authorizations, mirroring the command; teammate policy revisions never revoke, mirroring the command path. Audience bumps repeat on replay (counters). Composite-key tables are a hook no-op until step 4 adds `sync_key`. `deleteFailed` i18n key removed; `historyPreserved` kept. `authorization_source_snapshots_are_immutable_and_folder_policy_is_required` now seeds the conversation, revision, and message reference parents because the delete guard stands down when any parent is missing.
  - Review fix round (after merging feat/sync af3b1709 in a signed merge commit; migration edited in place, still sorts last after 20261024093938):
    - The graph walk test (`replicated_chat_deletes_never_stop_on_a_device_local_foreign_key`) starts from all 29 replicated Chat tables, follows CASCADE edges transitively, and flags any NO ACTION or RESTRICT edge from a Local child, plus any CASCADE from a Local parent into a replicated child.
    - `chat_threads.execution_environment_id` and `scratch_generation_id`: CASCADE (thread rebuilt with its 6 indexes and 5 triggers). SET NULL is not viable because no CHECK clause accepts a scratch thread with one of the two nulled, and `chat_threads_execution_target_update` aborts on it. Folder-bound threads already cascade with their folder.
    - `chat_agent_runs.execution_environment_id` and `scratch_generation_id`: CASCADE, for the same CHECK-shape reason.
    - `chat_agent_runs.provider_thread_id`: SET NULL changed to CASCADE. `chat_agent_run_authority_immutable` lists `provider_thread_id`, so SET NULL aborts whenever the thread goes before the run. The run's turn belongs to that thread and already cascades it, so the outcome is unchanged and no longer depends on order.
    - `chat_assignment_authorization_revisions.execution_environment_id`: CASCADE. The delete guard now also stands down when the environment is missing.
    - `chat_execution_environment_reference_targets.execution_environment_id` and `chat_scheduled_message_references.execution_environment_id` (both replicated): the FK is dropped and the id stays unchecked (NOT NULL and the kind CHECK are kept), following the plan's mention and scheduled environment rows. These are more specific than the generic SET NULL rule, and SET NULL would break the scheduled reference CHECK. Readers inner-join the environment, so a missing one drops out and the label snapshot renders.
    - `chat_restore_operations.checkpoint_id` NO ACTION is pre-existing and left as is. It is listed in the test as a same-root edge: by construction the operation and its checkpoint share a thread (`chat_preview_checkpoint_restore` rejects a checkpoint whose `thread_id` differs, and the restore saga requires the preview's thread to match the request), so both cascade from that thread, and NO ACTION is checked at the end of the statement.
    - `chat_communication_search_delete` is ungated again (baseline body). 15 gated triggers remain.
    - Fixture `project_chat_graph.sql` gains a scratch environment, an environment mention, a scheduled environment mention, a scratch authorization revision, a scratch thread, a turn, and a run. Tests cover a conversation delete, a reply thread delete (scratch chain removed, mentions keep the unchecked id, `foreign_key_check` empty), and FTS cleanup for a message deleted while applying. `project_fk_actions` now counts only the folder-bound authorization.
    - The rollback test keeps the manual ROLLBACK, because the no-transaction migration owns its BEGIN on the in-memory pool's only connection (comment added). The stray blank line in `ChatTeammatesSettings.svelte` is removed.
    - The `sync_effects` search test now expects the hook to find nothing to remove after a delete, because the ungated trigger already removed the FTS row.
    - Commits: 75caf667 `merge: feat/sync into feat/sync-chat-ids-local-tables`, 56f4ee0e `fix(chat): let scratch deletes cascade and keep search cleanup during apply` (both signed).
    - Checks held: ganbaru-chat lib 155, ganbaru-db lib 140, tauri-app lib 1022, workspace clippy with all targets, fmt, desktop bin check, client check, focused Vitest (20 files, 176 tests), editor-check, and bundle-contracts.
  - Note for step 4: `sync_effects::after_materialize` must handle memberships, grants, and teammate policy, whose audience and access triggers are already gated.
  - Notes for step 3: register `after_materialize` from the replica hook and wake readers from `ChatSyncEffects`. `recover_stranded_follow_up_assignments` still deletes and reinserts inputs; its reroute belongs to step 7, as do dispatch table writers and scheduled receipt derived ids. Attachment upsert is step 5.
- [x] Step 3: Local class, trigger policies, hook registration, identity and policy
  - Date: 2026-10-24.
  - Branch: feat/sync-chat-identity (worktree agent-afe2907c08490bff7), head 72ebf0f6.
  - Commits (all signed): 6a32e0e1 feat(sync): replicate chat identity and policy; ffa7c3bb refactor(chat): read effective revisions by pointer; 76ae7ecd feat(sync): register chat materialize hook and wake revocation; bd8652f3 merge: feat/sync into feat/sync-chat-identity (feat/sync at f900d1c2, includes 3258767c); 72ebf0f6 docs(chat): state current chat replication boundary (docs only, after the gates).
  - AGENTS.md and the sync overview domain lists do not mention Chat yet; left for step 4, since group A is a partial domain.
  - Manifest: group A tables 352 to 356, VAULT_MANIFEST_VERSION = 13 (still free after the merge). Migration 20261025015209_chat_identity_replication.sql still sorts last.
  - Checks on bd8652f3 (post-merge): ganbaru-sync lib 362 passed, 7 ignored (505.72s, includes conformance); ganbaru-chat lib 155 passed (includes teammate filter); ganbaru-db lib 149 passed (810.23s); ganbaru-tauri-app lib 1030 passed (296.28s); clippy workspace all-targets -D warnings clean; fmt --check clean; cargo check -p ganbaru --bin ganbaru clean; Android cargo check (aarch64-linux-android, seven packages) exit 0 with only the five baseline warnings. Frontend unchanged, so frontend checks skipped.
  - Android check note: the shared ~/.cache/ganbaru-targets/android dir reused a stale ganbaru-sync from another worktree (cargo dep-info paths are workspace-relative), giving a false "no chat in manifest::vault" error. Run with a per-branch CARGO_TARGET_DIR (used ~/.cache/ganbaru-targets/chat-android).
  - Plan-silent decisions: unique name indexes on teammates and custom profiles dropped (cannot hold across devices); local user identity kept by trigger chat_participants_local_user_identity; commands check names per device (require_unused_teammate_name, require_unused_access_profile_name) and readers disambiguate equal names by id. Revision display numbers count rows by (created_at, id). Hook is registered for teammates, profiles, and profile revisions through chat::sync::with_hooks in sync/host.rs vault_hooks; it takes only the connection and change sink and reports a chat-access-revoked notice per revoked teammate. On desktop a managed RevocationWaker (holding the AppHandle) wakes chat::revocation::start_job_drain (renamed from start_startup_recovery) from refresh_cached. ganbaru-db migration tests share a shape-detecting project chat graph loader.
  - Notes for step 4: on Android the hook runs headless and queues revocation jobs, but nothing drains them (ganbaru-chat native runtime is desktop-only); decide whether Android needs a drain or the jobs wait for a desktop. The dispatcher reads effective rows at dispatch time, so arriving policy needs no extra wake.
- [ ] Step 4: conversation shell and revocation priorities
- [ ] Step 5: timeline order keys, comparators, size bounds
- [ ] Step 6: attachments as an asset class
- [ ] Step 7: assignments and scheduled messages
- [ ] Step 8: config split
- [ ] Step 9: presentation and replica writes
- [ ] Step 10: docs and physical acceptance
