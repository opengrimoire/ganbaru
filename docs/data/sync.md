# Device linking and synchronization

**Status: Partial.** The vault runs in mixed mode. Quick notes replicate concurrently between linked devices as signed operations through the coordinator desktop, with conflicts and recovery entries; this is implemented in source. Every other domain keeps single-writer whole-vault handoff: secure LAN pairing, one administration desktop, explicit write ownership, read-only refresh, and combined distraction usage accounting. Physical multi-device acceptance is pending for both. Concurrent replication of the remaining domains, Notes text and tree merging, anchored authority with LAN peer exchange, operation encryption, and delivery beyond the LAN are planned.

This contract links one person's devices; multi-person sharing is later work. The phone must provide full offline access to portable Notes, Projects, Calendar, and other synchronized content. Focus execution has its own controller and evidence rules in [Focus authority](../algorithms/pomodoro/focus-authority.md).

Related documents: [Sync engine decision](../architecture/decisions/sync-engine.md) explains why the engine is built this way, [Sync topology and authority](../architecture/decisions/sync-topology-and-authority.md) why authority is ordered by one anchor per space, [Sync merge rules](../algorithms/sync/README.md) owns merge semantics and worked examples, and [Sync schema](schema/sync.md) owns the engine tables.

## Implemented local whole-vault handoff

Source: `apps/client/src-tauri/app/src/vault/handoff/`, with the wire protocol, pairing state, and Linux firewall authorization in `crates/ganbaru-handoff/`. Whole-vault handoff carries every domain that is not yet replicated, and the full vault including the operation log on refresh and transfer.

### Pairing and transport

The administration desktop runs one embedded coordinator on a private LAN address and coordinates up to 32 enrolled desktops and phones for one vault. Phones link by scanning a short-lived, single-use QR invitation; desktops paste the complete textual invitation. The QR form is a compact binary projection because module density limits reliable scanning distance. Every device authenticates with its own certificate and pins the coordinator fingerprint from the invitation, and mutual TLS protects all later traffic. There is no account, hosted service, relay, internet traversal, or general remote procedure interface.

Enrollment is idempotent for the same device identity and coordinator. A linked client must unlink before accepting an invitation from a different coordinator, vault, or certificate. A reinstalled app has a new device identity and stays a separate membership until the old entry is removed. Memberships are device-local and recorded per vault: only the active vault's linked devices count, so a new or switched vault starts unlinked. Enrolling a device under another vault replaces its earlier membership, and at the stored limit, enrollment revokes the oldest memberships of other vaults first.

On Linux, when an active host firewall blocks the coordinator, the app explains the request and opens the operating system authorization agent; it never receives the administrator password. Installed Debian and RPM packages use a named PolicyKit action whose privileged entry point revalidates every argument; development and portable builds use a generic authorization fallback. The app manages only its own rule (TCP 43821 for production, 43822 for development so both can run) scoped to the current private subnet, interface, and address. It never disables the firewall, and the user can remove its rules from Data settings.

Every invitation, transfer, owner poll, and archive declares the handoff protocol and a fingerprint of the embedded SQLite migration set. A mismatch is rejected before snapshot preparation with a request to update both devices. Package versions are included only for diagnosis, because equal versions do not prove an equal data format.

### Ownership

Ownership is explicit and single-writer. Device-local records hold the current owner, a monotonically increasing generation, transfer phase, and resumable transfer state. SQLite connections and managed file writes enforce the current role centrally. The coordinator serializes transfers: the current owner first returns a committed snapshot, then the coordinator grants a later generation to the requester. For handoff-carried domains, other replicas stay read-only throughout, so those domains need no conflict resolution. Replicated domains are writable on every linked device; see [mixed mode](#mixed-mode). A stale generation or a grant from outside the pinned coordinator never restores write access.

An active Chat run or Pomodoro run blocks transfer, and local playback stops before the source is frozen. Before queuing a request for a remote owner, the coordinator requires a recent owner heartbeat; an unavailable owner ends the attempt with ownership unchanged.

Ownership persistence distinguishes failure before replacement from uncertain durability after it. If the directory sync after replacement fails, ownership reads, database access, and managed writes stay blocked until the state reloads successfully; the old writable owner is never restored only in memory. Unix builds synchronize the containing directory before reporting success; other platforms still need physical crash and power-loss acceptance.

### Transfer and refresh

Ownership transfer and read-only refresh share one bounded path:

1. Fence new writes, drain current writes, isolate database access, and create a consistent SQLite snapshot.
2. Archive the complete portable vault without live WAL or SHM files and stream it into device-local staging.
3. Validate identities, generation, bounds, archive hash, paths, required files, migration history, and SQLite integrity.
4. For a transfer, commit the new generation; then atomically activate staging, reopen through normal startup, and acknowledge.
5. Keep post-commit state until acknowledgement, so a restart retries the same activation instead of granting a second owner.

Failure or cancellation before the transfer is durably stored restores pre-commit ownership before fences are released. Committed ownership is never rolled back by preparation cleanup.

The archive includes `vault.json`, portable `config.json`, the SQLite snapshot, managed project folders, and managed assets. External music and project folders, credentials, executable and provider paths, operating-system permissions, the active-vault pointer, device keys, pairing and ownership records, and live process state stay device-local.

Each non-owner keeps its last activated copy for read-only browsing and refreshes through the coordinator on start, resume, LAN reconnect, or explicit request. A read-only coordinator first fetches a fresh snapshot from the owner and relays it. Reachable clients keep a bounded authenticated long poll open so an explicit request does not wait for the periodic reconnect interval; disconnected clients retry with slower backoff. After activation, Android reruns startup reconciliation, rebuilding Calendar notification schedules and republishing distraction rules. A disconnected phone keeps its existing alarms and rules until it refreshes. Whole-vault refresh is a lifecycle or explicit operation and must never run after every mutation as a substitute for incremental sync.

### Replacing and recovering vaults

Linking never merges two independent vaults. Before the first transfer replaces an independent local vault, the app asks for confirmation and states where the old copy will be kept: Android writes a complete portable backup to Downloads, and desktop keeps the previous vault in a visible sibling folder. Failing to preserve it aborts activation. The only exception is an untouched Android starter vault created during first-use onboarding, which may become a replica directly.

Unlinking never silently picks a new owner. The owner can remove an unreachable read-only device immediately but not an unreachable device that owns the vault. The coordinator keeps a bounded revocation record; on reconnect the removed device is denied, clears its link, and keeps its copy read-only. Network and TLS failures never clear membership. If the owner is permanently lost, an explicit recovery action unlinks the devices and advances the local copy to a new standalone generation; the copies do not merge.

### Distraction usage exception

Distraction usage is the only write from inactive devices. Browser, desktop, and Android usage enters a bounded device-local spool with stable device-scoped sample IDs. Through the coordinator, the owner inserts samples transactionally and idempotently, acknowledges them, and returns the combined counter. Disconnected enforcement uses the last accepted combined value plus new local usage, and reconnection preserves the exact sum. Simultaneous disconnected use can temporarily exceed a combined limit because neither device knows the other's newest usage.

Whole-vault handoff itself is deliberately not the concurrent synchronization system: it has no operation journal, conflict resolution, or bidirectional editing. Those belong to the replication engine below.

## Implemented concurrent replication

Source: `crates/ganbaru-sync-contracts/`, `crates/ganbaru-sync/`, `crates/ganbaru-sync-replica/`, and `apps/client/src-tauri/app/src/sync/`. **Status: Implemented** in source for Quick notes and their tags; physical multi-device acceptance is pending. User-facing behavior is owned by [Quick notes](../features/quick-notes.md), and the network surface and key storage by [Network and privacy](security/network-and-privacy.md).

### Mixed mode

- Every table has an explicit classification in the engine manifest: replicated, owned children, derived, engine, or unconverted. A test fails on any unclassified table or column, and unknown tables are treated as unconverted.
- Replicated tables are writable on every linked device and travel as operations. Unconverted tables stay owner-only and travel through whole-vault handoff.
- The owner opens the vault read-write. A linked replica that is not in recovery and has no active transfer opens it guarded: a writable connection whose connection-scoped temporary triggers abort every write to an unconverted table with the normal read-only error, while replicated, derived, and engine tables stay writable. Guards never enter the vault file. Any other replica opens read-only.
- A guarded open validates the migration set first. A replica whose schema differs falls back to read-only until both devices run the same app version.
- Ownership status reports `replicatedWrites` for guarded replicas, so the UI can explain that Quick notes stay editable while the rest of the vault is read-only on that device.
- `config.json` and the distraction usage spool keep their owner-only rules.

### Capture and sealing

- Capture triggers, rendered from the manifest and verified against it by a conformance test, record which rows and field groups changed in the same transaction as the domain write. Imports, restores, background jobs, and every command are covered without per-command code.
- Value guard triggers enforce operation bounds on every local write, so any locally valid row seals into a valid operation. Published row keys are never reused: a deleted key cannot be created again.
- The sync service seals pending captures into signed operations from current row values after a short debounce, before every exchange, before snapshot preparation, and before applying remote operations. Saves never wait for sealing or the network.
- Sealing runs only while the vault is linked and a writer exists. A standalone vault keeps coalesced captures, at most one per changed row, and seals them when it first links.

### Writers

- Every installation signs with its own Ed25519 writer key, certified by the vault person key. The certificate binds the writer key, person key, device id, and creation time. A writer's first operation carries its certificate, so certificates travel with the log.
- Desktop keeps writer keys in the operating-system credential store. Android keeps them in app-private files excluded from backup and device transfer, the same interim protection as the person key.
- A device-local writer record holds a sequence reservation made durable before signing and the last committed sequence. At service start and after every database replacement, the record is checked against the operation log. A restored database, a database copied from another installation, or a lost key retires the writer and creates a successor, so one installation never signs a sequence twice.
- A full-disk clone copies the key itself, so two machines can sign the same sequence. The hub detects the diverging chain as a fork. The device keeps the agreeing prefix, rotates to a successor writer, and re-seals its later operations under it; both histories survive.
- Sealing waits while the person key is unavailable, status reports waiting for identity, and the key is requested from the coordinator as described in [Contacts and contact requests](#contacts-and-contact-requests).
- Removing a device makes the hub seal a revocation of that device's writers with a cutoff at the hub's stored sequence. Every replica refuses later operations of a revoked writer. This relies on the single hub; any second delivery path waits for the [anchored authority](#topology-and-authority).

### Exchange

The coordinator desktop is the hub, and clients never exchange directly. Protocol 5 adds sync requests to the existing mutual TLS listener with one request and one response per connection. Every request names the vault and device and is authenticated as an enrolled peer before it reaches the hub. The personal space id derives from the vault id.

| Request | Response | Purpose |
| --- | --- | --- |
| `SyncHello` | `SyncState` | Compare stored version vectors; a probe of the device's own head finds a clone whose chain has the same length as the hub's |
| `SyncPush` | `SyncPushResult` | Upload operations the hub lacks, stored up to the first refusal |
| `SyncPull` | `SyncOps` | Download operations the device lacks, in per-writer order |
| `SyncWait` | `SyncState` | Long poll that returns when the hub stores more, when a newer wait from the same device arrives, or before the control timeout |
| `SyncHashes` | `SyncHashList` | Consecutive operation hashes of one writer, to locate a fork point |

- Operations travel as base64url pages sized to fit the 1 MiB control frame; a page always holds at least one operation of the maximum size. One exchange moves a bounded amount of operations in each direction and applies what it pulled before the next exchange continues, so a long backlog never waits unapplied in full.
- Push refusals are `gap`, `fork`, `revoked`, `invalid`, `unknown_writer`, and `newer_format`. The hub accepts a writer's genesis only from the device its certificate names, and other operations only for writers whose genesis it already holds.
- The hub only stores and serves. Its own sync service applies what it stored, so apply runs in one code path on every device. An operation is acknowledged only after it is committed; a hub receipt means delivery, not that another device applied it.
- An operation from a newer format or manifest, or one that fails intrinsic validation, is stored, forwarded, and held with its reason visible in status. Everything causally after it waits until an app update releases it.
- Clients exchange after debounced commits, on start, on Android resume, on Sync now, and on a fallback interval, and keep a long poll open while linked. Failures back off up to five minutes, and changes queue locally while the hub is unreachable. Android exchanges only while the app process runs.

### Carry-forward across replacement

Whole-vault refresh, ownership transfer, and backup restore never lose operations:

- The source seals pending captures under the write fence before preparing a snapshot. Snapshots carry the full operation log and engine state; the copy drops pending captures, which stay in the source and seal there.
- Before activation replaces the vault, the receiver seals its own captures under the same fence and exports every operation it stores that the staged database lacks into a bounded device-local bundle. Captures that cannot be sealed refuse the replacement with an error that names unsynced Quick notes changes.
- The sync service imports the bundle into the new database before its writer check, then deletes it. Import skips operations already stored, so a crash between replacement and import loses nothing.
- After a backup restore, the writer check creates a successor writer, and captures in the restored database seal under it.

### Conflicts, recovery, and controls

- Valid operations converge to identical replicated state on every replica regardless of delivery order. Causality decides supersession; clocks only choose the displayed value among concurrent versions. The merge rules and worked examples are owned by [Sync merge rules](../algorithms/sync/README.md).
- Surfaced groups (Quick note title and body) keep concurrent versions as a conflict with a resolution. Other groups resolve silently and deterministically. A deletion concurrent with edits keeps those edits as a recovery entry that can be restored as a new note or discarded.
- Resolutions, restores, and discards are ordinary writes, so they replicate and converge like any edit.
- The Quick notes trash purge is a deletion like any other, so an edit made elsewhere to a purged note becomes a recovery entry.
- Data settings show the sync state, role, last exchange, pending and waiting changes, held operations with reasons, conflict counts, and recovery entries, with Sync now and Pause. Pause is device-local, stops exchanges, and keeps sealing.

## Target concurrent synchronization

### Architecture

SQLite remains the durable local store. Replicas exchange validated domain operations and immutable assets, never raw database pages, arbitrary SQL, or unclassified configuration. Foreground saves never wait for the network.

Collaborative text uses Rust Yrs with a compatible Yjs editor adapter. Binary CRDT state is canonical in SQLite; rich-text payloads and plain-text columns are deterministic projections. Text document identities are independent of block placement.

Local network linking works without an account or server. An optional user-hosted Rust relay stores opaque encrypted records for cross-network and asynchronous delivery. Hocuspocus was rejected as the relay because its normal persistence stores server-side Yjs documents, which breaks the encrypted record boundary.

The `ganbaru-sync-contracts`, `ganbaru-sync`, and `ganbaru-sync-replica` crates exist; the optional `ganbaru-sync-relay` binary is planned. Domain adapters keep composite value codecs, intrinsic validation, and projection ownership; the sync engine owns delivery, causality, and merging. Durable replication, live presence, and executable commands are distinct protocols.

### Topology and authority

Content merges without a leader and any path may deliver signed operations; only authority is ordered, once per space. The reasons are in [Sync topology and authority](../architecture/decisions/sync-topology-and-authority.md).

| Role | Meaning |
| --- | --- |
| Coordinator | The desktop that pairs devices and hosts the LAN listener. It is the personal space's first anchor. |
| Hub | Any member installation that stores and serves operations. Every reachable member serves its LAN peers, authenticated by mutual TLS with the certificates its enrollment names. |
| Anchor | The one member installation per space that sequences the hash-chained authorization log of enrollments, removals, roles, admin keys, and handovers. It holds the space keys and never gates content. |
| Relay | A blind, self-hosted store-and-forward service for encrypted records. It is never an anchor. |
| Always-on anchor | A self-hosted server running as an enrolled member that holds the keys, so it is also a hub reachable from every network and a backup. |

- Editing and delivery continue while the anchor is unreachable; only authorization proposals wait.
- Proposals are signed with an admin key separate from the person key, held by the anchor and chosen admin devices.
- A removal's cutoff is the anchor's stored vector. Operations the anchor stored are final. Operations only peers had seen are provisional, and those beyond the cutoff are excluded on every replica and kept as late changes for an admin to restore or discard.
- Any device can quarantine another locally while the anchor is unreachable. Quarantine never excludes operations by itself.
- Anchor handover is signed with the admin key and starts a new anchor epoch. A restored or cloned anchor is detected the same way a restored or cloned writer is.

### Storage ownership

Every persisted field needs an explicit replication classification before it can leave a device. Unknown fields fail closed.

| Domain | Portable data | Device-local data |
| --- | --- | --- |
| Notes, Projects, Calendar, Quick notes | Content, relationships, templates, archive, Trash, retained history, favorites | Recents, current selection, navigation, viewport layout |
| Preferences | Profile, themes, language, time format, rhythm defaults | Font scale, layout, shortcuts, notification delivery, explicit presentation overrides |
| Distraction blocker | Rule definitions and device-attributed history | Permissions, application bindings, enforcement state |
| Music | Library identities, playlists, assignments, portable preferences | Source bindings, media bytes, current playback, volume, routing |
| Chat | Organizational content, portable review history, origin-attributed drafts | Execution processes, credentials, provider homes, terminals, native trust, paths, caches |
| Managed assets | Immutable content and metadata | Transfer staging, local availability, caches |
| Focus | Committed history, explicit controller ownership history | Live presence, local activity sources, native alarms and effects |

Unsent Chat drafts keep their origin and can be explicitly continued on another device. Shared preferences move from `config.json` into SQLite so a preference change and its outbound record commit atomically. Remove `config.json` and the unused `.yjs` vault directory only after their consumers migrate.

### Transactional operation boundary

Every synchronizable mutation, including imports, restores, scheduled jobs, and native background writes, commits its canonical changes, required relational projections, and capture marks together. Sealing then turns captures into signed operations that carry stable identity, the installation writer, causal dependencies, the space, authorization revision, key epoch, and format and manifest versions. This is implemented for replicated tables; required history and asset references join the boundary as their domains convert.

UI invalidation and native effects follow commit. Incoming operations are validated intrinsically and applied in bounded transactions with capture suppressed; missing dependencies stay pending. A relay receipt means delivery to the relay, not that another device committed the change. The UI distinguishes saved on this device, received by relay, and confirmed on another device.

Authoritative connections already use WAL with `synchronous=FULL`, because WAL with `NORMAL` can lose committed transactions on power failure. See [SQLite synchronous](https://www.sqlite.org/pragma.html#pragma_synchronous).

### Conflict semantics

Valid concurrent operations converge regardless of delivery order; rejecting whichever arrives second is insufficient.

- Independent fields merge. Concurrent values for one scalar keep alternatives, a deterministic displayed value, and a visible resolution action.
- Calendar start, end, timezone, and recurrence form one coupled value. A conflict suspends automatic occurrence activation until resolved.
- Notes and project placement use stable identities and a cycle-safe [replicated tree move algorithm](https://martin.kleppmann.com/papers/move-op.pdf), with stable ordering identifiers instead of floating positions.
- Deletion creates tombstones. Concurrent edits stay recoverable in Trash or conflict recovery, and old operations never silently restore deleted content.
- Database property type changes keep incompatible values for resolution.
- History restore writes a safety version and new operations against current state; it never rewinds causal history.
- Cross-feature actions such as task scheduling form one operation group.
- Scheduled messages and other executable jobs have one execution device and stable execution receipts. Receiving their records never executes them.

### Notes editing

Whole-block replacement is not a collaborative text protocol, and current Notes writes still replace complete block payloads. The editor must submit incremental operations, preserve relative selections and comment anchors, and keep local undo separate from remote changes. TypeScript and Rust share UTF-16 positions. Incoming changes are prepared in isolated working state and committed atomically with their SQL projections before reaching the active cache. Writer IDs must be unique across installations, restored copies, and windows. See [Yrs](https://docs.rs/yrs/latest/yrs/).

### Keys and enrollment

Both enrolling devices show a verification code, and the existing device confirms before releasing vault keys. Direct connections use TLS 1.3 with pinned device identity, operations are signed, records and asset chunks use XChaCha20-Poly1305, and resource-key distribution uses HPKE with X25519 and HKDF-SHA256. Secrets live in native credential storage or Android Keystore wrapping. [HPKE](https://www.rfc-editor.org/rfc/rfc9180.html) alone does not provide authorization, replay protection, or downgrade protection, so the protocol composition needs review before transport is enabled.

Enrollment and revocation follow the anchor-sequenced authorization log in [Topology and authority](#topology-and-authority). A separate owner recovery identity and recovery kit receive resource-key envelopes. Revocation rotates affected keys and rejects new operations from the removed device; it cannot erase copies that device already holds.

### Contacts and contact requests

The vault's one person identity is an Ed25519 key pair, generated by the first device that can write the vault and has no coordinator pin. That device creates it on the first sync pass after linking or on the first Contacts action that needs it, whichever comes first; other devices receive the row with a vault refresh, so sealing on a replica waits until then. The private key lives in native credential storage on desktop and, as an interim measure until Android Keystore wrapping exists, in an app-private file on Android. A linked device that holds the identity row but not the key asks the coordinator for it over the authenticated mutual TLS link; every linked device therefore holds a copy, and unlinking a device does not erase the copy it already received. Person key rotation is not implemented.

Contact requests are the only messages the coordinator accepts without a client certificate. They ride the same listener and protocol as pairing: a request carries the signed requester card, the recipient's current card nonce, and a request signature, and a status poll carries a signed, time-bounded query so only the requester learns the outcome. The coordinator verifies signatures before writing anything, rejects a stale nonce as a revoked card, replaces an earlier pending request from the same key, caps pending received requests, and answers a blocked requester exactly as it answers an accepted write. Received requests persist only while the coordinator can write the vault. The bounds are recorded in [Network and privacy](security/network-and-privacy.md#current-network-surfaces); the user-facing behavior is owned by [Contacts and invitations](../features/collaboration/README.md).

### Bootstrap, assets, backup, and compaction

Bootstrap transfers a consistent typed snapshot and causal checkpoint, followed by incremental operations, staged and validated before atomic activation. A phone with a different vault keeps it as a recoverable local vault; combining vaults requires an explicit import preview.

Managed assets use immutable identities, encrypted manifests, authenticated hashes, bounded resumable chunks, and atomic publication. Filenames are metadata and never choose destination paths. Structured data syncs automatically; attachments default to unmetered transfer with explicit download and offline controls.

Backups capture a consistent database and pinned asset set with authenticated encryption. Restore defaults to an isolated recovery copy; rejoining requires membership reconciliation and a fresh writer generation, and never restores credentials, rewinds acknowledgements, or resurrects tombstones. Compaction requires acknowledged checkpoints, and asset collection respects live references, retained history, pending transfers, and backup pins.

Vault replacement must fence the generation across processes, not only within one process as the current guard does.

### Settings and Android delivery

Onboarding and Settings will show linked devices, connection method, last successful sync, pending changes, unavailable assets, conflicts, recovery status, focus controller, anchor, and relay configuration, with pause, retry, removal, and recovery export.

Android uses WorkManager for deferred sync and a visible, user-enabled connected-device service for live companion status. Permission denial, process death, reboot, network changes, and background restrictions must surface as degraded connectivity. Background service availability never establishes focus or idle activity.

## Delivery and acceptance

| Milestone | Status | Remaining work |
| --- | --- | --- |
| Focus correctness and contracts | Partial | Durable device controller ownership, remote commands, and live companion status |
| Durable mutation and storage boundaries | Partial | Scoped preferences and capture coverage for the remaining domains |
| Local whole-vault handoff | Partial | Physical multi-device acceptance |
| Concurrent replication of Quick notes | Implemented | Physical multi-device acceptance |
| Concurrent replication of other domains | Planned | Preferences, Contacts, Projects, distraction rules and usage counters, Calendar, Focus history, Music library, Chat organizational content, Notes text and tree merging, managed assets, and retiring whole-vault handoff |
| Concurrent secure local linking | Planned | Anchored authority with admin keys and late changes, LAN peer exchange, operation encryption, key lifecycle, typed bootstrap, and compaction |
| Delivery beyond the LAN and Android sync | Planned | Encrypted relay or always-on anchor, native background runtime, encrypted backup, measurements, and physical acceptance |

Required tests include reordered, duplicated, delayed, and interrupted delivery; text and tree convergence; deletion, undo, and history restore; crashes at persistence and acknowledgement boundaries; full disks, corrupt staging, and missing assets; invalid identity, signatures, invitation replay, revocation, key epochs, payload bounds, and protocol versions; controller handoff failure and expired commands; duplicate jobs; and long-offline replicas, compaction, restored backups, and cloned writers.

Queues and caches stay bounded. Pairing, key lifecycle, native bridges, and remote capabilities require a separate security review, and physical Android and desktop acceptance covers sleep, force-stop, reboot, permissions, manufacturer restrictions, clock changes, and disconnected use.
