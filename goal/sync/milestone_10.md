# Milestone 10: handoff retired

**Status: Planned.** Part of the [concurrent sync goal](README.md). This file is the implementation plan for retiring whole-vault handoff: every linked device opens the vault read-write, a new or long-dormant device starts from a scrubbed snapshot, devices acknowledge what they applied, operation bodies are compacted after causal stability, dormant writers stop holding compaction back, unreferenced managed assets are collected, and one process at a time opens a vault. [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract, with the [sync engine decision](../../docs/architecture/decisions/sync-engine.md), the [sync schema](../../docs/data/schema/sync.md), and the [sync topology and authority decisions](../../docs/architecture/decisions/sync-topology-and-authority.md); each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- There is no vault owner. Every linked device writes every domain at any time; the ownership prompt, the transfer flow, read-only and guarded modes, and whole-vault refresh are gone.
- Linking a device downloads one scrubbed snapshot from the device it linked with, then operation exchange keeps it current. A desktop puts the linked vault in a folder the user picks and keeps its own vault untouched. A phone that held a different vault keeps it as a recoverable backup.
- A device that was quiet but stayed current never downloads the vault again. A device away long enough that the others compacted past it downloads a fresh snapshot once, carrying its unsynced changes forward.
- Storage stays bounded: bodies of operations every counted device has applied are dropped, and managed asset files nothing references are deleted after a grace period.
- Opening a vault that another Ganbaru process (for example a development build) already has open is refused with a clear message instead of corrupting engine state.

## Scope

In scope:

- Removal of `VaultOwnershipManager`, `vault-ownership.json`, transfer phases, `VaultDatabaseAccess`, the `Guarded` and `ReadOnly` database access modes, guard triggers, the per-domain replica write flags from milestone 2, `owner_required` errors from milestones 4 to 9, the ownership prompt, the transfer and refresh protocol messages, and the coordinator's bundle uploads.
- Snapshot bootstrap as the only bundle path, for first links on desktop and Android and for re-bootstrap after dormancy.
- Kind `3` signed acknowledgements, a writer for every linked installation, causal stability, dormancy, operation body compaction, and the `bootstrap_required` signal.
- A managed write fence kept for vault replacement, now independent of ownership.
- Cross-process vault fencing through an OS lock on a lock file in the vault root.
- Garbage collection for every managed asset class.
- Data settings: bootstrap progress, "needs a fresh copy", dormant devices, and the device holding compaction back.

Kept, explicitly:

- The coordinator role: pairing, the TLS listener, admission, network access, enrollment approval, contact requests, person key release, the hub, and revocation authority (`crates/ganbaru-sync/src/store.rs:326-327`). Removing ownership does not remove the coordinator. Milestone 11 replaces revocation authority with the anchor.
- Carry-forward (`crates/ganbaru-sync-replica/src/carry_forward.rs`), which backup restore also uses (`apps/client/src-tauri/app/src/vault/backup.rs:992-1002`).
- Snapshot quiescence, staged activation, and database rollback copies for replacements.

Out of scope:

- Typed checkpoint bootstrap (README decision: whole-database snapshot).
- File sync of managed project folder contents (README decision: device-local after the one bootstrap copy).
- LAN peer exchange and member listeners (milestone 11).
- Deleting compacted operation rows or their contexts (Deferred).
- Merging two vaults (an explicit import preview, not this goal).
- Usage row compaction for distractions (milestone 7 Deferred).

## Prerequisites

The work splits into an engine half and a retirement half with different prerequisites:

- Steps 1 to 4 (fence, acknowledgements, compaction, asset collection) need [milestone 2](milestone_2.md) merged: winner elision with envelope hydration, the asset class registry with `asset_refs`, `AssetStore`, `carry_assets`, and `prepare_snapshot_copy`, which empties Local tables and resets Local columns to their materialization values. Protocol versions follow the README wire versions rule, so no step waits for another milestone's protocol change. These steps merge before [milestone 11](milestone_11.md), so the counted set comes from the coordinator's pairing state; milestone 11 step 6 switches it to the authorization log.
- Steps 5 and 6 (bootstrap and ownership removal) need milestones [4](milestone_4.md) to 9 merged, and step 5 also needs milestone 11 step 9, whose member listeners it serves bootstrap from, so no table is `Unconverted` and no command returns `owner_required`. Milestone 7 has already removed the distraction usage exchange (`DistractionsExchange`, `DistractionsAcknowledged`) and the native messaging host's vault writes.
- Step 6 also removes [milestone 3](milestone_3.md)'s ownership reads in the headless Android pass (the config-directory variants of `vault::ownership`).
- Step 6 flips Notes to writable on replicas, which needs Notes recovery restorers first: no Notes table has one, so recovery Restore and Discard both fail for Notes entries, and the restorer coverage test in `apps/client/src-tauri/app/src/sync/recovery.rs` demands them once `replica_writable` is gone. Step 6 adds them; this unblocks 13 of the 14 Notes manual acceptance rows. Keep both for surfaced block conflicts in the page strip stays a pre-release item, not part of step 6.

## Current state

- **Ownership.** `apps/client/src-tauri/app/src/vault/ownership.rs` (1586 lines) persists `vault-ownership.json` (:15) with an owner device, a generation, and `TransferPhase`. `database_access` (:254-262) maps `can_write` to `ReadWrite`, a linked stable replica to `Guarded`, and anything else to `ReadOnly`. `acquire_managed_write` (:404) and `acquire_replicated_asset_write` (:413) gate permits on ownership, and `fence_managed_writes` (:450) drains permits for replacement. `register_remote_owner` (:321), `recover_local_copy` (:299), and the outgoing and incoming transfer methods (:488 onward) exist only for ownership. Callers outside the file include quiescence, backup, `handoff.rs`, `contacts.rs`, the handoff receiver and coordinator, Android distractions, the Focus native runtime, the sync service, the Music session runtime, `calendar_edit`, and about 25 other modules, mostly asset writers; recount with `grep` when deleting.
- **Database access.** `apps/client/src-tauri/app/src/db.rs:163-190` opens the vault `ReadWrite`, `ReadOnly`, or through `connect_path_guarded`. `ganbaru_db::DatabaseAccessMode` (`crates/ganbaru-db/src/lib.rs:21-27`) documents that a guarded connection runs no migrations. Guards are connection-scoped TEMP triggers (`crates/ganbaru-sync/src/guards.rs:1-7`).
- **Protocol.** `PROTOCOL_VERSION` is 6 (`crates/ganbaru-handoff/src/protocol.rs:16`). `BundlePurpose { Ownership, Refresh }` (:174-177). Control messages for bundles and transfers (:212-310): `RequestBundle`, `BundlePrepared`, `BundlePending`, `DownloadBundle`, `UploadBundle`, `BundleMetadata`, `ResumeAt`, `BundleComplete`, `BundleStaged`, `CommitStagedOwnership`, `OwnershipGrant`, `CommitUploadedOwnership`, `ActivationComplete`, `ActivationAcknowledged`, `CancelTransfer`, `TransferCancelled`, `RefreshRequest`, `RefreshStatus`. `SyncHello` already sends the client's stored vector (:357-364). `CoordinatorPin` carries a `generation`; pairing types do not deny unknown fields, and `LinkedPeer.linked_at_ms` exists (`crates/ganbaru-handoff/src/pairing.rs:92-103`).
- **Snapshots.** `source.rs:121-152` prepares an ownership bundle under source quiescence or a refresh bundle under snapshot quiescence. `create_handoff_archive` (`backup.rs:330-349`) runs `VACUUM INTO` (:172) on a separate pool, `prepare_snapshot_copy` (:973, `crates/ganbaru-sync/src/local.rs:113-121` clears `sync_capture` and the apply flag), and archives the `backup.rs:34-71` allowlist (`is_bundle_layout_entry`: `vault.json`, `config.json`, `ganbaru.sqlite`, and the `notes`, `diary`, `projects`, `reports`, `assets`, `templates`, and `.yjs` directories), so `vault.lock` and `.database-rollbacks/` are already outside it. Prepared bundles are staged in the app config directory as `{id}.source.zip` (`pairing/transfers.rs:239-249`). The untouched-starter check reads the app-state `starter_vault_id` record (`vault.rs:82`). `MAX_ARCHIVE_BYTES` is 100 GiB.
- **Bootstrap today.** `receiver.rs:63-77` admits bootstrap only over an untouched Android starter vault; every other first link takes the ownership path. Replacements keep rollback copies (`database_rollback.rs:26-32`, three unflagged and two flagged per vault).
- **Engine log.** `sync_ops` (`crates/ganbaru-db/migrations/20261008120000_sync_foundation.sql:50-65`) stores `hash`, `clock`, `context`, `envelope` (`length(envelope) <= 524288`, so an empty blob is valid), `state`, and `stored_at_ms`. `sync_writers` (:31-46) has no compaction column. No code deletes operation bodies. Envelope readers are pull serving (`crates/ganbaru-sync/src/log.rs:299-399`), apply of unapplied candidates (`apply.rs:98-195`), own head verification (`local.rs:138-150`), fork re-seal of the own writer beyond the confirmed prefix (`reseal.rs:37-58`), and `reevaluate_held` (`status.rs:231-275`) for held operations, which compaction never touches. Tombstone contexts come from `sync_ops.context` (`merge.rs:455-482`, `log.rs:190-204`), not from envelopes.
- **Kinds.** Kind `3` is reserved; an unknown kind is held as `newer_format` (`store.rs:147-149`), which blocks that writer's later operations on older builds. Catch-up operations are kind `0` with no changes, sealed only when a change needs more than 256 dependencies (`seal.rs:318-349`).
- **Writers.** `ensure_writer` runs at service start and on every pass (`apps/client/src-tauri/app/src/sync/service.rs:883`, :449) and creates the writer key, certificate, and local record once the person key is available (`crates/ganbaru-sync-replica/src/writer.rs:358-436`, `service.rs:960-978`). Its genesis is sealed only with the first captured change (`crates/ganbaru-sync/src/seal.rs:160-195`), so quiet replicas have no `sync_writers` row on the coordinator.
- **Assets.** Quick notes has a grace-period sweep (`crates/ganbaru-quick-notes/src/images.rs:639`); other classes have none.
- **Processes.** `tauri-plugin-single-instance` (`runtime/desktop.rs:221`) is keyed by app identifier, so a production and a development build can open the same folder. Rust toolchain is 1.98.0, so `std::fs::File::try_lock` is available without a dependency.
- **Frontend.** `lib/vault/ownership.svelte.ts`, `ownership-prompt.ts`, `VaultOwnershipPrompt.svelte`, `handoff-workflow.ts`, `VaultHandoffPanel.svelte`, and `DesktopBundleReceiveMode = "ownership" | "refresh" | "bootstrap"` (`lib/api/vault-handoff.ts:65-66`). Commands `handoff_recover_local_copy` and `handoff_request_owner_bundle` (`vault/handoff.rs:768`, `:782`).


### Re-verification on 2026-10-22

Checked against `feat/sync` after milestone 2, milestone 3 steps 1 to 8, milestone 4 steps 1 to 3, and milestone 5 steps 1 to 3. Line numbers above are stale; facts that changed:

- Protocol: `PROTOCOL_VERSION` is 8 (`crates/ganbaru-handoff/src/protocol.rs:18`), `FORMAT_VERSION` is 2 (`crates/ganbaru-sync-contracts/src/op.rs:47`). Step 2 takes the next free protocol version at merge (9 if nothing else lands first) and decides whether kind `3` also bumps `FORMAT_VERSION`; old builds already hold kind `3` as `newer_format`.
- `DistractionsExchange` and `DistractionsAcknowledged` still exist (`protocol.rs:312-324`); the Prerequisites claim holds only after milestone 7, which step 6 waits for anyway.
- Ownership callers are about 33 modules, including themes, preferences, `sync/host.rs`, Chat dispatch, Notes data sources, and Focus idle and enforcement. Per-domain write flags also live in `DomainManifest.replica_writable`; step 6 deletes that field too.
- Envelope readers also include `hydrate.rs` (`elided_value`, reached from `local::retained_group_values` and `materialize.rs`), own-head verification in `local.rs`, fork reseal (`reseal.rs`), held reevaluation and storage sizing (`status.rs`), and tombstone contexts (`recovery.rs`, `log.rs`). Compaction's "empty body means compacted" rule must cover every one of them, and `hydrate.rs`'s doc comment must change.
- Nothing deletes operation bodies; rows are deleted only by fork truncation, and versions only when superseded by a covering context. `sync_ops` also has `hold_reason`.
- Catch-up is `seal.rs:339-375` with a hard-coded `Content::Changes`; acknowledgements need a kind parameter. Genesis on first pass must bypass the early return in the service's `seal_pending` when nothing is pending.
- Dormancy: `LinkedPeer.linked_at_ms` is `#[serde(default)]`, so a peer missing it loads as 0. The dormancy rule treats 0 as unknown and starts the dormancy clock at the coordinator's first observation instead.
- Stability: the hub ignores `SyncHello.stored`; `SyncState` has no `stable` or `bootstrap_required`; `SyncRefusalCode` has no bootstrap code. All are additive. The hub only stores pushes; stability recomputation belongs in the coordinator's service pass, not in `hub.push`.
- Seal before export is decided: quiescence seals, and `export_with` refuses pending captures (its message still names Quick notes).
- Asset classes: `quick-notes/images` (sweep `collect_unreferenced_images` in `ganbaru-quick-notes/src/images.rs`, 24 hour grace, gated on ownership in the app command), `profile` (no sweep), `music/artwork` (no sweep, and no music adapter until milestone 5 step 4). The generic sweep in step 4 must not run for a class whose domain has no adapter reporting references, or it would delete live artwork; it also drops the ownership gate.
- Only Quick notes, preferences, themes, and music (no tables yet) are registered domains; everything else is `Unconverted`, which matters only for steps 5 and 6.

## Design

### Acknowledgements

- Kind `3` is a signed acknowledgement. It has no changes and an empty body; its dependency delta lives in the signed header like any operation, so its stored context is the writer's full applied vector at sealing time. More than 256 advanced writers produce consecutive kind `3` operations, through the existing catch-up loop with the kind passed through.
- Every operation of a writer acknowledges its context, so a device that edits needs no separate acknowledgement. Kind `3` exists so that acknowledgement-only traffic never triggers further acknowledgements: a kind `3` operation from another writer is never a reason to seal one.
- Sealing rule, evaluated after each apply: seal kind `3` when the applied vector covers an operation of any kind other than `3` from another writer that the own head context does not cover, at most once per 10 minutes per writer. Independently, seal a keepalive kind `3` when the own writer's newest operation is older than 7 days. Both constants live with the engine bounds.
- Every linked installation seals its genesis on its first sync pass, not only when it first captures, so that the coordinator knows its writer and it can acknowledge. A device whose person key is not yet released has no writer and acknowledges nothing; see stability.
- Validation: kind `3` with a non-empty body or any change is refused as malformed.

### Stability and finality

- Stability is computed on the coordinator, the device every operation passes through. It is the vector `S[w] = min over counted devices d of ack_d[w]`.
- `ack_d` is the context of the newest stored operation, with a known context, of the device's newest active writer, with the writer's own entry set to that sequence. A device whose writers all have unknown contexts, or that has no writer yet, contributes the zero vector. A successor writer's context covers its predecessor, so only the newest active writer counts; retired, revoked, and forked writers never count.
- The coordinator's own entry is its applied vector, read directly.
- Counted devices are the linked, non-revoked, non-dormant devices: the pairing state's linked peers for this vault plus the coordinator. Milestone 11 step 6 replaces this with the devices enrolled in the authorization log.
- Finality follows without a separate mechanism: the anchor (the coordinator until milestone 11 hands it over) is a counted member, its acknowledgement is its applied vector, applied is a subset of stored, and milestone 11 defines final as stored by the anchor. Therefore every stable operation is final, and compaction never drops a body that removal exclusion could still need.
- `SyncState` gains `stable: Vec<SyncSeq>` (writers with a non-zero entry) and `bootstrap_required: bool`. Spokes compact only up to the last `stable` the coordinator sent them. Worst case 4096 JSON `SyncSeq` entries of about 60 bytes is about 240 KiB, within the 1 MiB `MAX_CONTROL_BYTES` control frame and no larger than the `stored` vector `SyncHello` already sends.
- Stability is recomputed after each hub apply that stores operations, from `sync_writers` and one `sync_ops` lookup per counted writer. It is not persisted.

### Dormancy

- A device is dormant when the coordinator's local time minus the newest `stored_at_ms` among its writers' operations exceeds 90 days, or, for a device with no stored operation, when `linked_at_ms` is older than 90 days. Receipt time on the coordinator is used instead of signed clocks, so a skewed device clock cannot make others dormant. The weekly keepalive keeps every connected device far from the limit.
- Dormancy is derived, never stored: no writer state is added, and a dormant device that stores a new operation is counted again immediately. Its low acknowledgement then holds stability back until it re-bootstraps; compaction already done is never undone.
- A dormant device's late operations are accepted and merged like any others. Versions, tombstones, and contexts are retained, so merges stay correct after compaction, and an offline edit can still win a register. Removal cutoffs are the only exclusion rule, and they belong to milestone 11.

### Compaction

- Unit: an applied operation of kind `0` or `3` at or below `S[w]`. Compaction replaces its `envelope` with the empty blob and keeps the row with `hash`, `clock`, `context`, `kind`, `state`, and `stored_at_ms`, so fork detection, tombstone contexts, and `next_context` keep working. Genesis envelopes and revoke envelopes (until milestone 11 removes kind `2`) are kept whole because certificates and cutoffs live in their headers. Kind `4` authorization entry bodies are never compacted, because milestone 11 replays the authorization log on anchor reselection and for new members. Every writer's head operation is kept whole because own head verification reads it.
- Headers are not kept separately. Nothing reads a header of an applied operation except pull serving, which never serves a compacted operation, and fork re-seal, which only reads the own writer above the confirmed prefix.
- `sync_writers` gains `compacted_seq INTEGER NOT NULL DEFAULT 0 CHECK (compacted_seq >= 0)` through `ALTER TABLE ADD COLUMN` in a new engine migration; no table rebuild. Compaction advances it in sequence order and never lowers it. `apply.rs` treats an empty envelope at or below `compacted_seq` as compacted, and anything else empty as corrupt.
- Own writer cap: a spoke compacts its own writers only up to the head the last hello probe confirmed the coordinator holds with the same hash, so a fork re-seal always has its bodies.
- Hydration duty from milestone 2: before dropping a body, every elided version that operation authored is hydrated when its domain row cannot serve the value, that is when the row has a pending capture, fails the stored hash, or is missing. Elided winners whose row matches stay elided, so winner values are still stored once.
- Batches of 256 operations per transaction, run at the end of a sync pass under the pass gate, until no candidate remains or 200 ms of work has passed. Freed pages are reused by SQLite; no `auto_vacuum` change and no `VACUUM`. Snapshots are produced with `VACUUM INTO`, so bootstrap archives are compact anyway.
- Serving: the hub sets `bootstrap_required` when the client's `SyncHello.stored[w]` is below `compacted_seq[w]` for any writer, and a pull that reaches a compacted operation is refused with the same typed reason as a backstop. A spoke never serves another writer's operations in the star, so its own compaction cannot strand anyone.
- Carry-forward export reaching a compacted operation that the staged database lacks is an error that aborts the replacement and keeps the current vault; it cannot happen when the staged snapshot is newer than the local `stable`, which bootstrap guarantees.

### Bootstrap

- One path. `BundlePurpose` is deleted; `RequestBundle`, `BundlePrepared`, `DownloadBundle`, `BundleMetadata`, `ResumeAt`, `BundleComplete`, `ActivationComplete`, `ActivationAcknowledged`, `CancelTransfer`, and `TransferCancelled` remain, without `purpose` and `generation` fields. The hub keeps the activation messages so it can delete the prepared archive and record when the device joined.
- Source: any serving member, meaning the anchor or a member listener from milestone 11 step 9; the new device first asks the device it enrolled with. Freshness is not a selection criterion, because operation exchange closes any gap after activation.
- Snapshot: `VACUUM INTO` under snapshot quiescence, then milestone 2's `prepare_snapshot_copy` (empties Local tables and resets Local columns to their materialization values), then the existing archive allowlist, which already leaves out `vault.lock` and `.database-rollbacks/`. Compacted rows ship as they are; the snapshot already holds every version, tombstone, and context, which is why whole-database bootstrap is correct after compaction.
- Admission: `admit_bootstrap` stops requiring an untouched Android starter. `ensure_compatible` stays, because each device now migrates its own file and the only cross-build schema exposure is a snapshot from a newer build.
- Desktop first link: the user picks the folder for the linked vault in the enrollment flow; the default is the platform default when it is empty, otherwise an empty sibling named after the vault. The desktop's previous active vault folder is never touched and stays in the vault picker.
- Android first link: an untouched starter is replaced as today. Any other vault is first written as a portable backup into app-private `replaced-vaults/` with the existing Android backup format, then replaced. Settings > Data lists these copies with Export to Downloads and Delete; at most two are kept, and bootstrap refuses with a storage message when the backup cannot be written.
- Re-bootstrap: when `bootstrap_required` arrives, the device pushes its own operations first (the hub accepts them), then shows "needs a fresh copy" with one action. It is not automatic, because replacement pauses editing and may download a large archive on mobile data. Until then the device keeps writing and pushing but receives nothing. Activation seals pending captures, exports carry-forward, keeps a rollback copy with the current retention, and keeps the writer when the high-water check passes.
- Managed project folders are copied once by the snapshot on desktop and are device-local afterwards.

### Ownership removal

- `vault/ownership.rs` is deleted. A `vault/write_fence.rs` keeps the managed write permit and fence (permits no longer check anything, the busy error reads "the vault is being replaced"), because bootstrap activation, backup restore, and rollback restore still drain managed writes. `acquire_replicated_asset_write` merges into `acquire_managed_write`.
- Every connection is `ReadWrite` and runs migrations at open. `VaultDatabaseAccess`, `DatabaseAccessMode::{ReadOnly, Guarded}`, `connect_path_guarded`, `guards.rs`, mixed mode, and the per-domain replica write flags are deleted. Every `can_write`, `replicated_writes`, and `owner_required` check becomes unconditional.
- Handoff: `source.rs` keeps only the call into `ganbaru-sync-replica` snapshot creation, source quiescence and `coordinator/incoming.rs` (uploads) are deleted, `coordinator/outgoing.rs` serves bootstrap, and `receiver.rs` becomes the bootstrap client without the refresh repoll (`receiver.rs:13-14`). Android reconciliation keeps only the pass trigger.
- Protocol: `BundlePending`, `UploadBundle`, `BundleStaged`, `CommitStagedOwnership`, `OwnershipGrant`, `CommitUploadedOwnership`, `RefreshRequest`, and `RefreshStatus` are deleted. `CoordinatorPin.generation` is deleted; pairing types ignore unknown fields, so existing pairing files still load and `PAIRING_STATE_SCHEMA_VERSION` stays.
- Commands: `handoff_recover_local_copy` is deleted, and `handoff_request_owner_bundle` is replaced by `handoff_request_bootstrap`. `vault_ownership_status` becomes `vault_link_status` with `{ linked, coordinatorDeviceId, bootstrap: none | required | running }`.
- `vault-ownership.json` is no longer read or written. It is not deleted, because a cleanup path would be a migration shim for device state with no external users.
- Contacts: received requests persist on the coordinator unconditionally. The person identity is created by the coordinator and reaches other devices in the bootstrap snapshot or as a replicated row.
- Frontend: the ownership store, prompt, prompt tests, ownership and refresh workflow branches, `DesktopBundleReceiveMode`, and `OwnerBundlePurpose` are deleted. The handoff panel keeps pairing, linked devices, and bootstrap progress.

### Cross-process fence

- The vault root holds `vault.lock`. The process opens it and takes `File::try_lock()` (exclusive, advisory) before the first pool connects, keeps the handle in a process-global registry for as long as that vault is active, and releases it when switching vaults. One handle per process, because `flock` conflicts between two descriptors of the same process; the Android headless pass and the UI share it.
- `WouldBlock` refuses to open with "this vault is open in another Ganbaru app" (typed error, localized). `Unsupported` opens the vault and shows a persistent Data settings warning that another process cannot be detected on this filesystem.
- Replacement keeps the fence across the swap: lock `staging/vault.lock`, swap directories, then release the old handle, so no other process can open the new vault in between.
- `vault.lock` stays outside the archive allowlist, so backups and snapshots never carry it (a regression test pins it); the untouched-starter check reads app state and is unaffected. `tauri-plugin-single-instance` stays; it cannot cover a production and a development build pointed at one folder.

### Managed asset collection

- One sweep for every asset class registered in the manifest. The live set is `asset_refs` over every live row of every table that declares references (Replicated and Local), over every retained version in `sync_register_versions` (losers, conflicts, recovery entries, held deletes), plus the in-memory set of in-flight uploads and downloads.
- A file outside the live set and older than the existing Quick notes grace period is deleted. The Quick notes sweep becomes this sweep.
- Backups have no persistent pins: an archive is self-contained once written. The sweep and archive creation (backups, bootstrap snapshots, and milestone 12's encrypted backups) share one async mutex, so a sweep never runs while an archive is being written. The sweep, the mutex, and archive creation live in `ganbaru-sync-replica`, so the anchor's backup command and milestone 12's encrypted backups take the mutex without Tauri. The app runs the sweep holding a managed write permit, so replacement fences it.
- It runs once per day after a successful sync pass and after compaction, off the UI thread.

### Data settings

- This device: linked, downloading a copy (with progress), needs a fresh copy (with the action), or the unsupported-lock warning.
- Coordinator view of linked devices: last acknowledgement time, a dormant badge after 90 days explaining that the device will download a fresh copy when it returns, and, when compaction has been held for more than 30 days, which device holds it back and why (no writer yet, newer app version holding operations, or not seen recently).
- Android: replaced vault backups with Export and Delete.
- Strings go in `sync.ts` (shared with milestones 2, 3, 11, and 12) as additive keys; ownership keys are removed.

### Protocol versions

- Step 2 moves `PROTOCOL_VERSION` to the next protocol version for kind `3` and the two `SyncState` fields; older builds would otherwise hold kind `3` as `newer_format`.
- Step 6 moves it to the next protocol version again for the deleted messages and fields. Both follow the README wire versions rule: each takes the next free number when it merges.

## Testing

- Contracts: kind `3` codec round trip, refusal of a body or changes, consecutive acknowledgements past 256 dependencies.
- Engine: acknowledgement trigger fires on kind `0` and kind `4` from another writer and never on kind `3` (two-writer ping-pong test runs 1000 passes and seals a bounded number); keepalive after 7 days with an injected clock; stability with a writerless member, an unknown context, a retired predecessor, a revoked device, a dormant device, and the coordinator's applied vector; a dormant device returning lowers `S` but never `compacted_seq`.
- Compaction: genesis, revoke, head, and kind `4` envelopes kept; hydration for pending capture, hash mismatch, and missing row, and no hydration for a matching row; tombstone contexts unchanged after compaction; own cap at the probe-confirmed head; batch bound; crash between batches leaves a consistent prefix; `bootstrap_required` from the hello vector and the pull backstop; carry-forward abort on a compacted gap.
- Simulation (the existing harness, extended by milestone 2): add acknowledgements, a coordinator replica computing stability, compaction on every replica, a long-offline replica that becomes dormant and re-bootstraps with carry-forward, and the invariant that every replica's materialized state equals a replica that never compacted.
- Migration: `compacted_seq` default on existing rows, fresh-vault schema test in `crates/ganbaru-db/src/tests/`.
- Fence: second open in another process (spawned test binary) fails with `WouldBlock`; release on vault switch; replacement keeps the lock across the swap; archive excludes the lock file.
- Bootstrap: admission on desktop and Android with and without a previous vault, the Android replaced-vault backup, compatibility refusal, resume after interruption, activation crash recovery through the existing staging recovery.
- Assets: live set from rows, retained versions, and in-flight transfers; grace period; sweep blocked during archive creation, tested in `ganbaru-sync-replica` without Tauri.
- Frontend: link status mapping, Data settings states, and removal of ownership branches in `handoff-workflow.test.ts` and `state.test.ts`.

## Work breakdown

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-vault-lock` | `vault.lock` registry, typed error, archive exclusion test, swap ordering, Data settings warning. Needs nothing. | `cargo test -p ganbaru-tauri-app --lib -j 4 vault_lock -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 backup -- --test-threads=4` |
| 2 | `feat/sync-acknowledgements` | Kind `3` contracts and sealing, writer on first pass, stability and dormancy on the coordinator, `SyncState.stable` and `bootstrap_required`, next protocol version, simulation acknowledgements. Needs milestone 2. | `cargo test -p ganbaru-sync-contracts --lib -j 4 op -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 acknowledg -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 stability -- --test-threads=4`, `cargo test -p ganbaru-handoff --lib -j 4 sync -- --test-threads=4` |
| 3 | `feat/sync-compaction` | `compacted_seq` migration, compaction with hydration, own cap, serving refusal, carry-forward abort, re-bootstrap trigger over the existing refresh replacement, simulation dormancy, Data settings device states. | `cargo test -p ganbaru-sync --lib -j 4 compaction -- --test-threads=4`, `cargo test -p ganbaru-db --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-sync-replica --lib -j 4 carry_forward -- --test-threads=4` |
| 4 | `feat/sync-asset-collection` | Generic sweep over every class, Quick notes sweep folded in, sweep, archive mutex, and archive creation (backup and bootstrap snapshot) in `ganbaru-sync-replica`. | `cargo test -p ganbaru-sync-replica --lib -j 4 assets -- --test-threads=4`, `cargo test -p ganbaru-sync-replica --lib -j 4 backup -- --test-threads=4`, `cargo test -p ganbaru-quick-notes --lib -j 4 images -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 backup -- --test-threads=4` |
| 5 | `feat/sync-bootstrap` | Bootstrap for any first link, desktop folder choice, Android replaced-vault backups, re-bootstrap moves from refresh to bootstrap, serving from member listeners. Needs milestones 4 to 9 and milestone 11 step 9. | `cargo test -p ganbaru-tauri-app --lib -j 4 receiver -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 backup -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/vault/handoff-workflow.test.ts --maxWorkers=1` |
| 6 | `refactor/sync-retire-ownership` | Delete ownership, guards, mixed mode, write flags, `owner_required`, transfer and refresh, uploads, prompt, and messages; write fence module; link status command; next protocol version; headless pass reads only the vault path. | `cargo test -p ganbaru-tauri-app --lib -j 4 vault -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-db --lib -j 4 schema -- --test-threads=4`, `cargo check -p ganbaru --bin ganbaru -j 4`, `pnpm --dir apps/client exec vitest run src/lib/vault/state.test.ts --maxWorkers=1` |
| 7 | `test/sync-retirement-acceptance` | Simulation seed soak, physical acceptance record, delivery table and feature status rows. Needs 5 and 6. | `pnpm -w run validate` |

Steps 1 to 4 land in order on the engine track while domain milestones finish. Step 6 is mostly deletions and stays one step, because splitting off the frontend and i18n removal would need stub types for the backend removal to compile. Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

## Documentation

- `docs/data/sync.md`: replace the whole-vault handoff section (9-63) with "Linking and bootstrap" (pairing 13-25 stays, ownership 27-33 and transfer and refresh 35-49 are deleted, replacing and recovering 51-57 becomes re-bootstrap and the Android replaced-vault backup); delete mixed mode (69-76); writers (85-94) add a writer per linked installation and kind `3`; exchange (96-116) adds `stable`, `bootstrap_required`, and the protocol version; carry-forward (118-125) names re-bootstrap; line 215 says the identity arrives in the bootstrap snapshot; lines 219-227 state the scrubbed whole-database snapshot, compaction after causal stability with the anchor's acknowledgement, 90-day dormancy, tombstones kept forever, asset collection, and the `vault.lock` fence in place of the generation fence; line 231 lists the Data settings states; delivery rows 241 and 243 to 244 drop handoff and record compaction. This closes the README "Docs differ" rows that name milestone 10.
- `docs/data/schema/sync.md`: line 24 says envelopes are kept whole until compaction empties them, with genesis, revoke, head, and kind `4` envelopes always whole; add `compacted_seq`; delete the guard text (38) and update the device-local invariant (30) for `vault.lock`.
- `docs/architecture/decisions/sync-engine.md`: replace the mixed mode decision with "no owner, snapshot bootstrap", and replace the accepted limitation at line 45 with compaction and its stability rule.
- `docs/testing/vault-handoff.md` becomes `docs/testing/device-linking.md` with the acceptance script below; update links to it (`docs/testing/README.md`, `docs/testing/android.md`).
- Rewrite every remaining owner, ownership, read-only replica, refresh, and handoff statement to the linked-device model in: `docs/ROADMAP.md`, `docs/features/README.md` (sync and handoff status rows), `docs/features/collaboration/README.md`, `docs/features/pomodoro/README.md`, `docs/features/music/playback.md`, `docs/features/distractions/usage-limits.md`, `docs/features/chat/execution-and-workspace.md`, `docs/features/localization.md`, `docs/algorithms/pomodoro/focus-authority.md`, `docs/platforms/desktop/README.md`, `docs/platforms/android/` (README, architecture and capabilities, native services and data, validation and release), `docs/architecture/frontend.md`, `docs/architecture/native-backend.md`, `docs/architecture/integrations.md`, `docs/data/README.md`, `docs/data/invariants.md`, `docs/data/security/README.md`, `docs/data/security/application-boundaries.md`, `docs/data/security/chat.md`, `docs/data/schema/chat.md`, `docs/data/schema/supporting-domains.md`, `docs/testing/music.md`, and `docs/testing/distractions.md`. Most need one sentence.
- `docs/data/security/network-and-privacy.md`: the listener's message set without uploads, transfers, and refresh.
- `AGENTS.md`: the `src/vault/handoff/` line becomes "LAN device linking and snapshot bootstrap"; the Sync key convention drops "single-writer whole-vault handoff" and "The rest of the vault stays single-writer on replicas"; the vault folder map adds `vault.lock`.

## Manual acceptance

Two desktops (A is the coordinator, B is linked) and one Android phone (P), release builds. All domains are converted.

| Scenario | Expected |
| --- | --- |
| Link a new desktop C that has its own vault | C picks an empty folder, downloads the snapshot, opens read-write; C's old vault is unchanged in the picker. |
| Link P while it holds a different vault | The old vault appears under replaced vault backups and exports to Downloads; the linked vault opens read-write. |
| Edit Calendar, Notes, Projects, Chat organization, and Music on B and P at once | Every edit converges on all devices; no prompt anywhere. |
| Open the vault folder from a production and a development build of A | The second build refuses with the "open in another Ganbaru app" message. |
| Leave B idle for two weeks of acknowledgements (clock forward on B) | B never needs a fresh copy; A's Data settings show B's last acknowledgement. |
| Turn P off, set A's clock forward 91 days, run a pass | A shows P as dormant; compaction advances past P. |
| P edits offline, then reconnects after the dormancy run | P's edits reach A and B; P shows "needs a fresh copy"; after the action, P has every change and its offline edits are kept. |
| Interrupt a bootstrap download on P (airplane mode) | Resumes from the last offset; no partial activation. |
| Kill the app during bootstrap activation on C | Next start recovers through staging recovery; C opens either the old state or the new vault, never a mix. |
| B on an older app version | Hello refuses with the update message; nothing is held or lost after updating. |
| Delete a Quick note image and empty trash on A | After the grace period and the next daily sweep, the image file is gone on every device; a conflict version that references an image keeps it. |
| Unlink B on A | B's later edits never reach A or P; B stops counting for stability. |

### Acceptance results

Recorded by the maintainer in step 7.

## Performance budgets

Measured on the dense fixtures of milestones 5 to 9 in the release profile and on the acceptance devices.

| Measure | Budget |
| --- | --- |
| Compaction transaction of 256 operations | At most 50 ms on P, 15 ms on A |
| Stability computation with 64 writers | At most 5 ms on A |
| Acknowledgements per device while another device edits continuously | At most 6 per hour |
| `sync_ops` bytes per compacted operation (row plus context, 10 writers) | At most 400 bytes |
| Asset sweep over 10,000 files | At most 2 s on A, off the UI thread |
| Database size after compaction on the dense fixture | Recorded before and after; body bytes of stable operations are zero |

## Risks

| Risk | Mitigation |
| --- | --- |
| A device holding operations as `newer_manifest` or `newer_format` acknowledges nothing past them, so compaction stalls for everyone until it updates or goes dormant | Data settings name the device and the reason after 30 days; dormancy bounds the stall at 90 days. |
| A linked device without the person key has no writer and holds compaction at zero | Same Data settings line; key release already runs over the link. |
| Compacting a body whose elided winner is still needed loses a value | Hydration rule with hash check, simulation invariant against a never-compacting replica, and milestone 2's hydration tests. |
| Coordinator clock jumps forward and marks every device dormant | Only compaction advances; returning devices push first and re-bootstrap with carry-forward, so nothing is lost, only an extra download. |
| A large `projects/` folder makes the first desktop bootstrap slow | Resumable download and progress; folder exclusions are Deferred. |
| Lock files on network filesystems | `Unsupported` opens with a warning; SQLite WAL is already unsafe there and the docs say so. |
| Step 6 is a large deletion touching many modules | Mechanical: each gate becomes unconditional; the compiler finds every caller once `ownership.rs` is gone. |

## Deferred

- Deleting compacted `sync_ops` rows and keeping contexts only for tombstoning operations, after the size measurements show they matter.
- Exclusions for managed project folder contents in the bootstrap archive.
- Combining two vaults through an import preview.
- `VACUUM` or `auto_vacuum` for shrinking the file after large compactions.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- **Runtime:** That `File::try_lock` maps to `flock` on Linux and Android and to `LockFileEx` on Windows, and what it returns on FUSE and SMB mounts. Rule: anything other than `WouldBlock` and success is treated as `Unsupported`.
- Settled from source: no Kotlin code in `gen/android` or the `ganbaru-mobile-*` plugins references `projects/`. The Android snapshot download skips `projects/` contents and creates empty managed folders.
- Settled from source: `BundleStaged` acknowledges uploads only (`vault/handoff/transport.rs:394`, `transport/server.rs:766`); the download path never sends it, so it is deleted with the upload messages.
- **Earlier step:** Whether the replacement path seals pending captures before the carry-forward export. Rule: if not, step 3 seals them inside the same quiescence before exporting.
- **Earlier step:** Whether any domain milestone left a table `Unconverted` (for example a Chat or Notes table kept device-only by mistake). Rule: step 6 does not start until `VAULT_MANIFEST` has no `Unconverted` table; such a table is classified Local or converted by its domain milestone first.

## Progress

- [x] Step 1: cross-process vault fence.
  - Date: 2026-10-22. Branch: `feat/sync-vault-lock` (from `feat/sync` at 1652c89f). Commit: 3c032ccb.
  - Runtime implementation check: primary branch held on Linux (ext4): `File::try_lock` returns `WouldBlock` across processes (spawned child test) and succeeds otherwise; any `TryLockError::Error` is treated as `Unsupported` and logged. Windows (`LockFileEx`), Android, FUSE, and SMB behavior remain for the acceptance matrix.
  - Decisions: the lock is taken eagerly at vault selection (`select_quiesced_vault`, so a folder open elsewhere is refused before it becomes active) and again lazily before every vault pool connect (`connect_database` for the vault only, `connect_vault_for_sync_in`, `connect_active_vault_read_only`); the benchmark database never locks. Closing pools keeps the lock; switching vaults, replacement, and `RunEvent::Exit` (desktop and mobile, so a restart's new process finds the vault free) release it. Roots compare by canonical path. One process-global registry holds at most one handle; a refused switch keeps the previous vault fenced.
  - Replacement: on Unix `replace_with` locks `staging/vault.lock`, swaps, then drops the old handle. On Windows both handles close for the swap and the target is relocked after (directories holding open files cannot be renamed); unverified, recorded for the acceptance matrix.
  - `finish_replacement` discards the old `vault.lock` so it never forces a "before replacement" sibling. Archive exclusion pinned by `bundle_layout_admits_...` and `backup_archive_uses_consistent_database_snapshot`.
  - Frontend: `vault_lock_status` command, `getVaultLockStatus`, `dataFolderError.openElsewhere`, `sync.lockUnsupported` warning shown by `VaultLockWarning` in desktop and mobile Data settings.
- [x] Step 2: acknowledgements, stability, dormancy, and the next protocol version.
  - 2026-10-22, branch `feat/sync-acknowledgements` from `feat/sync` (merged `feat/sync` fb3d1d07 in as 570d5597 for the vault lock).
  - Commits: 7fce1450 `feat(sync): seal signed acknowledgements and writer geneses on every pass`; b603d084 `feat(sync): derive coordinator stability and dormancy from device acknowledgements`; e59d62db `feat(sync): report stability to clients under protocol 9`; 3b637341 `test(sync): assert a pushed acknowledgement settles the headless pass`. Full `ganbaru-sync --lib` runs in about 86 s (sims 43 s, the rest 48 s, in parallel).
  - Implementation checks: none of the plan's implementation checks applies to step 2.
  - Kind `3` keeps `FORMAT_VERSION` 2: old builds already hold unknown kinds as `newer_format`, and protocol 9 refuses old peers before any exchange, so a format bump would add nothing.
  - Acknowledgement trigger: any applied operation of another writer that is not kind `3` beyond the writer's own head context; at most once per 10 minutes measured from the writer's newest kind `3` receipt time; keepalive when the newest own operation is 7 days old. The decision runs before the engine write, so an idle pass commits nothing and does not wake waits.
  - Genesis on first pass: `Engine::acknowledge` seals the genesis when the space does not know the writer, so the service's `seal_pending` early return needs no change; the service calls it every pass after apply, rotating once on `WriterInactive` like `seal`.
  - A client that sealed an acknowledgement after its exchange runs the next pass at once (`Next::Continue`, headless `more`) so the hub learns of it without waiting for the fallback interval.
  - Stability is computed only in the coordinator service pass (not in `hub.push`), published to the hub through a shared `HubStability` cell keyed by vault id, cleared on vault switch and on client role, and met with the hub's stored vector in every `SyncState`. Not persisted.
  - Device acknowledgement: context of the newest stored operation with a known (non-empty) context of the device's newest active writer by creation time, with that writer's own entry set to the operation; no active writer or no known context acknowledges nothing.
  - Dormancy: newest `stored_at_ms` among the device's writers' operations; without operations, `linked_at_ms`, or the coordinator's first observation (in service memory, reset on vault switch) when it is 0. Exactly 90 days is not dormant. Because the first observation is not persisted, it also resets on every coordinator restart, so a device with `linked_at_ms` 0 and no operations effectively never turns dormant on a desktop that restarts normally and holds stability at nothing; step 3 must decide whether to persist the first observation before compaction depends on it.
  - Counted devices: `linked_peers_for(vault_id)` from the pairing state plus the coordinator through its applied vector.
  - Clients do not keep the `stable` they receive yet; step 3 stores it when spokes start compacting. `bootstrap_required` is always false and `SyncRefusalCode::BootstrapRequired` is never sent until step 3.
  - Simulation: every delivery is followed by `acknowledge` (crashing at `acknowledge.before_commit` or `seal.reserve` on a tenth of calls, since most calls seal nothing and snapshots doubled the run), occasional clock jumps of 10 minutes to 7 days, hub stability over all replicas checked after exchanges (within every replica's and the hub's applied vector, nobody dormant), and per-writer acknowledgement spacing checked at the end. `failpoints_roll_back_every_entry_point` covers `acknowledge.before_commit` deterministically.
  - Physical acceptance: protocol 9 requires every linked device to update together (older builds are refused with an update request); add an all-devices-update check to the milestone's physical acceptance run.
- [x] Step 3: compaction and re-bootstrap signal.
  - Date: 2026-10-22. Branch: `feat/sync-compaction` (from `feat/sync` 45e78d3a, merged with `feat/sync` 5fd25c24 in 1c3210cb).
  - Commits: 2cbda620 test(sync): shrink large seal fixtures with longest keys; 1e8c863f feat(sync): compact stable operation bodies; 0a0f1110 feat(sync): signal bootstrap for devices behind compacted operations; 1c3210cb merge: sync base into compaction; 71ab33bf test(sync): check sim invariants only where a step changed state; 8e8dfb42 feat(sync): show dormant, relink, and bootstrap device states; 4df28d9f docs(sync): describe operation compaction and bootstrap signal; fc665e78 fix(sync): clear uncounted devices once every device counts again; fd3f7bcf test(sync): check sim invariants after compaction steps (also qualifies the remaining roadmap item as asset collection).
  - Merge note: the projects conversion pass from 5fd25c24 affects tauri-app vault setups; this step adds no tauri-app test that creates a vault, and the replica crate tests do not replicate projects. All suites pass after the merge.
  - Implementation check branches: compaction per writer up to min(limit, applied, stored minus 1); kinds 0 and 3 emptied, head, genesis, revoke, and kind 4 kept; every elided version of a compacted operation hydrated first; `ops_after` returns `Compacted` when the peer is behind `compacted_seq`; empty envelope above `compacted_seq` is corrupt; own active writer capped at the last hello-probe-confirmed head; batches of 256 within 200 ms per committed batch, failpoint `compact_batch`; client stores `stable` in `sync_stable`; hub sends `bootstrap_required` in `SyncState` and as a non-retryable pull error, and maps a fork inside the compacted range to `SyncRefusalCode::BootstrapRequired`; carry-forward aborts when the replacement lacks compacted operations.
  - Decision: hydration covers every elided version of a compacted operation (all kinds that carry versions), not only winners; the "matching row stays elided" test became its inverse (a matching row is hydrated before its body drops), since a later remote change could otherwise leave the version unreadable.
  - Decision: `sync_stable` lives in the same migration as `sync_writers.compacted_seq` (20261023030543_sync_compaction.sql); no manifest or protocol bump.
  - Decision (step 2 wording): dormancy no longer uses an in-memory first observation. A linked device with `linked_at_ms <= 0` is neither counted nor dormant (`RelinkRequired`), does not hold stability, and Data settings asks to link it again.
  - Decision: the simulation keeps replica 0 as a never-compacting replica and asserts every compacting replica's domain state equals it after quiescence; 40 percent of exchange steps compact.
  - Decision: a client that receives `bootstrap_required` ends the pass in the error state with `SyncErrorCode::BootstrapRequired` after pushing; it pulls nothing.
  - Note: a reseal after a fork truncates above the fork point, which can leave the new head at a compacted sequence; compaction never empties a head, so this is consistent but worth a later look.
  - Note: the sim acknowledgement-interval assertion now accepts acknowledgements stored in the same seal (a documented multi-header acknowledgement, exposed at seed 39 by the new RNG path).
  - Test time, `cargo test -p ganbaru-sync --lib -j 4 -- --test-threads=4`: 87.8 s before step 3; 76.7 s after shrinking seal fixtures; 86.5 s with compaction; 95.8 s after merging 5fd25c24; 87.5 s (87.9 s wall) after checking sim invariants only on the acting replica and hub. Sim compaction costs about 11 percent of a sim shard. `ganbaru-sync-replica --lib` takes 211 s after the merge (104 s before it), mostly from base changes.
  - Performance budgets pending: a synthetic test compacted a full batch of 255 operations in 32.5 ms (debug profile), freeing about 245 bytes per operation; release numbers on target hardware are not recorded yet.
  - Checks passed: ganbaru-sync lib (214), compaction (13), ganbaru-sync-replica lib (50), ganbaru-db sync (6), ganbaru-tauri-app sync (38), clippy -D warnings on ganbaru-sync, ganbaru-sync-replica, ganbaru-db, ganbaru-tauri-app (all targets), cargo fmt --check, cargo check -p ganbaru --bin ganbaru, pnpm --dir apps/client run check, vitest api/sync, sync/status, catalog-shape.
- [x] Step 4: managed asset collection.
  - Date: 2026-10-23. Branch `feat/sync-asset-collection` from feat/sync 713d74f0, signed-merged with local feat/sync 8bd68f87 (no conflicts, merge 24f1e9a0).
  - Commits: 1d59a409 `test(sync): copy a migrated template for replica test vaults`, 19ad29e2 `refactor(sync): move vault archive creation into the replica crate`, 4cdba2b9 `feat(sync): sweep unreferenced managed assets of every registered class`, ae364947 `refactor(quick-notes): fold image collection into the sync asset sweep`, 33024f52 `docs(sync): describe managed asset collection and the archive mutex`, 23dfd14d `fix(sync): claim the daily asset sweep only once the vault is writable` (after the merge).
  - Sweep: `ganbaru_sync_replica::assets::collect_unreferenced_assets` enumerates classes from the manifest (classes without asset tables are skipped), takes the live set from `Engine::referenced_assets` (live rows plus retained versions, which back conflicts and recovery), and deletes only regular `{sha256}.{ext}` files older than 24 hours that no transfer of this process touched within 24 hours. Temporary staging files, foreign names, and unregistered directories stay. Tests cover an orphaned replaced profile picture alongside Quick notes images.
  - In-flight transfers: a process-wide `InFlightAssets` registry is touched by the content-addressed store on receive, keep, and confirm; no persistent pins.
  - Scheduling: the app runs the sweep after a successful sync pass, at most once a day per vault per process (`sync/asset_collection.rs`, injectable due claim with a unit test). The service starts for every vault and an unlinked pass still succeeds, so solo vaults collect too; the daily claim is taken only once a writable pool is available. The frontend Quick notes trash purge no longer collects images and the `quick_notes_collect_images` command and permission are removed.
  - Archive mutex: `ArchiveGate` (process-wide tokio mutex) is held by the sweep and by backup and handoff snapshot archive creation, so archives never read a half-collected set.
  - Archives: backup and snapshot creation (vacuum, layout filter, sidecar exclusion, zip writing, snapshot sync-capture clearing) moved to `ganbaru_sync_replica::backup`; the app keeps thin wrappers and aliases the layout constants. zip was already a workspace dependency used by the app; the replica crate now depends on it too.
  - No migration and no manifest change; VAULT_MANIFEST_VERSION untouched.
  - Replica test timing: 50 tests took 212.17 s because each `create_vault` ran the full migration chain (about 3 s in debug after the Music capture and compaction migrations). Test vaults now copy a migrated template serialized once per process; the same 50 tests take 20.85 s, coverage unchanged since `connect_path` still validates the catalog and migration history. After this step 58 replica tests run in 21.4 s.
  - Checks (after merge): replica lib all 58 passed, quick-notes images 4 passed, tauri-app backup 20 passed, clippy all targets on the three crates clean, cargo fmt clean, Vitest trash-purge 4 passed, client check 0 errors. The later claim fix passed fmt, clippy, and its app test.
  - Not compiled: mobile-gated edits (`runtime/mobile.rs`, the Android archive call in `vault/backup.rs`), since no Android target is installed on this host.
- [ ] Step 5: snapshot bootstrap for every first link and re-bootstrap.
- [ ] Step 6: ownership, guards, transfer, and refresh removed, and the next protocol version.
- [ ] Step 7: simulation soak, physical acceptance, and status rows.
