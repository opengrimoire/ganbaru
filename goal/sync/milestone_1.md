# Milestone 1: sync foundation and Quick notes

**Status: Implemented, physical acceptance pending.** Committed on `feat/sync` (2026-10-09) after the full validation gate passed; only step 8 remains. Part of the [concurrent sync goal](README.md). This file is the implementation plan for the first milestone: the engine, the operation format, writers, mixed mode, the LAN hub transport, and Quick notes as the first converted domain. [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract; each pull request moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- Quick notes and their tags are editable on every linked device, online or offline. A change reaches the other connected devices on the same LAN through the coordinator desktop in about two seconds.
- Concurrent changes converge to identical state on every device. Concurrent title or body edits show a conflict with both versions and a resolution; color, tag, lifecycle, and order resolve silently and deterministically.
- Deleting a note while another device edits it produces a recovery entry instead of losing the edit.
- Restoring a backup, reinstalling, or cloning a disk never reuses a writer sequence and never loses or duplicates changes.
- The rest of the vault keeps single-writer whole-vault handoff (mixed mode), with no lost Quick notes changes across refresh or ownership transfer.
- The engine, format, and test harness are the foundation every later milestone extends.

## Scope

In scope:

- New crates `ganbaru-sync-contracts` and `ganbaru-sync`.
- Engine metadata migration and the Quick notes replication migration.
- Merge kinds: immutable, register (surfaced and silent), coupled register with a priority resolver, max, position, reference with redirects, and owned children.
- Installation-scoped writers certified by the person key, high-water checks, fork handling, and revocation cutoff on device removal.
- Guarded replica database access and carry-forward through whole-vault refresh and transfer.
- Protocol 5 sync messages on the existing mutual TLS listener, with the coordinator desktop as hub.
- Quick notes changes: order keys, tag rename and delete, trash purge as a job, conflict and recovery UI, and a three-way editor rebase.
- Settings sync section with status, pause, sync now, and recovery entries.
- Golden vectors, mutation tests, the deterministic simulation harness, crash-boundary tests, and manifest conformance tests.

Out of scope (later milestones in the [goal README](README.md#milestones)):

- Any other domain, and preferences in `config.json`.
- Membership, counter, tree, and text merge kinds.
- Operation encryption, the relay, Android background sync, coordinator address refresh, and discovery.
- Causal stability, compaction, and value elision.
- Multi-person spaces.
- Quick notes bodies as Yrs text (moved to the multi-person sharing goal).
- Cross-process vault fencing.

## Prerequisites

- The people slice on `feat/contacts` is merged into `dev`. It provides the person identity row `contacts_local_identity`, the `ganbaru-contacts` crate (`PersonKeyPair`, `PersonPublicKey`, signature verification), person key release to linked devices over mutual TLS (`PersonKeyRequest`), and protocol version 4.
- Each pull request starts from current `dev` on its own topic branch (see [work breakdown](#work-breakdown)).
- No new external dependencies. `ring`, `sha2`, `base64`, `keyring`, `sqlx`, and `uuid` are already in the workspace. Two new workspace crates change `Cargo.lock`, so those pull requests run the full audit gate.

## Current state

Facts this design builds on, verified in source:

- **Single writer.** `VaultOwnershipManager::database_access` returns `ReadWrite` only for the stable owner; every other device opens `ReadOnly` with `PRAGMA query_only`. `require_writable` is used only by People and backups; Quick notes rely on the read-only connection to fail. The frontend validates `canWrite === (role === "owner")`.
- **Pool registry** (`crates/ganbaru-db/src/lib.rs`). One pool per path, `max_connections(1)`, WAL, `synchronous(Full)`, foreign keys on, 5 s busy timeout. Opening an open path in another access mode fails with "database access changed; close the existing pool first". `validate_current_schema` checks `_sqlx_migrations` against the embedded set.
- **Read-only caller.** `connect_active_vault_read_only` has one caller, `pomodoro/native_runtime/calendar_edit.rs:265`.
- **Handoff.** The coordinator desktop listens for pinned mutual TLS 1.3 connections. `PROTOCOL_VERSION` is 4. Control messages are serde JSON frames of at most `MAX_CONTROL_BYTES` (1 MiB), one request per connection, gated by `verify_authenticated_peer`. Replicas refresh on start, resume, and reconnect. Activation on desktop and Android converges on `activate_handoff_at_path` in `vault/backup.rs`, which runs under `begin_snapshot_quiescence`. The source prepares through `prepare_local_state_for_snapshot` in `vault/handoff/source.rs`.
- **Quick notes schema** (baseline). `quick_note_tags` has `UNIQUE (name)` with `NOCASE`, `UNIQUE (sort_order)`, and `sort_order < 9`. `quick_notes` has `tag_id ... ON DELETE SET NULL`, `manual_order REAL`, `revision`, and `CHECK (pinned = 0 OR (archived = 0 AND trashed_at IS NULL))`; `idx_quick_notes_active` and `idx_quick_notes_tag_active` are the only references to `manual_order`. `quick_note_text_runs` references notes `ON DELETE CASCADE`. `quick_notes_search_fts` is maintained by three triggers.
- **Quick notes backend** (`src-tauri/app/src/quick_notes.rs`). `manual_order` is written by create (:555), `quick_notes_set_pinned` (:670), `reorder_from_pool` (:718, which rebalances the whole group), unarchive (:868), and restore (:913); `QuickNotesCursor` (:110) and the cursor predicate (:463) page by it. `cleanup_expired_trash` (:374) hard-deletes on the list (:425) and load (:550) read paths. `quick_notes/tags.rs` has only list and create, which picks the lowest free `sort_order` slot below `MAX_TAGS` (9).
- **Quick notes frontend.** `QUICK_NOTE_TAG_LIMIT` lives in `lib/quick-notes/types.ts:5`; `mapQuickNoteTag` rejects `sortOrder >= 9` (`lib/api/quick-notes.ts:171`); the panel sorts tags by `sortOrder` (`QuickNotesPanel.svelte:198`) and resolves shortcuts by list position (:430). Windows stay in step through `publishQuickNotesChanged` and `listenForQuickNotesChanges` in `lib/quick-notes/window-sync.ts`. The editor uses client UUIDs, autosave with `expectedRevision`, and a reload-or-keep-a-copy conflict state in `QuickNoteEditorModal.svelte`.

## Design

### Crates and modules

`crates/ganbaru-sync-contracts` has no database or Tauri dependency. It depends on `ring`, `sha2`, and `ganbaru-contacts`.

- `ids`: `SpaceId` and `WriterId` (16 bytes), `Seq` (u64, from 1), `OpId`, `RowKey` (UTF-8, 1 to 128 bytes, not only spaces and without NUL, matching the Quick notes id rule), `TableId` (u16), `GroupId` (0 to 63), and `GroupMask` (u64).
- `hlc`: hybrid logical clock.
- `vector`: version vectors and coverage checks.
- `codec`: bounded big-endian reader and writer.
- `op`: envelope, header, body, change, value, encode and decode.
- `signing`: domain-separated signing and hashing.
- `certificate`: writer certificates, verified through `ganbaru-contacts` instead of a second Ed25519 path for person signatures.
- `order_key`: the `OrderKey` newtype with `key_before`, `key_after`, and `key_between`.
- `bounds`: every limit as a typed constant.

`crates/ganbaru-sync` is the engine. It depends on the contracts crate and SQLx, with `ganbaru-db` as a dev-dependency for schema tests.

- `manifest`: the model (classification, tables, groups, merge kinds, declared resolutions) and the concrete vault manifest in `manifest/vault.rs` and `manifest/vault/quick_notes.rs`, including the classification of every table in the schema.
- `capture`: capture trigger SQL rendered from the manifest.
- `guards`: guard set computation and TEMP trigger SQL.
- `log` (writer rows, vectors, operation pages, and the context cache), `seal`, `store`, `apply` (readiness, batches, and held re-evaluation), `merge` (merge state per kind), `materialize` (winners into domain rows), `repair` (duplicate tag repairs), `conflicts`, `recovery`, `reseal`, `status`, `validate`, and `sql` (engine write markers).
- `adapter`: the `DomainAdapter` trait (composite value codecs, intrinsic validation, derived projections, recovery presentation).
- `domains/quick_notes.rs`: the Quick notes adapter (run list codec, plain text projection, bounds). It lives in the engine crate for this milestone so the simulation harness merges real Quick notes values; the app calls the same projection instead of keeping its own copy. Later domains may move adapters into their own crates.
- `exchange`: the client and hub state machines over a transport trait, so the harness and the app share them.
- `sim` (test only): the deterministic simulation harness.

The engine refuses to start when the manifest names an adapter that is not registered.

Replica composition in `crates/ganbaru-sync-replica/`, Tauri-free:

- `writer.rs`: writer key storage, the writer record, and the high-water check.
- `access.rs`: role to access mode mapping and connection hooks.
- `hub.rs` and `client.rs`: both sides of the exchange; the client reaches the hub through a transport trait.
- `carry_forward.rs`: export and import around whole-vault activation.
- `recovery.rs`: restoring and discarding recovery offers.

App composition in `apps/client/src-tauri/app/src/sync.rs` and `sync/`: `service.rs` (per-vault lifecycle, debounce, and the exchange loop), `transport.rs` (the paired transport), `recovery.rs` (the recovery listing), `devices.rs`, `status.rs`, `commands.rs`, and `tests.rs`.

`ganbaru-db` gains the `Guarded` access mode and per-connection hooks (see [database access](#database-access-and-mixed-mode)).

### Identifiers and clocks

- `space_id` is the first 16 bytes of SHA-256 over `"ganbaru-sync/personal-space/v1"`, a zero byte, and the vault id.
- `writer_id` is the first 16 bytes of SHA-256 over `"ganbaru-sync/writer/v1"`, a zero byte, and the writer public key.
- An operation is identified by `(space_id, writer_id, seq)`. Sequences are contiguous from 1; sequence 1 is always the writer genesis.
- The hybrid logical clock is a u64: 48 bits of Unix milliseconds and a 16-bit counter. A seal takes the maximum of the physical clock, the last local value plus one, and the adopted remote value plus one; counter overflow advances the physical part.
- Remote clocks are adopted only up to the local physical time plus 60 seconds. An applied operation whose clock is more than 5 minutes ahead of local time raises a clock warning naming the device. Clocks never cause rejection: validation stays a pure function of operation bytes, and supersession is decided by version vectors.

### Writers

- **Key.** Ed25519 through `ring`. Desktop stores it in the operating-system keyring under service `com.ganbaru-ai.sync`, reference `writer-key:{vault_id}:{writer_id}`. Android stores one app-private file per writer under `app_config_dir/sync/`, written with `write_private_file_atomically`; the existing backup and device-transfer exclusions cover it.
- **Record.** `app_config_dir/sync/{vault_id}.json` holds `writerId`, the encoded certificate, `reservedThrough`, `lastCommittedSeq`, `paused`, and `retiredKeys` (retired writers whose keys still wait to be removed), written atomically with fsync. Key store calls can block (keyring unlock), so they run on blocking threads and never while a vault connection is held.
- **Reservation.** Before sealing a sequence above `reservedThrough`, the record advances `reservedThrough` by a block of 256 and syncs. After every seal commit, `lastCommittedSeq` is written. Seals are debounced, so this is a few small writes per minute at most.
- **High-water check.** Runs at service start and after any database replacement, after carry-forward. Let `m` be the highest sequence of the recorded writer in `sync_ops`. The database is genuine when `lastCommittedSeq <= m <= reservedThrough`, the key exists, and the operation at `m` verifies with it. Otherwise the database was restored, copied from another installation, or the key was lost: the old writer is marked retired locally, a new writer is created, and the old key is removed once the new record is durable. A record with `m = 0` and `lastCommittedSeq = 0` is a crash before the genesis committed and stays valid.
- **Creation.** A writer is created at the first seal once the vault is linked and the person key is available. Its genesis operation carries a certificate binding the writer public key, the person public key, the device id, the creation time, and optionally the retired predecessor (informational only), signed by the person key under `"ganbaru-sync/writer-certificate/v1"`. The personal space trust anchor is `contacts_local_identity.public_key`.
- **States** in `sync_writers`: `active`, `retired` (local successor exists), `revoked` (device removed), and `forked`.
- **Missing person key.** Sealing waits, status shows `waiting_for_identity`, and `people::identity::try_release` is requested from the coordinator. The service reads the person key directly from its store and checks it against the space anchor, so sealing under quiescence never waits on the People pool.

### Operation format

Version 1 is a hand-written, bounded, big-endian encoding with golden vectors. Field order is fixed; there are no optional encodings of the same value.

Envelope, stable across format versions so signatures verify even when the payload is from a newer format:

| Field | Size |
| --- | --- |
| Magic `GBSO` | 4 |
| Format version | 1 |
| Kind | 1 |
| Space id | 16 |
| Writer id | 16 |
| Sequence | 8 |
| Previous operation hash | 32 (zero for the genesis) |
| Header length and header | 4 + n |
| Body length and body | 4 + n |
| Ed25519 signature | 64 |

- The signature covers `"ganbaru-sync/op/v1"` followed by every envelope byte before the body length. The body is excluded, so a later compaction can drop superseded values without breaking verification.
- The operation hash is SHA-256 over `"ganbaru-sync/op-hash/v1"` and the signed bytes. It forms each writer's hash chain.
- Header: hybrid logical clock, manifest version (1), authorization revision (0), key epoch (0), dependency delta (writers whose applied sequence advanced since this writer's previous operation, as `(writer_id, seq)` pairs), and the change index.
- Change: table id, row key, action (`create`, `write`, `tombstone`), group mask, a 32-byte value hash per set group, and for tombstones an optional `replaced_by` row key.
- Body: for each change and set group, the group value as a field count followed by typed fields (`null`, `i64`, `text`, `blob`). Composite values such as the Quick notes run list are a single `blob` produced by the domain adapter's canonical codec.
- Value hash: SHA-256 over `"ganbaru-sync/value/v1"`, the table id, the group id, and the encoded value.
- Kinds: `0` changes, `1` writer genesis, `2` writer revoke (target writer, cutoff sequence, reason `revoked` or `forked`), `3` reserved (milestone 10 makes it a signed acknowledgement). Genesis and revoke payloads live in the signed header and their body is empty, so compaction can never drop a certificate or a cutoff. Sequence 1, the genesis kind, and a zero previous hash imply each other; a writer cannot revoke itself, and a tombstone cannot redirect to its own row.
- Every domain-separated hash and signature input is the domain string, a zero byte, and the payload. Writer keys sign operations only.

Bounds, all derived from the 1 MiB control frame:

| Bound | Value | Reason |
| --- | --- | --- |
| Operation | 512 KiB | About 683 KiB as base64, inside one frame with headroom. |
| Header | 64 KiB | Change index for the largest batch; headers outlive compacted bodies, so sealers split on header size as well as on change count and operation size. |
| Changes per operation | 1,024 | Bounded apply transactions. |
| Dependency entries | 256 | Writers per space in practice are few. |
| Row key | 1 to 128 bytes | Matches the existing id validation. |
| Text field | 256 KiB | A 65,536-character note body is at most 256 KiB of UTF-8. |
| Blob field | 320 KiB | The largest text plus composite structure such as a run list. |
| Fields per group value | 16 | Coupled groups stay small. |

The largest Quick note (200-character title, 65,536-character body, 4,096 runs) encodes to about 283 KB, so one row always fits in one operation and splitting happens only at row boundaries.

Validation is intrinsic and pure: envelope and bounds, signature against the writer certificate, a known format and manifest version, known table and group ids for that manifest version, a create that sets every replicated group, no write to an immutable group after create, and per-group value checks (types, length limits, color range, lifecycle coherence, order key grammar, tag name trimming and length). Tightening any bound bumps the manifest version and keeps the old rule for old operations.

### Manifest

- Manifest version 1. Table ids: `1` is `quick_note_tags`, `2` is `quick_notes`. Ids are never reused.
- Every table in a freshly migrated database has a classification: `Replicated` (the two tables above), `OwnedChildren` (`quick_note_text_runs`, owned by the Quick notes body group), `Derived` (`quick_notes_search_fts` and its shadow tables), `Engine` (`sync_*`), or `Unconverted` (everything else). There is no device-local class: the vault database is snapshotted whole, so device-local state lives in the app config directory.
- Every column of a replicated table is a key, a member of exactly one group, local, or derived.
- Every UNIQUE index and multi-column CHECK on a replicated table has a declared resolution.
- Space scope: every milestone 1 table belongs to the personal space.

### Quick notes mapping

`quick_note_tags` (table 1). Tags have no revision column.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at` | Immutable | No | No |
| name | 1 | `name` | Register | No | No |
| order | 2 | `order_key` | Position | No | No |
| updated | 3 | `updated_at` | Max | No | No |

Resolution for `UNIQUE (name)`: duplicate repair (see [tag repairs](#tag-repairs-and-references)).

`quick_notes` (table 2).

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at` | Immutable | No | No |
| title | 1 | `title` | Register | Yes | Yes |
| body | 2 | Owned `quick_note_text_runs`; `body_plain_text` derived | Register | Yes | Yes |
| color | 3 | `color` | Register | No | Yes |
| tag | 4 | `tag_id` | Reference to table 1, null allowed | No | Yes |
| lifecycle | 5 | `pinned`, `archived`, `trashed_at` | Coupled register, priority trashed, then archived, then active | No | Yes |
| order | 6 | `order_key` | Position | No | No |
| updated | 7 | `updated_at` | Max | No | No |

- `revision` is local. The applier bumps it when title, body, color, tag, or lifecycle materialize a change, never for order or `updated_at`, matching the rule that reordering does not pretend content changed.
- The multi-column CHECK lives entirely inside the lifecycle group, so every materialized winner satisfies it.
- The run list value is canonical: adjacent runs with equal style merged, no empty runs.

### Migrations

`{timestamp}_sync_foundation.sql` creates STRICT engine tables:

- `sync_spaces`: space id, vault id, creation time.
- `sync_apply_state`: singleton with `applying` 0 or 1, seeded with 0.
- `sync_capture`: primary key `(table_id, row_key)`, `mask`, `forced_mask` (groups sealed even when unchanged, for conflict resolution), `created`, `deleted`, `replaced_by`, `captured_at_ms`.
- `sync_writers`: primary key `(space_id, writer_id)`, public key, device id, certificate, state, predecessor, `cutoff_seq`, `stored_seq`, `applied_seq`, `head_hash`, `head_clock`, `created_at_ms`.
- `sync_ops`: primary key `(space_id, writer_id, seq)`, kind, hash, clock, full causal context (computed at store time from the previous context and the delta), the whole signed envelope (so peers are served the exact bytes), state (`waiting`, `applied`, `held`), hold reason, `stored_at_ms`.
- `sync_rows`: primary key `(table_id, row_key)`, state `live` or `tombstoned`, a `recovery` flag, and a `conflict_mask`, both maintained by apply so status and lists read them through partial indexes. A row is present here once published.
- `sync_register_versions`: primary key `(table_id, row_key, group_id, writer_id)`, sequence, clock, value hash, the encoded `value`, and `ref_key` for reference groups. A writer's newer version replaces its older one. Values are copied from the operation body so merges, recovery, and conflicts never decode stored envelopes, and `ref_key` lets tag deletion and redirects find referrers by index. Versions of tombstoned rows are the retained state.
- `sync_tombstones`: primary key `(table_id, row_key, writer_id, seq)`, `replaced_by` (CHECK lower than the row key). The causal context is the operation's.

`{timestamp}_quick_notes_replication.sql` converts Quick notes inside the normal migration transaction:

1. Save `(id, tag_id)` of every tagged note into a temporary table.
2. Create `quick_note_tags_next` with the final shape: `id`, `name` (`NOCASE`, 1 to 40, `UNIQUE`), `order_key` (1 to 128), `created_at`, `updated_at`. No `sort_order` and no tag count CHECK.
3. Copy tags with order keys ranked by `(sort_order, id)`.
4. Drop `quick_note_tags`. Its implicit delete fires `ON DELETE SET NULL` on notes, which step 6 undoes; nothing else in notes changes.
5. Rename `quick_note_tags_next` to `quick_note_tags`. The FK text in `quick_notes` names `quick_note_tags`, so it binds to the new table.
6. Restore the saved `tag_id` values and drop the temporary table.
7. Notes are converted additively and never rebuilt, because `quick_notes` is the parent of `quick_note_text_runs ... ON DELETE CASCADE`: drop `idx_quick_notes_active` and `idx_quick_notes_tag_active`, add `order_key TEXT NOT NULL DEFAULT ''` with a length CHECK, fill it with rank keys by `(manual_order, id)`, drop `manual_order`, recreate both indexes on `order_key`, and add a trigger that rejects an empty key on insert or update. FTS is untouched.
8. Rank keys use the order key grammar: head `c` plus three base62 digits, an integer part of length four. This covers 238,328 rows; the migration aborts above that through a guard table CHECK, which no real vault approaches.
9. Install the capture, key immutability, and re-create guard triggers rendered from the manifest.
10. Seed `sync_capture` with `created = 1` and the full mask for every existing tag and note, so the first seal publishes them.

The tags rebuild stays inside the migration transaction rather than a `no-transaction` migration with foreign keys off: a crash can never leave a half-rebuilt table, and the explicit save and restore makes the FK action harmless. A test asserts that every `tag_id`, every text run, and every FTS match survive, and that `PRAGMA foreign_key_check` is empty.

### Capture

Capture triggers are persistent schema objects created by the migration from the manifest. A conformance test compares them with freshly rendered SQL and prints the expected SQL on mismatch.

- `AFTER INSERT` on a replicated table upserts a capture with `created = 1`, `deleted = 0`, and the full mask.
- `AFTER UPDATE` computes the mask with one `OLD.column IS NOT NEW.column` term per group and skips a zero mask; the upsert ORs masks.
- `AFTER DELETE` upserts `deleted = 1`.
- Every capture trigger has `WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0`, so applying remote operations never captures.
- Owned children: `AFTER INSERT`, `AFTER UPDATE`, and `AFTER DELETE` on `quick_note_text_runs` mark the parent's body group. Deleting a note cascades into runs and marks them too; the sealer treats `deleted` as superseding every group mark for that row.
- Reference rule: the tag group is not marked when the new `tag_id` is null and the old target no longer exists, because that change is the FK action of a tag deletion, which the tag tombstone already expresses.
- Row keys are immutable: `BEFORE UPDATE OF id` aborts. Immutable groups change only while `applying` is 1.
- Re-create guard: `BEFORE INSERT` aborts when `sync_rows` has the key published but the row is not present and `applying` is 0. This covers tombstoned keys and any published key whose row is gone, without depending on how it went.
- Value guards: `BEFORE INSERT` and `BEFORE UPDATE` triggers enforce the manifest value rules (row key and text without NUL, byte lengths, timestamp text of at most 64 bytes, the order key grammar, color range) on every write, applying or not, so every locally valid row seals into a valid operation. They run after column affinity, so rules test stored types. Change detection in capture and immutable guards compares with `COLLATE BINARY`, so a case-only rename of a `NOCASE` tag name is captured. The migration does not validate existing rows; a row that predates the guards and breaks a rule is reported by the sealer as an invalid local row instead of blocking the migration.
- The module that renders capture and guard SQL is `triggers`, not `capture` and `guards`; the TEMP guards for unconverted tables join it in step 4.

**Single-connection invariant.** The `applying` flag and the seal-then-apply order are correct only because every vault pool has exactly one connection, so nothing interleaves inside an engine transaction. This is recorded as an invariant: a registry test asserts `max_connections(1)` for vault pools, and the engine documents the dependency where it sets the flag. Raising the pool size requires redesigning capture suppression first.

### Sealing

- Sealing runs only while the vault is linked (a coordinator pin or at least one enrolled device) and a writer is available. A standalone vault keeps coalesced captures, at most one row per changed domain row, and seals them when it first links, before its first snapshot.
- Triggers: the commit notifier of the service (750 ms debounce, no separate cap since the 60 s fallback bounds the delay), before every exchange, before snapshot preparation, and at the start of every apply transaction.
- One transaction: read captures by `(table_id, row_key)`. A row created and deleted before it was ever published is dropped. A deleted published row becomes a tombstone (engine repair and recovery captures carry `replaced_by` or a covering intent). A created present row becomes a create with every group. Anything else becomes a write of its masked groups from current values. Encode and split at row boundaries; for each operation reserve the sequence, compute the clock and dependency delta, sign, insert it as `applied`, record its register versions and tombstones (each local version supersedes everything in its context, so it is the sole version), update the vectors, and clear the captures. Commit, then write `lastCommittedSeq`.

### Store and causal readiness

- A received operation is decoded, its envelope signature verified against the writer certificate (a genesis carries its own certificate, verified with the person key), and checked for contiguity: `seq = stored_seq + 1` and the previous hash equals the stored head. Otherwise the result is `gap` or `fork`. Operations of a revoked writer past its cutoff are refused.
- Storing is its own transaction; acknowledgement follows commit.
- An operation with an unknown format or manifest version is stored and held with reason `newer_format` or `newer_manifest`. `reevaluate_held` classifies those again after an update, and status reports held counts per reason. An operation failing intrinsic validation is held with reason `invalid`. A held operation blocks its writer's later operations and anything depending on them, is forwarded by the hub, and is listed in status.
- An operation is ready when its writer's `applied_seq` is `seq - 1`, every dependency is applied, and it is not held.
- Each exchange moves at most 32 MiB of operations per direction, with pages sized to the rest of that bound; the pass then applies what it pulled and the service runs the next pass right away. Waiting operations therefore stay bounded without a separate `buffer_full` state, which was dropped.

### Apply and merge

Ready operations apply in transactions of at most 256 operations or 1 MiB. Each transaction seals pending captures first, sets `applying = 1`, applies, resets `applying = 0`, and commits; events follow commit.

Per change:

- **Register, coupled, position, reference, max.** Remove the group's versions covered by the operation's causal context, insert the new version, and materialize the winner if it changed. Winners: register and position by greatest `(clock, writer)`; coupled lifecycle by greatest `(priority, clock, writer)`; max by greatest value. Versions with equal value hashes are not a conflict.
- **Immutable.** Set on create. Concurrent creates of one key keep the lowest `(clock, writer)`; `seq` is not compared.
- **Owned children.** The adapter decodes the run list, replaces the runs, and sets `body_plain_text`.
- **Create of a live key.** Merges as writes per group.
- **Tombstone.** Insert the tombstone, mark the row tombstoned, resolve references to it, delete the row (owned children cascade, derived FTS follows its triggers), and keep its versions as retained state.
- **Write or create for a tombstoned key.** Retain the version and recompute recovery.
- **Write for a key that is neither live nor tombstoned.** Held as `invalid`. A valid write is ready only after its row's create has applied, so this case means a defective or malicious operation.
- **Order inside an operation.** Creates apply parents first (tags before notes); tombstones apply children first.

### Tag repairs and references

- A note's materialized `tag_id` resolves its winning reference: follow the effective redirect of a tombstoned tag (the lowest non-null `replaced_by` among its tombstones), then duplicate hiding; a tombstoned tag without a redirect resolves to null. Redirects always point to lower ids, so chains terminate.
- Duplicate names: among live tags whose names collide case-insensitively, only the lowest id is materialized. The others stay in engine state and every reference to them resolves to the keeper. This is a pure function of replicated state, so every replica shows the same tags regardless of arrival order.
- Repair: a linked replica that observes a hidden duplicate seals an engine repair tombstone for it with `replaced_by` set to the keeper. Concurrent repairs from several replicas union harmlessly. The manifest declares this as a `DuplicateRepair` resolution with the unique columns and their collation, and the engine indexes tombstone redirects so reference resolution stays a lookup.
- While materialization swaps unique values between rows, a value moves aside under a numbered placeholder starting with U+007F, which user text avoids, so the UNIQUE index never fails mid-batch.
- A concurrent rename of a tag that is merged away is discarded with that tag; tag names are not recoverable.
- The nine-tag cap applies only to local creation. Merges can leave more than nine tags; they stay visible so rename and delete can fix them, and shortcuts 1 to 9 address the first nine by order.

### Conflicts

- A conflict is a surfaced group with two or more versions whose value hashes differ. For Quick notes these are title and body. Every replica derives the same conflicts.
- `quick_notes_conflict({ id })` returns, per conflicting group, the displayed value and each alternative with its device label and time.
- The engine exposes `conflicts`, `conflict_details` (versions with writer, device, clock, value, and which one is displayed), and `force_group`, which marks a group captured so the sealed write covers every alternative; requests for unknown tables, groups, or rows fail with `InvalidRequest`.
- `quick_notes_resolve_conflict({ id, group, choice })` takes `displayed`, an alternative, or `keep_both`. It writes the chosen value and marks the group in `sync_capture` even when the value is unchanged, so the sealed write's context covers every alternative. `keep_both` keeps the displayed value and creates a new note from the alternative.
- List rows carry `hasConflict` for a card indicator; Settings shows the count. Both use the open conflict mask (`OPEN_CONFLICT_MASK_SQL`): the row's conflict groups less those a local resolution forced for the next seal, so a resolved conflict disappears at once instead of after the seal.

### Recovery

- A recovery entry is a tombstoned row with retained versions of recoverable groups that no tombstone covers. This includes edits that arrived before or after the deletion.
- `sync_recovery_list` returns entries with the adapter's title and preview, device, and time.
- Restore creates a new active note from the winning retained values and seals a covering tombstone for the entry. Discard seals a covering tombstone. Both converge on every replica. Restore and discard requests reach the sync service as a `RecoveryChoice` and run inside its pass, so they never race a seal or apply.
- The trash purge is a deletion like any other, so an edit made elsewhere to a note purged after seven days becomes a recovery entry.

### Database access and mixed mode

- `DatabaseAccessMode::Guarded` opens a writable WAL connection without running migrations, then validates the schema with `validate_current_schema`. On mismatch it closes and reopens `ReadOnly`, and status shows `schema_mismatch` until both devices run the same app version.
- Hooks are installed per connection in `after_connect`, because the pool may recycle its connection. `Guarded` installs the TEMP guard triggers and the commit notifier; `ReadWrite` installs the commit notifier.
- Guard set: every `table` entry of `PRAGMA table_list` in `main`, excluding `sqlite_*`, `_sqlx_migrations`, virtual and shadow tables, and tables classified `Replicated`, `OwnedChildren`, `Derived`, or `Engine`. Unknown tables are guarded. Each guarded table gets `CREATE TEMP TRIGGER` guards for insert, update, and delete that `RAISE(ABORT, 'This vault is read-only on this device')`. Guards are connection-scoped and never enter the vault file that is snapshotted to other devices. A helper maps that trigger failure and `SQLITE_READONLY` to the existing `READ_ONLY_ERROR`, with a test.
- A `ReadOnly` request for a path with an open `Guarded` pool returns that pool, so the Calendar edit reader keeps working on replicas. A `Guarded` request for a path with an open `ReadOnly` pool reuses the read-only pool: a replica linked while a read-only pool is open stays read-only until the next replacement or restart, and an unlinked replica keeps its guarded pool until then.
- `database_access` returns `Guarded` when the role is replica, the vault is linked, the device is not in recovery, and no transfer is active; otherwise replicas stay `ReadOnly`. Role changes already close pools; the next open uses the new mode.
- `VaultOwnershipStatus` gains `replicatedWrites`, true only for `Guarded`. Frontend validation accepts it only together with `canWrite = false`. The ownership prompt explains that Quick notes stay editable while the rest of the vault is read-only on this device.
- `config.json`, the distraction usage spool, and every unconverted table keep their current owner-only rules.
- The trash purge leaves the read paths. Reads hide notes trashed more than seven days ago. `quick_notes_purge_expired_trash` deletes expired trash and returns the count and the next expiry; it does nothing while the vault is not writable on this device (owner, and guarded from step 4). The frontend lifecycle job in the window that owns background work runs it at vault open, on resume, after any local or foreign Quick notes change, and at the returned expiry, instead of polling hourly.

### Handoff integration

- **Source.** Source and reserved quiescence stop the sync service first, decide the seal mode (owner `ReadWrite` or linked replica `Guarded`) before the write fence, and seal after pools close through a private pool registry, so no app pool reopens under the fence. The snapshot copy is scrubbed with `prepare_snapshot_copy` (captures dropped, `applying = 0`) instead of requiring an empty `sync_capture`, because unsealed captures stay in the source database and are sealed there later. Snapshots carry the full operation log and engine state.
- **Receiver refresh and transfer.** Carry-forward runs inside `activate_handoff_at_path`, shared by desktop and Android, because a guarded replica can write during the download and the export must be fenced together with the replacement:
  1. `begin_snapshot_quiescence` stops the sync service, blocks writes, and seals pending captures as on the source.
  2. Open the current and staged databases read-only through a private registry and export every locally stored operation the staged database lacks, from any writer, into the bounded device-local bundle `app_config_dir/sync/{vault_id}.carry`, after any bundle an interrupted replacement left. Captures that could not be sealed (no writer or person key) refuse the export and the replacement, with an error that names unsynced Quick notes changes.
  3. Replace the vault as today.
  4. The sync service, resumed after the replacement, imports the bundle before its writer check: it stores and applies the operations in causal order and deletes the bundle. Quiesced sealing runs the same start, so a bundle is also imported before a later snapshot.
- Import runs after writes reopen rather than before `quiescence.finish()`, because opening the service under the fence would need a second pool path. A local write before the import is a capture on top of the replacement; the import stops at it, the service seals, and the apply resumes. Import is idempotent because storing skips operations already present with the same hash, so a crash between import and deletion is harmless.
- **Backup restore.** The writer check after reopen creates a new writer; captures in the restored database are sealed under it.
- **Local copy to standalone recovery** keeps its database and needs no carry-forward.

### Transport

Protocol 5 adds sync messages to the existing listener. `PROTOCOL_VERSION` is 5; linked devices must run the same version, which is acceptable before external users. Each TLS connection carries one request and one response, like the other control messages.

| Request | Response | Purpose |
| --- | --- | --- |
| `SyncHello { vault_id, device_id, manifest_version, stored, probe }` | `SyncState { manifest_version, stored, probe_hash }` | Compare vectors; the probe names the device's own head so a clone whose chain has the same length as the hub's is still found. |
| `SyncPush { vault_id, device_id, ops }` | `SyncPushResult { stored, refusal }` | Upload operations the hub lacks, stored up to the first refusal. |
| `SyncPull { vault_id, device_id, known, max_bytes }` | `SyncOps { ops, more }` | Download operations in per-writer order. |
| `SyncWait { vault_id, device_id, known }` | `SyncState` | Long poll that returns when the hub stores more than `known`, when a newer wait of the same device arrives, or after 12 s (below the 15 s control timeout). |
| `SyncHashes { vault_id, device_id, writer, from_seq, limit }` | `SyncHashList { hashes }` | Consecutive operation hashes of a writer, to locate a fork point. |

- Every request carries `protocol_version` and the vault and device ids instead of a space id, because the personal space id derives from the vault id. The listener runs `verify_authenticated_peer` for every sync request before routing it to the hub; a listener without a hub answers `sync_unsupported`. Operations travel as base64url in pages of at most 600 KiB of raw bytes, always at least one operation, which keeps the JSON frame below 1 MiB.
- Refusal codes: `gap`, `fork`, `revoked`, `invalid`, `unknown_writer`, `newer_format` (the writer's genesis uses a format newer than the receiver reads). Hub errors use the existing `Error { code, message, retryable }` frame (`sync_unavailable`, `sync_failed`, `vault_mismatch`, `invalid_sync_request`).
- **Hub.** The coordinator desktop. `transport/server.rs` routes sync messages to the `ganbaru-sync-replica` hub, separate from the handoff coordinator actor so long polls never occupy its queue. The hub only stores and serves; its own sync service applies what it stored, so apply runs in one place. Hub stores and every service pass bump a change signal that wakes waiters; one wait per device.
- **Genesis acceptance.** The hub accepts a pushed genesis only when its certificate names the authenticated peer's device. Operations of other writers are accepted from an enrolled peer only when the hub already holds that writer's genesis.
- **Revocation.** The hub's service reads the coordinator's revoked-device list on every pass and seals a writer revoke for each active writer certified for an unlinked device, with cutoff at the hub's stored sequence, once the writer's genesis is applied. The list is the durable source, so no unlink hook is needed and a revoke missed before a crash is sealed on the next pass. Every store refuses operations past a known cutoff with `revoked`. This relies on the star topology and is redesigned in milestone 11.
- **Fork.** The hub does not seal fork revokes; it refuses with `fork` and keeps its copy. The device finds the fork point from the hello probe, a `fork` refusal on one of its own writers, or a hub that holds more of its active writer than it does: it compares `SyncHashes` pages with its own chain from sequence 1, keeps the agreeing prefix, rotates to a successor writer, and calls `reseal_fork`, which removes its operations past the prefix and re-seals them under the successor with their original clocks, manifest versions, and causal contexts. The old writer keeps its state and gets a cutoff at the kept sequence locally only; the hub's copy of the old chain remains valid, so both histories survive. A batch store stops at the first fork. A pass runs at most three recoveries.
- **Client loop.** A pass is triggered by a debounced commit, start, Android resume, `sync_now`, and a 60 s fallback: hello, push pages, pull pages, apply, then a long poll while linked. The long poll is raced against commits and `sync_now`; an answer the local vector already dominates repeats the wait. Backoff after failures: 1 s, 5 s, 30 s, then 5 min. While the hub is offline, changes queue locally. Android exchanges while the app process runs and passes on resume; a final exchange on pause is deferred.

### Service lifecycle and events

- One service per active vault, started by the desktop and mobile runtimes after the vault opens, stopped in `freeze_runtimes`, and resumed in `resume_native_runtimes`.
- Start order: reset `applying` to 0, create the personal space row, import a pending carry-forward bundle, run the writer check, seal, exchange. The purge job stays in the frontend lifecycle. On Android the service starts after interrupted-restore recovery.
- Wake-ups: the commit hook of every writable vault connection, debounced by 750 ms, and a 60 s fallback. Apply always commits its engine marker, so a pass applies only when the stored vector changed since the last apply; otherwise the hook would wake the service forever. When a seal leaves the capture count unchanged (for example only invalid rows), the service does not retry until the count changes.
- Notices: each WebView subscribes once through `sync_subscribe` with a Tauri channel (the Android bundle forbids the global event module). A `status` notice carries the status view whenever it changes; an `applied { tables }` notice follows an apply that changed rows and is routed in the frontend to the same invalidation as `listenForQuickNotesChanges`. Closed WebViews are pruned on send.
- Status view: `state` (`off`, `idle`, `syncing`, `offline`, `paused`, `waiting_for_identity`, `error`), `role` (`hub` or `client`), `paused`, `lastExchangeAtMs`, `lastError`, `pendingLocalChanges`, `waiting`, held counts by reason, conflict counts by table, and `recoveryCount`. A schema mismatch keeps the replica read-only and shows through the ownership status; buffer limits are engine refusals, so neither has a sync state. Clock warnings and per-peer exchange times on the hub are deferred.
- Pause is device-local (`paused` in the writer record). It stops exchanges; sealing continues. A pause request made before the writer exists waits in memory and is applied on the next pass.
- Commands, registered on desktop and mobile and listed in `runtime/tests.rs`: `sync_status`, `sync_now`, `sync_set_paused`, `sync_recovery_list`, `sync_recovery_restore`, `sync_recovery_discard`, `quick_notes_conflict`, `quick_notes_resolve_conflict`, `quick_notes_rename_tag`, `quick_notes_delete_tag`. Errors use a typed `{ code, message }` shape like People.

### Quick notes domain changes

- Order keys at every write site: create, `quick_notes_set_pinned`, `reorder_from_pool`, unarchive, and restore place a note first in its group or between its new visible neighbors, with no rebalancing. One placement function in `quick_notes/order.rs` reads the group in `(order_key, id)` order inside the transaction: when the slot's neighbors share a key (concurrent merges can produce that), it rewrites the run of rows equal to the lower neighbor so the slot opens without moving anything else, and when keys are exhausted it re-keys the group with rank keys. The SQL subqueries using `MIN(manual_order) - 1024` are gone. `QuickNotesCursor` and its predicate page by `(pinned, order_key, id)`. Content updates never move a note, even when they change `pinned`; the pin command, create, unarchive, and restore place it first.
- Tags: create appends after the last tag, list orders by `(order_key, id)`, new rename and delete commands, and the nine-tag cap stays a local create rule. Deleting a tag clears it from its notes with a revision bump in the same transaction and succeeds when the tag is already gone. Note writes bind `tag_id` through a lookup, so naming a tag that was deleted meanwhile leaves the note untagged instead of failing. Tag commands return typed `{ code, message }` errors (`duplicate_name`, `limit_reached`, `not_found`, `failed`). The API exposes `orderKey` instead of `sortOrder`.
- The trash purge moves to a command driven by a frontend lifecycle job; list and load stop writing.
- The run list codec and plain text projection come from the engine adapter.
- Local command semantics, including `expectedRevision`, stay as they are.

### Frontend

- `lib/api/sync.ts` with runtime validation of every response, `lib/stores/sync.svelte.ts` with a generation reset on active vault change and a reference-counted event subscription held by the visible sync section, and `lib/sync/status.ts` for presentation helpers (state and role labels, last exchange, held reasons, device names, and whether Sync now can run).
- `components/sync/SyncSection.svelte` inside `VaultHandoffPanel.svelte` (which the mobile data section already reuses) for linked vaults: state, role, last exchange, pending and waiting changes, held operations with reasons, the conflict count, Sync now, and Pause. There is no clock warning (deferred with per-peer times). `components/sync/SyncRecoveryList.svelte` lists recovery entries with Restore and Discard and publishes a Quick notes change after a restore.
- Quick notes: `QuickNoteConflictBanner.svelte` in the editor while the note has a conflict, listing each group's versions with device and time, with keep this (displayed), use this (a version), and keep both; resolving applies the canonical note through the same three-way rebase as saving, so unsaved edits to other fields survive. Cards show a conflict badge. The selected tag chip (`QuickNoteTagChip.svelte`) has a menu, also on right-click, with an inline rename and a confirmed delete. Tags are sorted by `compareQuickNoteTags` (order key, then id, by code unit like SQLite `BINARY`) with `orderKey: string` in `lib/quick-notes/types.ts` and `mapQuickNoteTag` validating the key grammar instead of range-checking positions; `QUICK_NOTE_TAG_LIMIT` remains only the create cap.
- `listenForQuickNotesChanges` also listens to `applied` notices for Quick notes tables, so the panel, the trash purge job, and the open editor refresh after replicated changes. The editor reloads the canonical note only while it has nothing unsaved; otherwise the rebase on the next save picks the changes up.
- Trash purge: `lib/quick-notes/trash-purge.ts` (pure scheduler over `createLifecycleScheduler`) and `trash-purge-runtime.ts` (wiring to the API and window sync), loaded lazily by the desktop main window and the mobile shell so neither startup bundle grows.
- Editor three-way rebase (`lib/quick-notes/rebase.ts`): the editor's base is the canonical note it loaded or last saved. On `revision_conflict` it reloads once; fields only the editor changed keep the local value, fields it did not change take the canonical value, and the result is saved with the new revision. Only fields changed on both sides to different values show the existing reload-or-keep-a-copy state.
- `VaultOwnershipStatus.replicatedWrites` in `lib/vault/state.ts` (true only with the `read-only` role). The ownership prompt reads it through `getVaultOwnershipStatus` because the pairing status does not carry it, and shows replica copy that says Quick notes stay editable.
- i18n in `lib/i18n/messages/en/sync.ts` and `es/sync.ts`, plus Quick notes keys for conflicts and tag management. Surfaces use the shared UI foundations; no new theme tokens. Dynamic message keys are avoided in favor of explicit label helpers.
- The full Svelte Check graph outgrew the 1,792 MiB heap (the committed tree already peaked near 1,740 MiB); the client check now runs with 2,048 MiB and the validation infrastructure doc records the measurement.

## Testing

Contracts:

- Golden vectors for a genesis, a changes operation with create, write, and tombstone with redirect, and a revoke. Decoding and re-encoding is byte-identical.
- Mutation tests with a small seeded generator written in the test module: random bit flips, truncation, and extension never panic and never verify.
- Order key fuzzing: `key_between` is strictly between its bounds, keys satisfy the grammar and length bound, and the migration's rank keys are part of the corpus.
- Clock monotonicity and the adoption cap, certificate signing and verification, every bound at and past its limit.

Engine:

- Unit tests per merge kind, capture rule (including the FK reference skip and cascade marks), seal case, contiguity, gap, fork, held reason, repair, conflict, recovery, and re-seal.
- Manifest conformance on a freshly migrated database: every table and column classified, capture triggers equal to the rendered SQL, and a declared resolution for every UNIQUE index and multi-column CHECK.
- Simulation harness (`tests/sim.rs`): the real Quick notes manifest only, since a generic test manifest would need its own schema and capture triggers and the Quick notes tables already exercise every merge kind used in this milestone; three to five replicas on in-memory databases with one connection each and a hub replica that every client exchanges with; random edits, deletes, purges, tag creates, renames, and deletes, reorders, resolutions, and recovery actions; delivery drops, duplicates, reordering, and partitions; crash injection at failpoints; clock skew. Invariants after every step: `foreign_key_check` empty, constraints hold, no partial apply. At quiescence the canonical dumps of domain and engine tables are byte-identical. A crash at a failpoint must leave the database unchanged before the call is retried. Revocations by the hub drive rotation and re-seal; forks are simulated with the transport in step 5. 48 seeds of 60 steps by default (about 30 s in the debug test profile); `GANBARU_SYNC_SOAK_SEEDS` sets more and `GANBARU_SYNC_SEED` replays one failing seed. One replica set at a time keeps memory bounded.
- Crash boundaries through failpoints: before seal commit, after seal commit and before the record update, at reservation, mid-batch apply, during carry-forward export, between replacement and import, and during re-seal.

Database (`crates/ganbaru-db/src/tests/`):

- `sync.rs`: engine tables, CHECKs, and keys.
- `quick_notes.rs`: migration survival of tags, assignments, runs, and FTS; order keys valid and ordered like the old positions; `foreign_key_check` empty.
- Access modes: guards abort unconverted writes, converted and engine writes succeed, schema mismatch degrades to read-only, and a `ReadOnly` request reuses an open `Guarded` pool.

App:

- Writer record reservation, restore detection, clone detection, and key loss.
- Role to access mode mapping.
- `LocalPair` transport tests for hello, push, pull, wait, unauthenticated refusal, fork, revoke, and protocol version.
- Three-replica Quick notes convergence through the real hub handler.
- Carry-forward across refresh and transfer, including a crash between replacement and import.

Frontend: focused Vitest for `api/sync`, `stores/sync`, `sync/status`, the Quick notes API mapper, the editor rebase helper (pure), and tag ordering.

## Work breakdown

Each step is a pull request into `dev` from its own topic branch. Every pull request updates the docs its contract changes, runs `pnpm -w run check` while iterating and `pnpm -w run validate` before opening; steps 1 and 2 add workspace crates and also run `pnpm -w run validate:full`.

| Step | Branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-contracts` | Contracts crate, codec, signing, certificates, order keys, golden vectors, mutation tests. | `cargo test -p ganbaru-sync-contracts -j 1 -- --test-threads=1` |
| 2 | `feat/sync-schema` | Engine crate skeleton with manifest and capture rendering; both migrations; conformance; Quick notes backend on order keys, tag rename and delete, purge job; frontend tag mapper and ordering. | `cargo test -p ganbaru-db --lib -j 1 quick_notes -- --test-threads=1`, `cargo test -p ganbaru-sync -j 1 conformance -- --test-threads=1`, `cargo test -p ganbaru-tauri-app --lib -j 1 quick_notes -- --test-threads=1`, focused Vitest for Quick notes |
| 3 | `feat/sync-engine` | Seal, store, readiness, apply, merges, repairs, conflicts, recovery, re-seal, simulation harness. | `cargo test -p ganbaru-sync -j 1 -- --test-threads=1` |
| 4 | `feat/sync-replica-access` | `Guarded` mode and hooks, guards, ownership mapping, writer keys and record, service lifecycle without transport, carry-forward, purge on guarded replicas. | `cargo test -p ganbaru-db --lib -j 1 access -- --test-threads=1`, `cargo test -p ganbaru-tauri-app --lib -j 1 sync -- --test-threads=1` |
| 5 | `feat/sync-transport` | Protocol 5, hub and client, revocation, forks, status events. | `cargo test -p ganbaru-tauri-app --lib -j 1 handoff -- --test-threads=1` |
| 6 | `feat/sync-ui` | Sync section, recovery list, conflict UI, editor rebase, i18n. | Focused Vitest, `pnpm --dir apps/client run check` |
| 7 | `docs/sync-milestone-1` | Final docs, acceptance results, goal README update. | `pnpm -w run validate` |

Captures accumulate after step 2 without sealing; they are bounded by the number of rows, and step 4 seals them.

## Documentation

- `docs/data/sync.md`: protocol 5 messages, mixed mode, writers, carry-forward, and the delivery status rows.
- `docs/architecture/decisions/sync-engine.md` (new): capture and seal, registers with version vectors, installation writers, the hand-written codec, and the rejected alternatives.
- `docs/algorithms/sync/README.md` (new): merge rules with worked examples (concurrent title edit, delete against edit, duplicate tag, concurrent reorder, fork re-seal).
- `docs/data/schema/sync.md` (new), the Quick notes section of `supporting-domains.md`, and the schema README domain map.
- `docs/features/quick-notes.md`: editing on replicas, conflicts, recovery, tag rename and delete, ordering.
- `docs/data/security/network-and-privacy.md`: the sync message surface and writer key storage.
- `docs/testing/README.md`: simulation seeds, `GANBARU_SYNC_SOAK_SEEDS`, and focused sync commands.
- `docs/architecture/native-backend.md`: the sync module boundary.
- `docs/features/README.md`, `docs/ROADMAP.md`, and the `AGENTS.md` workspace map (new crates, `src/sync.rs` and `sync/`, `lib/sync/`, `components/sync/`).
- This folder: progress below and the goal README.

## Manual acceptance

Two desktops (A is the coordinator and owner, B is linked) and one Android phone (P). Quick notes only unless stated.

| Scenario | Expected |
| --- | --- |
| Edit a note on P while A owns the vault | Appears on A and B within about 2 s. |
| P offline edits a title; A edits the same title | After reconnect both show the same title and a conflict; resolving on one device clears it everywhere. |
| P offline changes color; A changes the body | Both changes kept, no conflict. |
| A permanently deletes a note while P edits it offline | P's edit becomes a recovery entry; Restore creates an active copy on every device. |
| A and P each create tag "Work" offline and tag notes | One "Work" tag everywhere; every note keeps it. |
| A and P reorder the same notes offline | Same order on every device. |
| B edits offline, then ownership moves to P | Nothing lost; B's edits arrive after B reconnects. |
| P has unsynced edits when a refresh activates | Edits carried forward and synced. |
| Restore a backup on B | B uses a new writer; no duplicated or lost notes. |
| Unlink B on A | B's later edits never reach A or P; B shows that it is unlinked. |
| P's clock is one day ahead | Data stays correct; P's clock is adopted only up to local time plus 60 s. The clock warning is deferred with per-peer times. |
| Pause sync on P, edit, resume | No exchange while paused; catches up on resume. |
| A offline while B and P edit | Edits queue; everything converges when A returns. |
| Edit a Calendar event on B | Still refused with the read-only prompt. |
| Background P right after an edit | The edit reaches A before suspension or at the next foreground. |
| B on an older app version | `schema_mismatch` status, read-only, no data loss after updating. |

### Acceptance results

Automated equivalents run in the full gate on in-memory and temporary-file replicas (engine tests in `ganbaru-sync`, exchange tests over the real hub and client in `sync/tests/exchange.rs`, and the seeded simulation). They prove the data behavior of each scenario; latency, Android lifecycle, and on-screen presentation need the physical run.

| Scenario | Automated coverage | Physical only |
| --- | --- | --- |
| Edit on P reaches A and B | `exchange_pushes_and_pulls_until_both_replicas_hold_every_operation`, `wait_returns_once_the_hub_stores_new_operations` | The 2 s latency |
| Concurrent title conflict and resolution | `concurrent_titles_surface_a_conflict_that_a_forced_choice_resolves`, `a_resolved_title_conflict_closes_on_both_replicas` | Banner and badge |
| Color against body | `concurrent_values_of_groups_that_are_not_surfaced_merge_silently` | None |
| Delete against offline edit, then Restore | `restoring_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere` (engine and exchange), `discarding_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere` | Settings recovery list |
| Duplicate "Work" tag | `duplicate_tag_names_keep_the_lower_key_and_redirect_references`, `deleting_a_tag_clears_concurrent_assignments` | None |
| Concurrent reorder | Seeded simulation (`seeded_runs_converge_after_partitions_crashes_and_revocations`) | None |
| Ownership moves while B is offline; refresh with unsynced edits | `carry_forward_moves_local_operations_into_a_replacement`, `snapshot_copies_drop_unsealed_captures` | The handoff flow end to end |
| Backup restore on B | `writer_survives_restarts_and_rotates_after_a_restore`, `a_cloned_installation_is_found_and_resealed_by_a_successor` | None |
| Unlink B | `hub_refuses_foreign_geneses_unlinked_writers_and_other_vaults`, `revoking_a_writer_reseals_its_unpublished_operations` | Unlinked status on B |
| P's clock one day ahead | `observe_adopts_remote_clocks_up_to_the_cap`, `observe_never_moves_the_clock_backwards` | None |
| Pause and resume | Pause state persists in the writer record | No exchange while paused |
| A offline while B and P edit | Seeded simulation partitions | None |
| Calendar edit on B | `guards_abort_unconverted_writes_and_allow_replicated_ones` | Read-only prompt |
| Background P after an edit | None | Suspension and next foreground |
| B on an older version | `operations_with_a_newer_manifest_are_held_until_an_update_can_read_them` | `schema_mismatch` status |

## Performance budgets

Measured in release-profile tests recorded in the pull request, and on the acceptance devices.

| Measure | Budget | Desktop result (release, median of 5, 2026-10-09) |
| --- | --- | --- |
| Capture overhead per domain write | Under 0.2 ms | 0.024 ms |
| Seal of 100 changed notes | Under 20 ms | 15 ms |
| Apply of a 256-operation batch | Under 50 ms desktop, under 150 ms Android | 45 to 48 ms desktop; Android pending (step 8) |
| Guard installation per connection | Under 50 ms Android | Pending (step 8) |
| LAN latency between foreground devices | Under 2 s at p95 | Pending (step 8) |
| Simulation with 200 seeds | Under 2 minutes single-threaded in the release profile (the debug default runs 48 seeds) | 91 s |
| Sync service steady-state memory | Under 10 MiB | Pending (step 8) |

The desktop rows come from the ignored `budgets` test in `ganbaru-sync`, which asserts them in the release profile. Meeting them required batching per-change statements: a created row skips the register-version lookup, version upserts and capture deletes run as one statement each, the flag refresh reads state and versions together and hands them to materialization, and the apply loop keeps writer heads in memory, reads small envelopes with them, and writes applied sequences once per transaction.

## Risks

| Risk | Mitigation |
| --- | --- |
| Operation log grows until compaction | Debounced sealing coalesces edits; bounded values; compaction in milestone 10. |
| Guard installation is slow on Android | Measured in step 4; triggers can be generated once per schema version and reused. |
| FK actions inside triggers behave differently than assumed | Verified first in step 2 with focused SQLite tests before engine work depends on them. |
| A replica without the person key cannot seal, which blocks refresh | Status explains it; the key is released over the same link that sync uses. |
| Whole-vault refresh is costly while mixed mode lasts | Unchanged from today; retired in milestone 10. |
| Re-seal after a fork is complex | Isolated module, crash-boundary tests, and simulation of clone scenarios. |
| Plaintext writer keys on Android | Same protection as the person key; Keystore wrapping in milestone 11. |
| All linked devices must update together | Exact protocol version match with a clear status; acceptable before external users. |
| Simulation memory on constrained machines | One replica set at a time, temporary files, serialized test threads. |

## Deferred

- Engine generalization, membership sets, and other domains (milestones 2 and 4 to 9).
- Coordinator address refresh, discovery, overlay networks, and Android background sync (milestone 3).
- Compaction, causal stability, acknowledgements, and managed asset garbage collection (milestone 10); generic managed assets (milestone 2).
- Anchored authority, LAN peer exchange, and Android Keystore (milestone 11).
- Record encryption, relay, and always-on anchor (milestone 12).

## Verify during implementation

- The sqlx 0.8 commit hook through `lock_handle()` inside `after_connect`, its thread requirements, and that the notifier only flips a flag and wakes the service.
- Pool connection recycling (`idle_timeout`, `max_lifetime`): hooks must be present on every connection, or recycling disabled for vault pools.
- Inside one migration transaction: the implicit delete of `DROP TABLE` sets `tag_id` to null, the rename binds the FK to the new table, the restore succeeds, and `foreign_key_check` is empty.
- `ADD COLUMN` with a CHECK against existing rows, and `DROP COLUMN` once the indexes are gone, on the SQLite bundled by `libsqlite3-sys` 0.30.
- Whether an `ON DELETE SET NULL` action fires `AFTER UPDATE` triggers on the child while the parent row is already gone, which the reference capture rule relies on.
- UPSERT inside trigger bodies and `CREATE TEMP TRIGGER` on `main` tables.
- That `begin_snapshot_quiescence` blocks domain writes before the carry-forward export and that apply completes before `quiescence.finish()`.
- Guard count and installation time with the full schema on Android.
- That the coordinator can serve hub writes while it holds a `Guarded` pool as a non-owner.

## Progress

- [x] Step 1: contracts. All steps are implemented on one branch, `feat/sync`, and split into pull requests at review time. Order keys follow the reference algorithm except that decrementing into the smallest integer appends a fraction, so `key_before` never yields the invalid smallest key; a key with the smallest integer and a fraction is valid. `PersonKeyPair::from_seed` was added to `ganbaru-contacts` for deterministic vectors.
- [x] Step 2: schema and Quick notes backend. Both migrations, manifest, rendered capture and guard triggers, conformance, order-key Quick notes backend (reorder between visible anchors, tag rename and delete), the lazily loaded trash purge job, and the frontend tag mapper are in place; Rust tests, clippy, focused Vitest, and the client check pass.
- [x] Step 3: engine and simulation. Engine modules, unit tests for every merge kind, store refusal, held reason, repair, conflict, recovery, and re-seal case, and the seeded simulation are in place; deviations from the original design are recorded in the sections above (re-seal keeps clocks and contexts, cutoffs refuse at store, Quick notes manifest only in the simulation, hub replica on in-memory databases, revocation-driven rotation only, forks deferred to step 5, debug default of 48 seeds). The full crate tests pass in about 45 s, 300 soak seeds pass, and clippy passes.
- [x] Step 4: replica access, writers, and carry-forward. `Guarded` access with per-connection hooks (guards and commit notifier), ownership mapping with `replicatedWrites`, writer keys (keyring on desktop, private files on Android) and the reserved record with the high-water check and successor rotation, the transport-free service lifecycle around handoff quiescence, quiesced sealing, scrubbed snapshot copies, and the carry-forward bundle are in place; deviations are recorded in the writer, access, handoff, and service sections. The full app library tests (1127), `ganbaru-db` tests, and workspace clippy pass.
- [x] Step 5: transport. Protocol 5 sync messages, the hub behind `verify_authenticated_peer`, the client exchange with long poll and backoff, fork recovery through the hello probe and hash pages with a successor writer, stateless revocation from the revoked-device list, apply only on stored-vector change, the status, sync-now, and pause commands, and Android resume passes are in place; deviations are recorded in the transport and service sections. Exchange tests cover convergence, wait, refusals, revocation, and a cloned installation. The full app library tests (1135) and clippy pass.
- [x] Step 6: frontend. The sync API, store, status helpers, Settings sync section with recovery, the conflict banner and card badge, tag rename and delete, the editor rebase, replica ownership copy, and i18n are in place; deviations are recorded in the frontend section. Focused Vitest (sync API, store, status, Quick notes API, rebase, tags, vault state) and the client check pass.
- [x] Step 7: docs, validation, and automated acceptance. Docs and the full validation gate are done (Rust format, clippy, and tests; client check; all Vitest shards; editor check; bundle contracts); every acceptance scenario is mapped to automated coverage in the acceptance results. The owning docs describe mixed mode, the engine decision, merge rules with worked examples, the sync schema, Quick notes behavior on linked devices, the sync network surface and writer key storage, focused sync test commands, and the module boundary; the workspace map lists the new crates and modules. Each exchange is bounded to 32 MiB per direction in place of the planned `buffer_full` state, the clock warning is deferred, and sync notices reach each WebView through a `sync_subscribe` channel because the Android bundle forbids the global event module. The Android module ceilings rose for the sync API, status helpers, conflict banner, rebase, tag chip, and sync catalog; generic validation readers moved to `lib/utils/readers.ts` so Quick notes no longer pulls Chat contracts.
- [ ] Step 8: physical acceptance. Blocked on hardware an agent cannot drive: the "Physical only" column of the acceptance results and the budgets marked pending (Android apply and guard installation, LAN latency, service memory) need two desktops and an Android phone running release builds. The maintainer records the results here, then the goal README status moves past Partial.
