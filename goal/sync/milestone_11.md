# Milestone 11: anchored authority and LAN peer exchange

**Status: Planned.** Part of the [concurrent sync goal](README.md). This plan replaces the hub-sealed revocation of milestone 1 with an anchor-sequenced authorization log, adds the admin key and recovery kit, removal cutoffs with late changes, quarantine, anchor handover, Android Keystore wrapping, and LAN peer exchange where every member serves. The owning contract stays [Device linking and synchronization](../../docs/data/sync.md), with [Sync topology and authority decisions](../../docs/architecture/decisions/sync-topology-and-authority.md), [Sync engine decisions](../../docs/architecture/decisions/sync-engine.md), [Sync schema](../../docs/data/schema/sync.md), and [Network and privacy](../../docs/data/security/network-and-privacy.md). The progress checklist is at the end.

## Outcome

- Each vault has one anchor installation that sequences a hash-chained authorization log: device enrollments, removals, admin keys, late change resolutions, and anchor handovers. Content never waits for the anchor; only authorization changes do.
- Enrollment and removal need an admin key, held only by admin desktops and by the recovery kit. A stolen phone cannot enroll, remove, or mint writers for other devices.
- A removal fixes a cutoff per writer at the anchor's stored vector. Operations the anchor stored are final and never excluded. Provisional operations past the cutoff are excluded on every replica and become late changes that an admin restores or discards; nothing is applied or dropped silently.
- Any device can quarantine another immediately and locally.
- The anchor survives restore, clone, and loss through an automatic self-handover, clone evidence, admin handover, and the recovery kit.
- Every member listens on the LAN and syncs with any enrolled peer over mutual TLS, so a switched-off coordinator no longer stops sync between other devices.
- Android wraps every private key (person, writer, device, pairing TLS) with an Android Keystore key.
- The peer exchange ships only after the simulation covers removals concurrent with edits and partitions, and after an external security review.

## Scope

In scope:

- Contracts: authorization entry kind `4` with an extensible entry kind, the space charter, writer certificate v2 signed by a per-installation device key, admin proposals, recovery seed derivation, and removal of the revoke kind `2`.
- Engine: enrollment-gated store, authorization apply and chain selection, removal exclusion with displaced-version retention, late changes, fork evidence from any peer, and readiness on the authorization revision.
- Replica and app: device and admin keys, the charter at first link, pairing as enrollment, the proposal outbox, the anchor sequencing service, the anchor high-water revision, self-handover after restore, admin handover with coordinator repinning, the recovery kit, quarantine, the anchor mark, and Reset sync state.
- Peer exchange: listeners on every member, admission from the log, a device-local peer book, address hints, bounded fan-out, member advertising, discovery key rotation on removal, and the optional Linux firewall grant for member desktops.
- Android Keystore wrapping: Kotlin code in `ganbaru-mobile-sync`, the registration export in the app crate headless entry.
- Settings UI for devices, roles, removal, quarantine, anchor status and handover, the recovery kit, and late changes, with `sync.ts` strings.
- Simulation of the anchor, peers, partitions, outages, removals, handovers, restore, clone, quarantine, and late changes.

Out of scope:

- Multi-person membership and roles beyond one person's devices (the [sharing goal](../sharing/README.md)). The entry format leaves room for them; no role other than `member` and `admin` exists.
- Android as admin or anchor, and Android listening in the background.
- Contact request receipt by any device: contact requests stay on the coordinator in this milestone (see [Contacts](#contacts-and-the-person-key)).
- The relay, always-on anchor, record encryption, and space key rotation ([milestone 12](milestone_12.md)). A shared-folder transport is dropped from the sync goal.
- Snapshot bootstrap itself, acknowledgements, and compaction ([milestone 10](milestone_10.md)). Milestone 10 step 5 serves bootstrap from the member listeners this milestone adds.
- Vouching for operations past a cutoff.

## Prerequisites

- [Milestone 2](milestone_2.md) is merged: the `Local` table class, the adapter registry, winner elision (tombstones copy winner values first), multi-domain simulation, and conflict and recovery presentation. Late changes reuse the recovery presentation and the winner-value copy.
- [Milestone 3](milestone_3.md) is merged: DNS-SD discovery with the member tag under a vault discovery key (delivered in `SyncState` over mutual TLS and kept in the interim `CoordinatorPin.discovery_key` field), address refresh through `SyncState`, private and overlay dial ranges, the Linux firewall scope shapes for TCP and UDP, `SyncHost` and `HeadlessHost`, and the Android WorkManager pass in `ganbaru-mobile-sync`.
- Milestone 1's physical acceptance does not block this milestone.
- [Milestone 10](milestone_10.md) steps 1 to 4 are merged: the vault lock, kind `3` acknowledgements with stability and dormancy over the pairing state's counted set, compaction of kinds `0` and `3`, and the asset sweep. Its steps 5 to 7 land after this milestone; see [Mixed mode before handoff retirement](#mixed-mode-before-handoff-retirement).

## Current state

Verified on 2026-10-23 at `feat/sync` 10671f43.

- **Versions.** `FORMAT_VERSION` is 2 (`crates/ganbaru-sync-contracts/src/op.rs:48`); kind `3` kept format 2, so step 1 takes the next free format number. Handoff `PROTOCOL_VERSION` is 9 (`crates/ganbaru-handoff/src/protocol.rs:18`). `VAULT_MANIFEST_VERSION` is 5 at this check (`crates/ganbaru-sync/src/manifest/vault.rs:14`) and moves with domain milestones. Certificate version is 1 (`certificate.rs:16`); pairing state schema is 2 (`pairing.rs:36`).
- **Kinds.** `OperationKind` is `Changes = 0`, `Genesis = 1`, `Revoke = 2`, `Acknowledgement = 3` (`op.rs:63-99`), with a comment reserving `4` for authorization entries. The revoke payload is target (16) | cutoff (8) | reason (1) (op.rs:17). `RevokeReason` is `Revoked` or `Forked` (op.rs:358-380); no production code seals `Forked`. The header carries `authorization_revision` (op.rs:401), which nothing interprets yet.
- **Bounds.** `MAX_OPERATION_BYTES` is 512 KiB and `MAX_HEADER_BYTES` is 64 KiB (`crates/ganbaru-sync-contracts/src/bounds.rs:8-12`).
- **Certificates.** Certificate v1 is version | writer key | person key | device id | created_at | predecessor | person signature (`certificate.rs`). No v2 or charter exists.
- **Space context.** `SpaceContext { space, anchor: PersonPublicKey, coordinator_device_id }` (`crates/ganbaru-sync/src/lib.rs:84-95`); `space_context` at `local.rs:63-73`. Carry-forward takes `coordinator_device_id` as a parameter (`crates/ganbaru-sync-replica/src/carry_forward.rs:34-141`).
- **Store.** `store_genesis` (`store.rs:258`) refuses `Untrusted` when the person key differs from `ctx.anchor` (:278-280) and applies the writer limit (:286). `may_issue` (:326-328) is called from `store_next` (:371, `UnauthorizedRevoke`) and from `apply_one` (`apply.rs:530`). `StoreRefusal` (:66-95) and `HoldReason` (:21-48) are unchanged.
- **Revocation.** `apply_revoke` (`apply.rs:425-470`, sole caller :536) returns `Blocked` when the target applied past the cutoff: a local target sets `reseal_needed`, a remote one goes to `blocked_revocations`. No rollback of applied remote operations exists. `merge::revoke_effects` (`merge.rs:623-660`) lowers `cutoff_seq`, sets `revoked` or `forked`, and holds waiting operations past the cutoff as `invalid`. `Engine::seal_revoke` (`seal.rs:1879`, coordinator check :1892) and `Sealer::seal_revoke` (:590) seal kind `2`; the hub calls it from `revoke_unlinked` (`apps/client/src-tauri/app/src/sync/service.rs:1558-1600`, called at :759).
- **Reseal.** `reseal.rs` has `ensure_unseen` (:82), `remove_effects` (:135), `truncate_chain` (:188), `Engine::reseal` (:247), `reseal_fork` (:272), and `reseal_after` (:294); replay re-seals kind `2` through `seal_revoke` (:404).
- **Coordinator.** `coordinator_device_id` (`service.rs:1631-1641`) is the pinned coordinator's id, else the host's own; it is re-exported at `sync.rs:33` and used by `service.rs:248,1191`, `vault/backup.rs:764`, and `sync/recovery.rs:113`. Person key release is `release_key_for_coordinator` (`contacts/identity.rs:348`, desktop) with the client side in `try_release` (:393). The handoff protocol's unrelated `coordinator_device_id` fields stay.
- **Keys.** Desktop writer keys use the keyring service `com.ganbaru.sync` (`crates/ganbaru-sync-replica/src/writer.rs:56`, `sync/host.rs:194`); Android uses `FileKeyStore` (writer.rs:92, `host.rs:199,355`). The pairing TLS key is `rcgen::KeyPair::generate()` (`pairing.rs:1088`). `CoordinatorPin` (:107) holds `discovery_key: Option<String>` (:125). `MAX_LINKED_PEERS` is 32 (:41).
- **Schema.** `20261008120000_sync_foundation.sql` keeps `sync_ops` STRICT with `hold_reason` checked against three values (:60) and `sync_writers.state` checked against `active`, `retired`, `revoked`, `forked` (:37). Later migrations added `sync_version_refs`, `sync_seeded_tables`, `sync_capture.held_until_ms`, elided versions, tombstone cause, `sync_writers.compacted_seq`, and the device-local `sync_stable`; the latest sync migration is `20261023030543_sync_compaction.sql`.
- **Compaction.** `compactable()` (`compact.rs:292`) covers kinds `0` and `3`; genesis, revoke, reserved kinds, and chain heads keep their bodies.
- **Simulation.** `crates/ganbaru-sync/src/tests/sim.rs` (1785 lines) runs `seeded_runs_converge_after_partitions_crashes_and_revocations` against a star in four shards, using `seal_revoke` (:1452).
- **Not present.** No charter, device keys, certificate v2, authorization log, proposal outbox, recovery kit, fork evidence, quarantine, or peer book.
- **Docs.** `docs/data/sync.md` covers writers and keys (:96), revocation by the hub (:100), kind `3` (:104), the hub and protocol 9 (:108), compaction (:125), exchange cadence (:128), the target design (:150), topology and authority (:162-178), and keys and enrollment (:221). The topology record is `docs/architecture/decisions/sync-topology-and-authority.md`.

## Design

### Terms

The anchor is the installation that sequences the authorization log. In this milestone it is always the coordinator: anchor and coordinator are one role, and handing over the anchor moves pairing and handoff coordination with it. The person key stays the identity in Contacts and signs the space charter; it no longer certifies writers. The field `SpaceContext.anchor` is renamed `person_key`, and "anchor" means only the sequencing installation from here on.

### Space charter

The charter is the authorization root: version | space id | anchor device id | anchor device public key | anchor TLS fingerprint | anchor admin public key | recovery admin public key | initial discovery key (the vault's current milestone 3 key) | created_at | person signature (domain `ganbaru-sync/charter/v1`). It is created on the coordinator when a vault first links (the same moment milestone 1 seals pending captures), travels in the pairing response, and is stored in a new `sync_spaces.charter` column. It is revision 0 of the log and implicitly enrolls the first anchor as an admin desktop with the anchor admin key, so the anchor signs its own proposals from the first entry without holding the recovery seed. Step 2 adds the anchor admin key field, which step 1 left out. Receivers verify it against the personal space identity in `contacts_local_identity`.

Vaults that already hold sync state from earlier builds have no charter. They need **Reset sync state** on the coordinator: it clears the engine tables, including `sync_seeded_tables`, and creates a charter, so the next vault open republishes every current row through milestone 2's `seed_captures`; every other device unlinks and pairs again. There is no upgrade path from v1 certificates or kind `2`, per the no-external-users rule.

### Device keys and writer certificate v2

Each installation creates an Ed25519 device key per vault at pairing. It is sent in the pairing request and recorded in the enrollment entry. Writer certificate v2 is version | writer key | device id | device public key | created_at | predecessor | device signature (domain `ganbaru-sync/writer-certificate/v2`). Step 1 adds `MAX_CERTIFICATE_BYTES`, its largest encoding (with a predecessor), to `bounds.rs`. A device mints successor writers (restore, clone, lost key) with its own device key and never waits for the anchor. A phone holding the person key can no longer certify writers for other devices, and sealing no longer depends on the person key being released.

`store_genesis` admits a genesis when the certificate verifies with the named device key and that device is enrolled and neither removed nor quarantined in the receiver's selected log. Because the certificate is self-verifying from the log, any peer can forward a genesis.

### Authorization log

Entries are kind `4` (`Authorization`) operations sealed in the anchor installation's ordinary writer chain. Storage, gap and fork detection, push and pull pages, carry-forward, and snapshots carry them with no new store. The body holds an inner header, epoch | revision | previous entry hash | anchor device id, then the entry. The inner chain links entries across the anchor's successor writers and across epochs. An entry is accepted only from a writer whose device is the anchor of that entry's epoch.

Every entry carries an extensible entry kind. A receiver stores and forwards an entry of a kind it does not know like any other operation and keeps it in the inner chain, and the entry contributes nothing to derived authorization state until the receiver understands its kind; an upgraded receiver derives it from the stored log. [Milestone 12](milestone_12.md) adds the `DeviceKey`, `KeyEpoch`, `RecoveryKey`, and `ServerConfig` kinds this way, with no format change. This milestone defines these kinds:

| Kind | Content | Signed proposal |
| --- | --- | --- |
| `enroll` | Device id, device public key, TLS fingerprint, label, device kind, role (`member` or `admin`), admin public key when the role is `admin` | Admin |
| `remove` | Device id, a cutoff for every writer of that device the anchor stored (its `stored_seq`), a fresh discovery key | Admin |
| `set_role` | Device id, new role, admin public key, or a replacement recovery admin public key | Admin |
| `resolve_late` | Removal revision, then per row: restore or discard | Admin |
| `handover` | New anchor device id, epoch + 1, continues-from revision and hash, the old anchor's stored vector when known | Admin, sealed by the new anchor as the first entry of its epoch |

Rules:

- Writers of a removed device that the removal does not list have cutoff 0. Writers certified by a removed device key after the removal are refused at store.
- The admin set is the charter's recovery key plus every admin key enrolled and not removed. The recovery key can be replaced, never removed, so the set is never empty. Only desktops can be admins; milestone 12 extends this to the always-on server device kind. The anchor must be an admin.
- The anchor validates each proposal against the log as it stands, including the author's admin key at that revision, so two admins removing each other resolve to the first one sequenced.
- Proposals are idempotent by hash; replaying a sequenced proposal returns its revision.
- Content operations already carry `authorization_revision`; an operation is ready only when the receiver's selected revision is at least that value. A kind `4` operation that creates revision R carries R minus 1 in that header field, so it never waits on itself. Store gating does not depend on it.
- A `remove` or a demoting `set_role` that names the current anchor is rejected; a handover comes first.

### Chain selection

A receiver can see more than one chain only after a handover race, an anchor restore, or an anchor clone. The selected chain is the one whose head has the highest epoch; between two `handover` entries with the same epoch, the one with the lower entry hash wins. An epoch is valid only through a `handover` proposal signed by an admin key valid at its continues-from revision. Entries of an older epoch past the handover's continues-from revision are superseded.

Authorization state, exclusions, and late changes are derived from the selected chain. When selection changes, the engine recomputes them in the same apply transaction: operations excluded only by a superseded removal return to waiting and apply through normal readiness, and that removal's late changes are deleted. Superseded proposals stay in their author's outbox and are proposed again to the new anchor.

Readiness on `authorization_revision` R holds when the selected revision is at least R or when the receiver stores entries reaching R in any chain it holds, selected or superseded. It guarantees only that the receiver has stored what the author saw, not that its selected state covers it; this keeps operations stamped on a later-superseded chain live, and exclusion after apply must equal never applying. A device that reselection leaves unenrolled keeps its applied operations (only a selected removal excludes); its new operations wait until an enrollment is selected again.

### Proposal outbox

Each admin installation keeps proposals in a device-local outbox (`app_config_dir/sync/<vault id>/outbox.json`, atomic writes) until they appear in the selected chain or the anchor rejects them with a final reason. The UI lists pending and rejected proposals. The anchor's own proposals use the same path, so an anchor restored from backup still proposes them again.

### Removal, finality, and exclusion

- **Cutoff.** A removal's cutoff vector is the anchor's `stored_seq` per writer when it sequences the removal. An operation the anchor stored is final and no later removal by that anchor excludes it.
- **Anchor mark.** Members keep, per writer, the highest anchor-confirmed sequence in a new `Local` engine table `sync_anchor_marks`, taken from the anchor's stored vector in direct exchanges and from the anchor's milestone 10 kind `3` acknowledgements, which peers relay. Status shows saved on this device, received by a peer, and confirmed by the anchor from it.
- **Displaced versions.** When a member applies an operation past its anchor mark that supersedes another writer's version, the superseded version moves to `sync_displaced_versions` (with winner values copied first, as tombstones do) instead of being deleted. Rows are dropped when the anchor mark passes the displacing operation. The anchor itself never writes rows. This keeps exclusion exact without scanning operation bodies, and stays correct after milestone 10 compacts bodies.
- **Exclusion.** Applying a `remove` marks every operation of the affected writers past its cutoff with `excluded_revision` and holds waiting ones as `invalid`. For applied ones, a generalization of reseal's `remove_effects` to a remote writer removes their versions and tombstones, restores displaced versions, and marks rows dirty. It replaces `Outcome::Blocked` and `blocked_revocations` (`apply.rs:346-348`). Operations past the cutoff are still stored, up to 1,024 per writer, with `excluded_revision` set, and forwarded, so late changes converge on every replica. The cutoff refusal at `store.rs:358` applies only beyond that bound and keeps the `Revoked` refusal.
- **Honest dependents.** Operations from other writers that depended on excluded ones stay valid.
- **Late changes.** Applying a removal derives `sync_late_changes` rows deterministically from the excluded bodies: the latest excluded value per field group, excluded tombstones, and rows created by excluded operations. A row created by an excluded operation stays hidden even when honest writers edited it, and its late change includes those edits, because bringing such a row back means accepting the removed device's creation, which is an admin decision. **Restore** seals the values (or the tombstone) as an ordinary write of the admin device, then proposes `resolve_late`; **discard** proposes only `resolve_late`. Every member drops the row when `resolve_late` applies. Late changes reuse milestone 2's recovery presentation.
- **No vouching.** Cutoffs are fixed. An admin restoring late changes covers the honest case, and accepting evidence from peers would let a removed device influence outcomes through peers that have not yet seen the removal.
- **Long outages.** Provisional retention has no hard cap. Status shows the count of unconfirmed operations, the displaced-version count, and how long the anchor has been unreachable, and suggests a handover after 30 days.
- **Removed device.** Peers that applied the removal refuse it at TLS and every replica refuses its new writers. It can still exchange with a member that has not yet applied the removal; it then receives its own removal, including the new discovery key. This window is accepted: identity stays the TLS fingerprint, so the only consequence is a presence leak until that member catches up. A device that receives its removal, or whose every known peer refuses the handshake, stops syncing and shows that it was probably removed. Its local vault is never deleted. Copies it already holds cannot be erased, and the UI says so. Content it never sent to anyone is lost with it, like any offline-only data.

### Fork evidence

Fork evidence is two signed envelopes with the same writer and sequence and different hashes. It is self-verifying and travels between peers: `SyncState` lists forked writers with the fork sequence, and a `SyncForkEvidence` request returns the pair, stored in `sync_fork_evidence`. Every receiver excludes both branches past the common prefix without late changes. Each clone installation re-seals its own branch under a successor writer through `reseal_fork`, generalized to start from received evidence. A clone destroyed before it re-seals loses its branch; this is accepted. A fork of the anchor's writer chain is clone evidence for the anchor: that epoch freezes, entries past the fork are superseded, and only an admin handover continues the log.

### Quarantine

Quarantine is device-local in `app_config_dir/sync/<vault id>/quarantine.json`. The device refuses the quarantined device at TLS, never dials it, and refuses to store operations of writers certified by its device key with a new `StoreRefusal::Quarantined`, so a forwarding peer skips that writer and keeps the rest of its page. Honest operations that depend on refused ones wait causally. On an admin device quarantine also places a removal proposal in the outbox. Quarantine never excludes anything; lifting it lets the operations arrive again. Any device, including a phone, can quarantine.

### Admin keys and the recovery kit

- Admin keys are Ed25519 per installation, created when a desktop is enrolled or promoted as admin, stored in the desktop keyring under `com.ganbaru.sync`, and never copied between devices. The coordinator is an admin by the charter.
- The recovery kit is a 32-byte random seed. HKDF-SHA256 with info `ganbaru-sync/recovery-admin/v1` followed by a zero byte derives the Ed25519 recovery admin key through `ring`. Milestone 12 derives its owner recovery identity from the same seed with its own info string. Charter creation uses the recovery public keys already stored on the vault when milestone 12 created a kit before the vault first linked, and generates a kit otherwise. No new dependency is added; the code format is local.
- The kit is shown as a plain text file to save and a grouped Crockford base32 code with a checksum. The seed is kept in the keyring until the user confirms saving it, then deleted, so no device stores it afterwards. Settings can generate a replacement kit, which proposes `set_role` with the new recovery public key.
- Using the kit on a desktop derives the key in memory, signs a `handover` to that desktop (and an `enroll` or `set_role` for its admin key), and drops it.

### Anchor lifecycle

- **Default.** The coordinator is the anchor from the charter.
- **Pairing.** Pairing happens only on the current anchor. Approving a pairing request commits the pairing state and proposes `enroll` from the anchor's admin key. Every sync pass, the anchor reconciles its pairing peers with enrolled devices and proposes the missing enrollment or removal, which covers a crash between the two commits. Unlinking a device proposes `remove`; `revoke_unlinked` is deleted.
- **Planned handover.** An admin chooses a desktop that is an admin and reachable. The old anchor pushes its stored operations to the new anchor, then the new anchor seals the `handover` with the old anchor's stored vector and refuses to sequence removals until its own stored vector reaches it. Members update `CoordinatorPin` from the handover: device id and fingerprint from the enrollment, endpoint from the peer book, generation incremented. The new anchor rebuilds its pairing peer list from the log and, on becoming anchor, uses its own stored vector as its anchor mark, so it drops its `sync_displaced_versions` rows at once. The old anchor pulls the handover from the new anchor and keeps serving sync requests as a hub until every enrolled device's stored vector in its exchanges covers the handover, or 30 days pass, because before peer exchange members dial only their pinned coordinator.
- **Restore.** The anchor's writer record gains the last sequenced revision and entry hash, reserved before each entry is sealed. When the database is behind the record, or milestone 1's high-water check rotates the anchor's writer, the anchor stops sequencing and pulls the log from members. If its head is still behind after one exchange round with every reachable member, it signs a self-handover to epoch + 1 from its current head, so entries that only a now unreachable member saw lose deterministically. Their proposals return from outboxes.
- **Clone.** Covered by fork evidence: the epoch freezes until an admin handover.
- **Loss.** Another admin desktop signs a handover to itself, or the recovery kit does on any desktop. A forced handover has no old anchor vector, so operations only the lost anchor stored can become provisional again and, if a removal excludes them, late changes. This is accepted. A forced handover can reach members only over peer exchange, so the UI offers it, and recovery kit use, only once step 9 has merged; before that, losing the coordinator uses the handoff recovery action as today.
- **Outage.** Content keeps flowing between peers, proposals wait in outboxes, and quarantine works locally.

### Mixed mode before handoff retirement

Until milestone 10 step 6 retires handoff, unconverted domains stay single-writer, snapshots still come from the vault owner, and the owner can differ from the anchor.

- Snapshots carry the authorization entries (they are operations) and the new engine tables; `sync_anchor_marks` follows the `Local` rule and is restored from the receiver's rollback copy.
- When the anchor receives a refresh, carry-forward already exports its own writer's unsent operations, including entries, so the refreshed anchor's head matches its record and no restore is detected. A test covers it.
- After step 9, losing the coordinator has a forced handover path, so converted domains keep syncing without unlinking. The handoff recovery action is still needed when the lost device was also the vault owner; it must keep the log and enrollments.
- Interaction with milestone 10: step 6 switches milestone 10's counted set from pairing state to the devices enrolled and not removed in the selected log; the anchor counts like any member and is never dormant. Stable implies final because the anchor's acknowledgement is part of stability, so compaction never drops a body a removal could still exclude, and `sync_displaced_versions` rows are already gone once their displacer is final. Compaction covers only kinds `0` and `3`, so kind `4` bodies, which reselection and new members replay, are never compacted; a test pins it.

### Contacts and the person key

Contact requests stay received by the coordinator only. Receipt by any device moves to the sharing goal, because it needs per-person request routing that does not exist for one person's devices. The person key keeps its release path from the coordinator for Contacts; sync no longer needs it on phones.

### LAN peer exchange

- **Listeners.** Desktops listen whenever sync runs. Android listens only while the app is in the foreground; WorkManager passes stay client-only. A listener on a non-anchor serves only sync requests (and, from milestone 10 step 5, bootstrap to enrolled devices), never pairing, other handoff, or contact requests.
- **Admission.** Mutual TLS accepts exactly the fingerprints of devices enrolled, not removed, and not quarantined in the local selected log. The anchor additionally accepts the existing pairing flow.
- **Peer book.** A device-local file `app_config_dir/sync/<vault id>/peers.json` is rebuilt from the log and address hints. `SyncState` carries up to 32 member endpoints with device id and observation time, replacing milestone 3's coordinator-only endpoint list.
- **Fan-out.** A desktop exchanges with at most 3 peers at once and a phone with 2. The anchor is preferred when reachable, then LAN-discovered peers, then known endpoints. One long poll runs at a time, to the anchor when reachable.
- **Order.** A client pulls the anchor writer chain first and applies new entries before other pages. When pushing a genesis, it pushes the anchor chain through the enrolling revision first. A peer that still answers `UnknownWriter` is retried in the next round.
- **Discovery.** Every member advertises `_ganbaru._tcp` with milestone 3's member tag. The current discovery key is the one in the latest selected removal entry, else the charter's. Members advertise only under the current key and browse with the current and previous key for 24 hours, so members that missed the removal are still found and the removed device sees no advertisement it can recognize. The discovery key moves into the log, and milestone 3's interim `CoordinatorPin.discovery_key` field is removed.
- **Firewall.** Linux member desktops get an optional "Allow peers to connect" action that requests milestone 3's TCP and UDP scopes. This reverses milestone 3's rule that client desktops request no firewall rule. Windows keeps the per-program prompt.

### Protocol

Each step that changes the wire (steps 4, 6, and 9) takes the next protocol version when it lands, per the [wire versions rule](README.md#parallel-work-reservations). Step 1 changes the operation and certificate format (kind `4`, writer certificate v2), so it takes the next `FORMAT_VERSION`; step 2 removes kind `2` and certificate v1 together with their callers. Added: auth head (epoch, revision, entry hash) in `SyncHello` and `SyncState`; member endpoints and forked writers in `SyncState`; `SyncPropose` (anchor only) answering with a revision or a rejection reason; `SyncForkEvidence`; the device public key in the pairing request; the charter in the pairing response. Removed: everything that names kind `2`, and the `CoordinatorPin.discovery_key` field.

### Android Keystore

`ganbaru-mobile-sync` gains an AES-256-GCM key in `AndroidKeyStore` (alias per vault, `setUserAuthenticationRequired(false)` because the WorkManager pass unwraps headlessly) that wraps the person, writer, device, and pairing TLS private keys. Android holds no admin key in this milestone, so admin keys need no Android wrapping; a later Android admin role reuses the same wrapping. Files in app-private storage hold only ciphertext. The Kotlin Keystore code lives in `ganbaru-mobile-sync`. Rust reaches it through a `JavaVM` and a `GlobalRef` captured with safe `jni` 0.21 calls in a registration native method exported from the app crate headless entry (`app/src/sync/headless.rs`); the exported symbol's `export_name` attribute is the only unsafe surface and joins milestone 3's `runPass` inventory entry for that file. A plaintext key file found at startup is deleted and the device unlinks, which the Reset sync state relink already requires. StrongBox is used when available, never required.

### Schema

One engine migration (engine track, `sync_*` only):

- `sync_spaces.charter BLOB`.
- `sync_ops.excluded_revision INTEGER` (nullable, added with `ALTER TABLE`, so the STRICT table and its CHECKs need no rebuild).
- `sync_auth_entries`: one row per kind `4` operation with epoch, revision, previous hash, entry hash, entry kind, and `selected`.
- `sync_devices`: current state per enrolled device (device key, fingerprint, label, kind, role, admin key, enrolled and removed revisions), derived from the selected chain.
- `sync_displaced_versions`, `sync_late_changes` (resolved revision nullable), `sync_fork_evidence`.
- `sync_anchor_marks` in the `Local` class.

`sync_writers.state` keeps the persisted value `revoked` for writers of removed devices. Kind `2` disappears from the codec; the number stays unassigned.

## Testing

- **Contracts.** Golden vectors for the charter, certificate v2, every entry kind, proposals, and the recovery derivation; mutation tests that flip each signed byte; kit code checksum and grouping round trips.
- **Engine units.** Store gating (unenrolled, removed, quarantined, wrong device key, forwarded genesis); an unknown entry kind stored, forwarded, chained, and ignored by derived state until understood; chain selection (handover race, equal-epoch tie, restore self-handover, clone freeze); reselection reverting exclusions; exclusion with displaced restore across every merge kind and domain converted so far; late change derivation for values, tombstones, and created rows edited by honest writers; `resolve_late` idempotence; two admins removing each other; fork evidence from a non-hub peer.
- **Simulation.** `sim.rs` gains a peer topology: an anchor, two admin desktops, and phones exchanging with random peers under partitions, crashes, anchor outages, removals concurrent with edits, mutual removals, handover races, anchor restore and clone, quarantine, and late change resolution. Every run converges to identical rows, conflicts, recovery entries, late changes, and authorization state. This lands before peer exchange, with the existing seed counts.
- **Replica and app.** Pairing as enrollment and reconciliation after a crash between commits; outbox replay; restore detection; refreshed anchor with carry-forward; handover repinning; quarantine refusal at TLS and store; admission refusal of removed and quarantined fingerprints; Reset sync state.
- **Android.** Instrumented test that wraps and unwraps with the Keystore key from a WorkManager worker.
- **Frontend.** Focused Vitest for the sync API validators, the authorization store, and the late change presentation.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement). Steps 1 to 4 edit shared engine files and run in order; step 6 can start after step 2 but lands after step 3; step 7 after step 6; step 9 needs steps 5, 6, and 8 and the external review.

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-auth-contracts` | Kind `4` entries with the extensible entry kind, charter, certificate v2, proposals, recovery derivation and kit code, next `FORMAT_VERSION`, golden vectors. Purely additive: kind `2`, `RevokeReason`, and certificate v1 stay until step 2 so every crate keeps compiling. | `cargo test -p ganbaru-sync-contracts --lib -j 4 -- --test-threads=4` |
| 2 | `feat/sync-auth-log` | Migration, enrollment-gated store, authorization apply, chain selection, readiness on revision, `person_key` rename, removal of kind `2`, certificate v1, `may_issue`, `coordinator_device_id`, `seal_revoke`, revoke apply, and `revoke_unlinked`. Minimal service wiring keeps pairing working: a device key at pairing, the charter at first link, and the anchor sealing `enroll` at approval and `remove` at unlink directly; step 6 replaces the direct sealing with the outbox, `SyncPropose`, and reconciliation. Until step 3 a `remove` sets the writer state and refuses later operations but does not exclude applied ones. | `cargo test -p ganbaru-sync --lib -j 4 auth -- --test-threads=4`, `cargo test -p ganbaru-db --lib -j 4 sync -- --test-threads=4` |
| 3 | `feat/sync-removal-cutoffs` | Exclusion, displaced versions, anchor marks, late changes and resolution, reselection recompute. | `cargo test -p ganbaru-sync --lib -j 4 removal -- --test-threads=4` |
| 4 | `feat/sync-fork-evidence` | Evidence table and exchange types, fork exclusion on any receiver, generalized `reseal_fork`, anchor clone freeze, next protocol version. | `cargo test -p ganbaru-sync --lib -j 4 fork -- --test-threads=4` |
| 5 | `test/sync-peer-simulation` | Peer topology simulation with every authority scenario. | `cargo test -p ganbaru-sync --lib -j 4 sim -- --test-threads=4` |
| 6 | `feat/sync-anchor-service` | Device and admin keys, charter at first link, Reset sync state, pairing as enrollment and reconciliation, milestone 10 counted set from the log, outbox, `SyncPropose`, sequencing, revision high-water, self-handover, handover and repin, recovery kit, quarantine, next protocol version. | `cargo test -p ganbaru-sync-replica --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-handoff --lib -j 4 pairing -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4` |
| 7 | `feat/sync-android-keystore` | Keystore wrapping in `ganbaru-mobile-sync`, registration export in `app/src/sync/headless.rs`, plaintext key removal, unsafe inventory update. | `cargo check -p ganbaru-mobile-sync -j 4`, `cargo check -p ganbaru-tauri-app --lib -j 4` for the Android target, Android instrumented test |
| 8 | `feat/sync-authority-ui` | Devices with roles, remove, quarantine, admin designation, anchor status and handover, recovery kit create and use, outbox, late changes, confirmation status, `sync.ts` in English and Spanish. | `pnpm --dir apps/client exec vitest run src/lib/api/sync.test.ts --maxWorkers=1`, `pnpm --dir apps/client run check` |
| 9 | `feat/sync-peer-exchange` | Listeners on every member, admission from the log, peer book, endpoints in `SyncState`, fan-out and ordering, member advertising, discovery key rotation and removal of the interim `CoordinatorPin.discovery_key` field, Linux firewall action, next protocol version. Merged only after the external review. | `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-handoff --lib -j 4 discovery -- --test-threads=4` |
| 10 | `docs/sync-milestone-11` | Final docs, automated acceptance mapping, goal README update. | `pnpm -w run validate` |
| 11 | None | Physical acceptance. | Manual |

The external security review covers steps 1 to 4, 6, 7, and 9 as one package: the formats, the store gate, exclusion, the anchor service, Keystore wrapping, and the listener surface. Findings are fixed before step 9 merges and summarized in `docs/data/security/network-and-privacy.md`.

## Documentation

- `docs/data/sync.md`: writers and certificates (device key, v2), exchange and protocol (:98, peers, ordering, propose, fork evidence), revocation (:92) replaced by removal, cutoffs, finality, exclusion, late changes, and quarantine; Android delivery (:116) with foreground listening; carry-forward (:118-127) noting authorization entries; topology (:148-164) from target to current; keys (:207-211) with device, admin, and recovery keys; the Android person key interim (:215) replaced by Keystore wrapping; the "Concurrent secure local linking" row (:244) and the security review line (:249) updated.
- `docs/architecture/decisions/sync-topology-and-authority.md`: status from Reference to Implemented for authority and LAN peers; record anchor equals coordinator, entries in the anchor writer chain, device-key certificates, fixed cutoffs, the late change rule for created rows, and the dropped shared-folder transport.
- `docs/architecture/decisions/sync-engine.md`: the star (:27) and its limits (:43-44).
- `docs/data/schema/sync.md`: new tables and columns, the `Local` anchor marks, kind `4` with its extensible entry kinds and the store and forward rule for unknown ones, certificate v2, and the removed kind `2`.
- `docs/algorithms/sync/README.md`: chain selection, exclusion with displaced versions, and late change derivation with worked examples.
- `docs/data/security/network-and-privacy.md`: every member listens, admission from the log, advertising by every member, discovery key rotation, and the review summary. It also records the firewall grant for member desktops.
- `docs/data/security/unsafe-rust.md`: milestone 3's app crate headless entry gains the Keystore registration export.
- `docs/platforms/android/permissions-and-security.md`: Keystore wrapping and foreground-only listening.
- `docs/platforms/desktop/README.md`: the member desktop firewall action.
- `docs/features/README.md:26`: sync status line.
- `AGENTS.md`: the Sync convention (anchored authority and LAN peer exchange move from planned to implemented), and the `ganbaru-sync-contracts`, `ganbaru-sync`, `ganbaru-sync-replica`, and `ganbaru-mobile-sync` descriptions.
- The README "Docs differ until" row for operation kinds names milestone 11: the schema and sync docs above record kind `4`, the extensible entry kind, and the unassigned kind `2`.

## Manual acceptance

Devices: desktop A (anchor and coordinator, Linux with a firewall), desktop B (admin, Windows or Linux), Android phone P, and a disk clone of A for the clone scenario. Release builds.

| Scenario | Expected |
| --- | --- |
| Reset sync state on A, pair B and P | Charter created, recovery kit shown once; B and P enrolled; B promoted to admin |
| A switched off, B and P edit on the same LAN | B and P sync directly; status shows unconfirmed changes |
| A back on | Changes become confirmed on B and P |
| P offline edits, B removes P while A is reachable, P comes back | P is refused; P's edits past the cutoff appear as late changes on A and B; restore one, discard one; both converge |
| P quarantined on B with A off | B refuses P immediately; the removal applies when A returns |
| A and B remove each other while partitioned | Only the first sequenced removal applies |
| Planned handover from A to B | B sequences; P repins without relinking; pairing works on B |
| A restored from an older backup | A pulls the log before sequencing; no removal is forgotten |
| Clone of A runs beside A | Members detect the fork and freeze the epoch; an admin handover on B continues |
| A wiped, B removed, recovery kit used on a fresh desktop | Handover to the fresh desktop; P keeps syncing |
| P key files inspected on a rooted test device | Ciphertext only |
| B on Linux allows peers | Firewall rule created and removed by the package removal hook |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Apply of a removal excluding 1,024 applied operations | 1 s desktop, 3 s Android |
| Apply throughput with displaced-version retention on a member | Within 10% of the milestone 2 budget test |
| Keystore unwrap of all keys per WorkManager pass | 50 ms |
| Idle listener memory on a member desktop | 2 MiB over the client alone |

## Risks

| Risk | Mitigation |
| --- | --- |
| Reselection after a handover race flips exclusions back and forth | Deterministic selection, recompute in one transaction, simulation coverage of races |
| Displaced versions grow on a member that never reaches the anchor | Status shows the count and suggests handover; milestone 10 acknowledgements relay the mark |
| Forced handover demotes final operations | Only on loss; demoted operations become late changes, never silent loss |
| A new listener on every member widens the attack surface | Admission from the log only, sync requests and bootstrap only, external review gate |
| Keystore key invalidated by an OS update or backup restore | Unwrap failure unlinks the device with a clear reason; relink restores sync |
| Users lose the recovery kit | Two admin desktops recommended in the UI; the kit can be regenerated from any admin desktop |

## Deferred

- Multi-person roles, membership, and role downgrade cutoffs (sharing goal; the downgrade revision becomes the cutoff for the removed capability).
- Contact request receipt by any device (sharing goal).
- Android admin and anchor roles, and background listening on Android.
- Relay reading authorization entries: the relay stays blind; removal beyond the LAN is enforced by space key rotation in milestone 12.
- Shared-folder transport: dropped.
- Vouching past a cutoff: rejected.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- **Earlier step:** That a pairing device has the person public key before it verifies the charter; if the pairing response does not carry it before the person key release, the pinned TLS channel to the anchor is the trust root for the charter and the person signature is checked once the key arrives.
- **Earlier step:** That an authorization entry with 32 enrolled devices fits the operation bounds; if a `remove` cutoff list exceeds the body bound, split it per writer batch rather than raising bounds.
- Settled from source: `store_many` stops the page only on `StoreRefusal::Fork` (`crates/ganbaru-sync/src/store.rs:465`) and records every other refusal and continues, so the quarantine refusal is a new non-fork variant and needs no loop change.
- **Earlier step:** That `remove_effects` is writer-agnostic apart from `ensure_unseen`; if it relies on the local writer elsewhere, extract the shared half into `merge.rs` instead of duplicating it.
- **Earlier step:** That `CoordinatorPin.generation` can be incremented by a handover without the handoff path treating it as a new pairing; if it cannot, add a handover generation field to the pin.
- **Earlier step:** That handoff recovery keeps the log and enrollments; if it clears engine tables, restrict it to ownership state.
- **Earlier step:** That carry-forward includes the anchor's unsent kind `4` operations after a refresh; if not, add them to the bundle rather than relaxing restore detection.
- Settled from source: `get_java_vm` and `new_global_ref` are safe functions in both `jni` versions in `Cargo.lock` (0.21.1 `wrapper/jnienv.rs:600,2746`, 0.22.4 `env.rs:1389,4433`), so this path adds no unsafe code; the direct dependency pins the version Tauri already links.
- **Earlier step:** Exclusion of 1,024 operations against the budget; if exceeded, batch effect removal per row rather than per operation.

## Progress

- [x] Step 1: authorization contracts.
  - Branch `feat/sync-auth-contracts` (worktree agent-a83b293ffaca1e61a). Commits: 914ae61c `feat(sync): add authorization contracts`, 7492392e merge of local feat/sync (Music review fixes, manifest v6); both signed.
  - Landed in ganbaru-sync-contracts: `keys` (DeviceKeyPair/DevicePublicKey, AdminKeyPair/AdminPublicKey, small-order refusal), `charter` (space charter signed by the person key under `ganbaru-sync/charter/v1`, decode and verify separate, charter hash seeds revision 1), certificate v2 under `ganbaru-sync/writer-certificate/v2` signed by the device key beside v1 with `MAX_CERTIFICATE_BYTES` = 315 pinned by a test, `authorization` (entries enroll, remove, set_role device or recovery, resolve_late, handover, Unknown opaque; admin-signed proposals hashed for idempotence; closed 13-variant ProposalRejection; AuthorizationRecord with anchor sequencing and entry-hash chain), `recovery` (HKDF-SHA256, empty salt, info label plus zero byte, generic `derive`, recovery admin key, Crockford base32 kit code with 3-byte checksum, tolerant parser).
  - FORMAT_VERSION 3; OperationKind::Authorization = 4 with minimum format 3 (older-format envelopes report UnknownKind). Header carries the entry hash and authorization_revision = R−1; body is the record, checked against the signed entry hash. `Content::Authorization` boxes the record (clippy large variant).
  - Kind 2, RevokeReason, Revocation, and cert v1 kept with rustdoc saying the authorization log replaces them. ganbaru-sync holds kind 4 as NewerFormat (store classify and apply) and refuses to reseal one.
  - Tests: golden vectors for charter, cert v2, proposal, every entry kind as a sealed op (anchor log plus handover by a second writer), recovery admin key and kit code; old vectors unchanged. Bit-flip and random mutation over every new artifact; kit code round trip, grouping, tolerant parsing, every single-character substitution and a swap fail the checksum; HKDF matches a manual HMAC computation; unknown entry round trip byte for byte; charter added to the prefix-free person domain test.
  - Check 1: both branches supported (charter decodes without verification, `verify` is separate).
  - Check 2 held: a removal names one device so it carries at most 64 cutoffs, fitting easily; a handover with a full 4096-writer vector and a 1024-row late resolution also fit one operation. No split needed.
  - Deviations: the record lives in the body with its entry hash in the signed header, because a handover vector (about 98 KiB) exceeds MAX_HEADER_BYTES. Anchor-only data (removal cutoffs, fresh discovery key, old anchor vector) is sequencing outside the admin signature, since the admin cannot know it. Admin-must-be-desktop is a rejection reason, not a codec rule.
  - Open questions for step 2: when the engine moves kind 4 from held to applied; late rows are identified by (table, row) and may need the writer or op id; proposals carry no nonce, so created_at alone distinguishes deliberate re-proposals; whether the handover's old-anchor vector should become mandatory when known.
  - Checks after the merge: contracts tests (103) pass, clippy clean, fmt clean, sync and replica all-targets check clean.
- [x] Step 2: authorization log in the engine.
  - Branch `feat/sync-auth-log` (worktree /home/victor/ganbaru/.claude/worktrees/agent-a137f93925178b28b), based on feat/sync 0c9ab833. feat/sync moved to 58d64d23 (M6 step 4, Projects manifest) before the final checks. It is merged in signed merge 02631d80; the only conflict was an import line in ganbaru-sync tests/conformance.rs. Migration order is fine: `20261023090624_sync_authorization_log` precedes `20261023094450_projects_replication`. All commits are signed.
  - Commits:
    - 58a25aab `feat(sync-contracts)`: the charter carries the anchor admin key after the TLS fingerprint, MAX_CHARTER_BYTES is added, wire kind 2 is unassigned, and Revocation and RevokeReason are removed. Certificate v1 is removed; v2 becomes the only WriterCertificate, keeping CERTIFICATE_DOMAIN `writer-certificate/v2`. Golden vectors were reblessed, revoke.hex was removed, and certificate-v2.hex was renamed certificate.hex.
    - 3c28336a `feat(sync)`: migration `20261023090624_sync_authorization_log` (sync_spaces.charter, sync_ops.authorization_revision and excluded_revision, sync_auth_entries, sync_devices, sync_displaced_versions, sync_late_changes, sync_fork_evidence, sync_anchor_marks; sync_writers rebuilt so the cutoff may be 0). It adds an enrollment-gated store, kind 4 apply and chain selection, readiness on authorization_revision, and anchor sealing of enroll and remove. Revocation and reseal were removed (reseal_fork stays), and the sim removes devices through the anchor.
    - 59a7c086 `test(sync)`: authorization log tests and the migration test.
    - f6256944 `feat(sync-replica)`: an `authority` module (AnchorKeys, ensure_anchor_keys, ensure_charter, enroll, remove) and writer certificates signed by the device key. SyncKeyStore replaces WriterKeyStore. Carry-forward and the hub no longer take a coordinator device id.
    - 2f17cb7a `feat(handoff)`: PROTOCOL_VERSION 10. ControlMessage::Enroll carries `sync_device_key`, and LinkedPeer requires it and validates it at admission.
    - 2fac0f96 `test(sync)`: formatting and the StoredDevice alias.
    - bb6c77fc `feat(sync)`, app wiring:
      - Desktop founds the charter on a linked pass when none exists.
      - The enrollment sealer runs a pass after enroll_peer and before the Enrolled reply, so the bundle holds the charter and the enroll entry.
      - Unlink triggers sync_now.
      - Each anchor pass reconciles linked peers with the log, replacing revoke_unlinked.
      - NotEnrolled stops sealing until the exchange brings the enrollment.
      - The device key is kept in the keyring service `com.ganbaru.sync` (FileKeyStore on Android).
    - aac73f85 `docs(sync)`: docs/data/sync.md and docs/data/schema/sync.md.
  - Checks, all on bb6c77fc plus docs and run under the lock:
    - cargo fmt --all --check: clean.
    - Lib tests: ganbaru-handoff 95, ganbaru-sync 223 (1 ignored), ganbaru-sync-replica 101, ganbaru-sync-contracts 59.
    - `sync` filter: ganbaru-db 7 passed, ganbaru-tauri-app 39 passed.
    - Workspace clippy --all-targets -D warnings: clean.
    - After merge 02631d80:
      - ganbaru-sync lib: 243 passed, 1 ignored.
      - ganbaru-sync-replica lib: 92 passed.
      - ganbaru-projects lib: 59 passed.
      - ganbaru-db `sync`: 7 passed.
      - ganbaru-tauri-app `sync`: 43 passed.
      - fmt --check and workspace clippy: clean.
  - Implementation checks:
    - Person key at pairing: the charter is not sent in Enrolled, because the coordinator cannot guarantee the person key at pairing time. A newcomer receives the charter only inside the vault bundle, and the sealer's pass founds the space before the bundle is cut.
    - remove_effects (reseal.rs) takes any writer id and a keep-through sequence. It does not depend on the writer being the local one, so step 3 exclusion can reuse it for a removed device's writer.
    - Carry-forward exports every stored operation the replacement lacks, regardless of kind, so kind 4 entries carry and reapply. carry_tests founds and enrolls before the handoff.
    - Handoff recovery: sync_auth_entries and sync_devices are engine tables, and the charter is a sync_spaces column, so they travel in the snapshot. Only sync_anchor_marks is in SYNC_LOCAL_TABLES.
  - Plan-silent decisions:
    - The anchor is enrolled at revision 0.
    - An unenrolled genesis is retried, not refused.
    - Kind 4 from a non-anchor writer is held.
    - Removed writers past the cutoff are filtered from stored and served operations.
    - Compaction lanes include the authority writer.
    - Unknown device kinds enroll as mobile, and new devices enroll as Member.
    - The founding discovery key is fresh and random, not advertised until step 9.
    - The person-key release request stays in create_successor.
    - The headless converge test (headless_tests.rs) enrolls the phone's real device key and adds a priming pass, because the first pass only pulls the enrollment. The `assert!(pass.more)` after pass 2 was removed.
    - Push refusal wire codes are unchanged: an unenrolled writer maps to `unknown_writer`.
    - An enrollment sealer failure is only logged; reconcile heals it on the next anchor pass.
  - Dev reset:
    - Old sync state without a charter or with v1 certificates gets ResetRequired.
    - Pairing state without `syncDeviceKey` fails to load, so every device must be relinked after the vault sync state is reset.
  - Open questions for step 3:
    - Stamps from a superseded epoch may wait forever.
    - The DFS rederive over the chain costs O(n²).
    - Retired writers versus removal cutoffs.
    - Writers of devices left unenrolled after chain reselection.
    - A removed device id that is linked again is skipped by reconcile; decide whether re-enrollment needs a new device id.
    - Should a failed enrollment seal block the Enrolled reply (step 6)?
  - Landing review: the headless converge test lost `assert!(pass.more)` because the priming pass now pulls the enrollment, so the second pass seals and pushes the acknowledgement itself; the acknowledgement count assertion still pins convergence. Not a weakening.
- [x] Step 3: removal cutoffs, exclusion, displaced versions, late changes.
  - Branch feat/sync-removal-cutoffs, merged with feat/sync at c197227b (signed merge commit).
  - Landed: migration 20261023120329_sync_removal_exclusion, the exclusion module (cutoff exclusion, up to 1,024 excluded operations stored per writer, undo of effects with displaced version restore, late changes resolved through `resolve late` entries), anchor marks, displaced version retention, the restore API, simulation removals concurrent with edits, entries of excluded authorization operations counted for chain selection, and linear chain selection.
  - Commits (merge 983840a3 of feat/sync at c197227b; the reselection revert test landed inside 67f1e2ec rather than its own commit): 9b4f2473 feat(sync): exclude operations past a removal cutoff and derive late changes; 58581a92 fix(sync): store unseen writers of removed devices excluded and restore late creates whole; 9795ad4e fix(sync): count entries of excluded operations in chain selection; 67f1e2ec perf(sync): walk the authorization chain in linear time and measure removal budgets (also adds the reselection revert test); 287ec183 docs(sync): describe removal cutoffs, exclusion, and late changes.
  - Checks: contracts 101, sync 257 plus 2 ignored budget tests, replica 69, db sync 8, tauri-app sync 44, workspace clippy, and fmt all pass.
  - Tests: two admins removing each other settle identically in either arrival order; reselecting a chain without the removal reverts its exclusions and equals direct delivery; excluding an operation admitted against a superseded entry equals never applying it; an operation sealed against a superseded entry still applies (liveness); a device left unenrolled by reselection keeps what applied while new operations wait.
  - Measurements (release, median of 5, this machine): removal excluding 1,024 applied operations 308 to 355 ms against the 1 s budget; apply of 256 superseding updates on a member 271 ms against 255 ms on the anchor (about 6.5 percent, within 10 percent); rederive of a 1,000-entry log 228 ms before linear selection, 143 ms after (remainder is decode and signature checks); apply of a 256-operation batch 191 ms on the branch against 209 ms on the base, so no regression, though both exceed the 50 ms budget; seal of 100 notes 74 ms on the branch and 57 to 69 ms on the base, both over 20 ms, preexisting on this machine.
  - Implementation checks: removal stayed under 1 s without batching effect removal per row; selection was made linear instead of only measured; retention stayed within 10 percent without changing the storage of displaced versions.
  - Plan-silent decisions: the local device is exempt from its own exclusion; readiness clamps to the stored revision and uses the reachable revision; a resolution column records late resolutions; anchor and local device columns are stored; marks come from the anchor's applied contexts plus marks recorded in direct exchange; sync rows are created on demand for late-created rows and orphan sync rows are dropped; the first decision on a late change stays; a restore writes only late groups with no newer version, and late creates are restored whole keeping newer honest values; unseen writers of removed devices are stored excluded with cutoff 0; a removed device presenting a different key is untrusted; entries of excluded authorization operations are recorded at apply start and in the settle loop, keeping store domain-free; the retention budget compares member against anchor; rederive has no asserted budget.
  - Open questions for step 4: an honest operation whose context covers an operation beyond the 1,024 bound waits indefinitely; authorization operations refused beyond the bound are never recorded, so chains could still diverge there; seal and apply budgets are already exceeded on this machine and need a decision on hardware baseline; the retention budget test passed with about 6.5 percent against a 10 percent margin on a machine with wide run-to-run variance, so it may be flaky elsewhere.
- [x] Step 4: fork evidence.
  - Landed (engine, replica, handoff, app, sim, budget, docs): evidence table settlement flag (migration 20261023162759_sync_fork_settlement.sql), `fork.rs` (verify, store, reconcile, exclude both branches, freeze), unified `Excluding { removal, fork }` with one readiness rule in `exclusion::ready` rustdoc, generalized `reseal_fork` from evidence or local detection, anchor clone freeze (`ProposalRejection::EpochFrozen`, wire 13), orphaned rows materialize absent, beyond-bound creations purged and honest changes to them skipped. Replica: hub `fork_evidence` responder and `forked` in `SyncState`, client `reported_own_fork`, `offer_own`, and `share_evidence`, pull ignores `revoked`. App: `reseal_own_forks` in `apply_ready`. Sim: cloned spare forks. Budget: fork exclusion of 1,024 applied operations.
  - Commits: 9a92bba5 `feat(sync): exclude forked writers from fork evidence`; 99ac9db3 `feat(sync): exchange fork evidence between members over protocol 11`; c81b288d `test(sync): fork cloned spares in the simulation and budget fork exclusion`; 5c5f2721 `docs(sync): describe fork evidence, clone reseal, and protocol 11`; 20fb374d signed merge of feat/sync 49bafc67; 7b6b6159 `style(sync): format the fork exclusion budget`; e8eb79cd `docs(testing): list the fork exclusion budget`.
  - Protocol version: 11 (`SyncState.forked` bounded by the vector bound, `SyncForkEvidence` and `SyncForkPair` carry at most 2 operations).
  - Implementation checks: All pass after the merge: contracts 101, sync 269 (2 ignored), replica 75, handoff 97, db sync 9, tauri-app sync 44, workspace clippy all-targets, fmt. Release budget: fork exclusion of 1,024 applied operations has a median of 410 ms against the 1 s budget, and the retention margin held this run (297 ms against 305 ms). The sim forks cloned spares in the star topology and keeps the 48 default seeds passing. kind 4 carriers are only the anchor device's writers, so the bound applies only to a removed or forked anchor; `drop_beyond_bound` deletes entries past cutoff plus 1,024 and `EpochFrozen` or `NotAnchor` refuse the rest. Clone successors share a device key and both are admitted; a second fork of a successor is tested. The plain apply budget test fails its absolute budgets on this machine both here and at baseline 66dffe31 with matching numbers (apply of 256 operations: 174 ms here, 184 ms at baseline; seal 70 ms against 74 ms), so the change shows no apply regression.
  - Plan-silent decisions: the local device never excludes its own forked writer and reseals through `own_forks`; the freeze revision is the lowest authorization revision of the pair; a row with no merge state left after exclusion materializes absent; an honest Write or Tombstone to an unknown row whose context reaches past a removal bound is skipped (the operation applies), and dropping beyond-bound operations purges the whole merge state of rows they created, honest versions included, so A, B, C and the honest writer end identical. The client treats a `revoked` refusal on pull as an operation the hub holds until its next apply; an installation learns its own fork from the forked writers in `SyncState` or from a one-operation offer, and the app reseals its own forks right after each apply. Re-offer after a reselection revert: when a newly selected chain no longer removes a device, its excluded operations are cleared of their exclusion and offered to apply again (waiting, or held with their reason); fork exclusion never reverts, since evidence is permanent. An anchor reseals domain operations only: authorization entries past the kept sequence are dropped, and the frozen epoch continues only through a handover.
  - Deviation: after the anchor confirms one branch, exclusion cannot restore the versions that branch displaced (the anchor retains none). The group keeps its domain value until a re-sealed copy writes it; a copy destroyed before it reseals leaves that divergence. A deterministic fix needs the anchor to publish the kept branch.
  - Open questions for steps 5 and 6: a device that forked and later becomes anchor is not frozen; the handover service must clear the freeze. The anchor reseal through `reseal_own_forks` in `apply_ready` under a frozen epoch is covered by engine tests but not end to end through the app service.
- [x] Step 5: peer topology simulation.
  - Landed: the anchor republishes the groups an excluded fork branch wrote (fc21b7d2). The named admin desktop seals a handover through `Engine::seal_handover` (5dd3e72f). New engine tables `sync_row_creations`, `sync_stale_rows`, and the device-local `sync_restated_groups` (90d76237). Engine fixes so fork exclusion, removals, handovers, and late changes converge across peers (f3e3ad34). The peer topology simulation in tests/sim/peer.rs (9c084364). Docs in docs/data/sync.md and docs/testing/README.md (33934d06, plus one sentence in 5dd3e72f).
  - Commits, all signed: fc21b7d2, 5dd3e72f, 90d76237, f3e3ad34, 9c084364, 33934d06, and merge b761c607 of feat/sync at 907b371b.
  - Scenarios: anchor, two admin desktops, and phones exchanging with random peers under partitions, crash failpoints, and anchor outages; removals concurrent with edits; mutual admin removals; handover races; planned and loss handovers; anchor restore from backup and clone; quarantine; late change restore and discard. Calendar is excluded. Peer sim: 48 seeds of 60 steps in 4 shards. Star sim: 48 seeds of 60 steps in 4 shards. All converge; no failing seed.
  - Checks after the merge: ganbaru-sync lib 300 passed; ganbaru-sync-replica 81; ganbaru-sync-contracts 101; tauri-app sync filter 47; ganbaru-db 133; clippy workspace all targets clean; fmt clean.
  - Budgets (release, this host, after the merge):
    - Capture 77 µs (budget 200 µs).
    - Removal exclusion 328 ms and 351 ms (budget 1 s).
    - Fork exclusion: member 375 ms and 440 ms, anchor 274 ms and 344 ms (budget 1 s).
    - Rederive 144 ms (no budget).
    - Calendar import seal 1.98 s and apply 1.43 s (recorded only).
    - Baseline failures, not tuned: seal 83 ms (20 ms), apply 165 ms (50 ms), Projects batch 514 ms (50 ms).
    - Retention failed twice. The member took 305 ms against a 277 ms limit and 298 ms against a 282 ms limit; the limit is the anchor median times 1.1. At feat/sync f948d1d2 it passes: member 287 ms, anchor 276 ms, limit 304 ms. On this branch the member's absolute time is within baseline noise (baseline spread 264 to 430 ms), but the anchor median dropped to about 254 ms, so the relative limit tightened. Not tuned, deferred to the engine profiling follow-up (coordinator decision); the limit and the test are unchanged.
  - Plan-silent decisions:
    - Fork republish covers every live group the stored branch wrote, including stored but unapplied operations.
    - The anchor republishes groups that operations excluded by a removal wrote, but only in an epoch a handover opened. The charter epoch has no earlier anchor, and republishing there would bury late values.
    - Each republished value is tagged by hash in device-local `sync_restated_groups`, so restoring a late change does not count the restatement as a newer edit. Tags drop once the row has no undecided late change.
    - `sync_stale_rows` keeps rows that storing an excluded operation may have made stale; the next engine write materializes them.
    - `sync_row_creations` plus `sync_rows.creation_pending` (the Uncreated root): a row whose creations are all excluded stays hidden, with any honest changes kept.
    - Captures seal before an authorization entry that would stop the local writer applies.
    - Each re-seal of a fork rotates the writer first. The reseal replay is bounded.
    - `SealSlot::may_seal` takes `&mut self`, so apply futures stay `Send` without `SealSlot: Sync`.
    - Harness: Step 6 services are modelled in the harness: a proposal outbox, quarantine as harness-local store and forward refusal, and restored-anchor self-handover. Late decisions are random-first and alternating, and close before quiesce. A restore takes a backup on demand. The trace uses readable hub names. Compaction stays in the star sim only.
  - Open questions for step 6:
    - Restatement tags are anchor-local, so a non-anchor admin restoring a late change could still see the restated version as newer.
    - A user edit that writes exactly the restated value is treated as a restatement.
    - No focused store-time test covers stale rows; sim seed 21 covers them.
    - App sealing and acknowledgement rotation may orphan captures.
    - The cutoff can fall below confirmations after a handover without a vector.
    - Rows created before the row-creations migration have no creation entries.
  - Deviation: merge b761c607 carries git's default message instead of the `merge:` prefix; it was left as is because amending is not allowed.
- [x] Step 6: anchor service, outbox, handover, recovery kit, and quarantine.
  - 6b branch `feat/sync-anchor-handover` from feat/sync 419c6128.
  - Commits: 1ef29ecf (items 3 and 6), e679b02d (item 5).
  - Item 3: `pass` answers hub proposal requests left queued by a pass that ended early with "sync is not ready on this device", next to recovery and reset requests.
  - Item 6: at the 256 bound `Outbox::add` drops the oldest final rejection; only an outbox full of waiting proposals refuses. `queue_reconciliation` stops adding when `has_room` is false and leaves the rest for a later pass instead of failing the coordinator pass. A rejected reconciliation change evicted this way is proposed again and rejected again, which is bounded churn.
  - Item 5: no backfill migration. Rows the migration left at 0 only order chain opening (`first_opener`), not merge results; they tie on 0 and open by writer id before any chain stored later, and `next_stored_order` numbers new rows above them. No external vaults exist. Test `chains_stored_before_storage_order_existed_open_by_writer_and_converge`.
  - Items 1 and 2 (3fa4f67f): every proposal names the hash of the charter its log starts from (PROPOSAL_VERSION 2, golden vectors reblessed), and the anchor refuses a proposal of another charter with `WrongSpace`, so outbox proposals signed before a Reset never replay into the new log. The charter hash changes only on Reset; a handover keeps it. `remove_installation_state` deletes the outbox and carry-forward bundle before the keys and the writer record; the reverse crash order is unreachable because the database reset runs first. The sequenced high-water records the charter and is cleared on writer activation when the space has no charter or another one. A record ahead of the selected head by exactly one revision is a rolled back seal; further ahead is a restore. The proposal format change takes the next handoff protocol number at merge.
  - Item 8: docs/data/sync.md says content a client never sent before a Reset may not come back with its carry-forward bundle when it pairs again.
  - Item 7 (open question for step 9): a restatement cannot be recognized from the operation alone on every member. It carries only the new value with its clock and context; its value hash equals the prior winner's, which a member may have superseded or never held. Recognizing it needs a marker in the operation format, which step 9 should decide with non-anchor admin restores.
  - 894be1ba: quarantine, handover catch-up, and item 4.
  - Implementation check (step 5 open question, freeze clearing): held in the engine. `advance` recomputes `frozen_at` for the new anchor only from freezes at or past the handover revision, so a device that forked earlier and becomes anchor is not frozen; `a_forked_anchor_continues_its_log_through_a_handover_to_itself` covers it. No service code needed.
  - Item 4 (done): the coordinator presents the charter at pairing whether or not it anchors, because the coordinator role (hub, vault ownership, pairing listener) does not move with the anchor in this step; a linked device learns the anchor from the log. `PinnedCharter::verify` checks space and person key only. A reply without a charter is refused by the device; the coordinator refuses with an error when its sealer yields none and keeps the link, because unlinking would make the anchor remove an enrolled device id for good.
  - Implementation check (earlier step): `CoordinatorPin.generation` is the whole-vault ownership generation used by transfers and bundle requests, so a handover cannot increment it; the fallback branch held. Because the coordinator does not move in 6b, the pin keeps naming the coordinator and members read the anchor from the log; the separate anchor field and the repin land with the peer book in step 9.
  - Plan-silent decision: in 6b the anchor moves but the coordinator stays. There is no peer book, a pinned desktop neither advertises nor serves, the log holds only TLS fingerprints, and the coordinator carries vault ownership. Members keep dialing the coordinator until step 9; "pairing works on B" is deferred to step 9.
  - Quarantine: device-local `quarantine.json` beside the outbox, kept across Reset sync state (the person's decision about a device id stands). Hub and client skip quarantined writers' operations and keep paging; the listener refuses quarantined linked peers after the TLS handshake; a member never exchanges with a quarantined coordinator.
  - Handover catch-up: `ProposalRejection::CatchingUp` (wire 14); a new anchor refuses removals while a writer named in the handover's old anchor vector is unknown locally; cutoffs take the larger of the local stored sequence and the old vector.

## 5a2633ff feat(sync): detect a restored anchor and continue its log in a new epoch
- The writer record keeps the sequenced epoch, revision, and entry hash under the charter. One revision ahead of the selected head is a rolled back seal; further ahead or a later epoch is a restore.
- The hub anchor waits up to ten minutes (immediately with no linked peers) for members to return the log, answering proposals NotAnchor, then seals a self-handover with no old anchor vector. Deviation from the plan's "one exchange round with every reachable member": recorded as a bounded window.
- A log holding only its charter cannot be continued by a handover (contracts refuse continues_from_revision 0); the pass reports an error asking to link the devices again.
- Test: a_restored_anchor_sees_its_lost_entries_and_hands_over_to_itself (replica outbox).

## 26e2f602 fix(sync): keep content syncing when a restored anchor holds only its charter
- With a head at revision 0 the hub skips the restore handover and keeps exchanging content, logging the condition, instead of failing every pass.

## 75d80b6b feat(sync): carry desktop admin keys at pairing and add the proposal forward messages
- A desktop sends its admin public key in Enroll (optional `sync_admin_key`); the coordinator keeps it on the linked peer for computers only, so an admin can later promote that desktop. `ensure_admin_key` creates the key without a recovery seed.
- Wire: `SyncForward { settled }` and `SyncForwarded { proposals }` (at most 32 each) let an anchor that is not the hub collect proposals the hub holds and report final rejections back.

## 861b09da feat(sync): hold proposals for an anchor that is not the hub and forward them
- A hub that no longer anchors keeps proposals in its outbox (`hold_for_anchor`) and answers the chain revision, the recorded final rejection, or NotAnchor. Only the device the log names as anchor collects them (`collect_forwards`, 32 per answer, 768 KiB budget) and reports final rejections in its next forward.
- Old anchor vector at take-anchor: the new anchor's own stored vector after a complete exchange with the hub, a superset of what the old anchor stored at that time.
- Test: a_promoted_desktop_takes_the_anchor_and_collects_what_the_hub_holds.

## Cloud host continuation (2026-10-25), feat/sync still e76a492, no merge needed
- 6ca2406 feat(sync): add authority commands to take the anchor, promote desktops, quarantine devices, and regenerate the recovery kit. The previous agent's staged work, reviewed and committed unchanged after fmt and clippy: `service/anchor.rs` (authority actions queued for the pass with oneshot replies, take anchor only after a current exchange, `anchor_as_member` forward collection, `answer_forwards` on the coordinator), commands `sync_quarantine_{list,device,lift}` (desktop and mobile) and desktop-only `sync_take_anchor`, `sync_promote_device`, `sync_regenerate_recovery_kit`, permission set and runtime registration tests; reconcile runs on any admin coordinator and holds proposals after a handover; replica `unlinked_devices` and `rotate_recovery_seed`.
- f6db357 feat(sync): show, forget, and use the recovery kit on a desktop. `AuthorizationView.recovery_admin_key` (engine); replica `pending_recovery_seed`, `forget_recovery_seed`, `recovery_promotion`; desktop commands `sync_recovery_kit_pending` (code plus whether the log names it), `sync_recovery_kit_saved` (deletes the seed), `sync_use_recovery_kit`. The restored-anchor self-handover now reads only the admin key, so a saved and forgotten seed is never recreated outside founding. A quarantine on a non-admin desktop stays local instead of logging an error. Tests use the file `SyncKeyStore` test double: `the_key_store_forgets_a_saved_recovery_kit_until_the_next_charter`, `a_recovery_kit_promotes_a_member_desktop_that_then_takes_the_anchor` (foreign kit and bad checksum refused).
- 8d23ce7 test(sync): `a_hub_stores_nothing_from_a_quarantined_device_until_the_quarantine_lifts`.
- 2c00520 docs(sync): handover, restore, recovery kit, quarantine, `SyncForward` row (docs/data/sync.md), device-local state (docs/data/schema/sync.md), status line (sync-topology-and-authority.md), recovery seed and admin keys (docs/data/security/network-and-privacy.md).
- Plan-silent decision, recovery kit use: the engine requires a handover's new anchor to be an admin desktop already, so kit use proposes `set role` admin for this enrolled desktop signed by the recovery key, sequenced by the live anchor; the desktop then takes the anchor. A forced handover by the kit or another admin while the anchor is lost (and kit use on a fresh, unenrolled desktop) needs peer exchange and the engine relaxing that check: step 9. The kit is a code only; the plain text file is a UI concern for step 8.
- UI: the plan puts Reset, recovery kit create and use, handover, and quarantine controls in step 8 (authority UI), so step 6 keeps backend commands only.
- Implementation checks for step 6: CoordinatorPin.generation, fallback branch (recorded above; repin with the peer book in step 9). Carry-forward of the anchor's kind 4, handoff recovery keeps the log, person key at pairing, store_many quarantine refusal (settled from source; hub and client skip `Quarantined` and keep paging, now covered end to end): primary branches held. Freeze clearing held in the engine.
- PROTOCOL_VERSION: the orchestrator bumped it to 14 in b1c3ffd (I did not bump it). The wire changed in this branch (proposal format v2 with the charter hash, `SyncForward`/`SyncForwarded`, `sync_admin_key` in Enroll, rejection code 14 `catching_up`); the orchestrator assigns the number at landing. No migration, no manifest change, no frontend change.
- Not covered by a focused test: the listener's refusal of a quarantined device after the TLS handshake (`transport/server.rs`), which needs a live TLS listener.
- Android compile check: not run (no toolchain on this host).
- Notes for steps 7 to 9: step 8 wires the commands above (pending kit shown once, confirm saved, regenerate, use, take anchor, promote, quarantine, Reset); step 9 adds forced handover, repin, discovery key in removal entries, and trust in final rejections from any hub.
- Final checks at 2c00520 (CARGO_TARGET_DIR engine): fmt clean; ganbaru-sync lib 376 passed (7 ignored); ganbaru-sync-replica lib 102 passed (1 ignored); ganbaru-sync-contracts lib 105; ganbaru-handoff lib 102; ganbaru-tauri-app lib 1035; clippy workspace all targets clean; `cargo check -p ganbaru --bin ganbaru` clean. ganbaru-db not run (no migration). Frontend checks not run (no frontend change). Android: not run (no toolchain).

## Fix round after the 6b review (2026-10-25, HEAD b1c3ffd, feat/sync dff5fb8 goal docs only, no merge)
- 1 (high, confirmed): hub sequencing never recorded the writer commit, so a restored database still matched the record. Every hub sequencing step now records the commit (reconcile, hub proposals, self-handover; the linked anchor already did). `check_writer` also retires a writer whose database is behind the authorization high-water, so a restored anchor seals nothing under the old writer, not even domain captures in the wait window; `anchor_ready` rotates once more if a restore shows up with an active writer. Tests: the outbox restore test now expects a successor; `a_restored_anchor_hands_over_under_a_successor_that_members_accept` (member that holds the lost entries accepts the handover, no fork); `a_restored_anchor_takes_back_the_entries_members_return_within_its_window`.
- 2 (medium): a linked desktop that anchors runs `anchor_ready` (members always reachable through the coordinator) before `anchor_as_member`.
- 3 (medium): `settle_sequenced` keeps the high-water when the space has no charter; founding refuses while the record names one (`reset::refuse_refounding`); Reset drops it first (`reset::forget_high_water`), so a Reset that crashes after clearing the database still founds anew. Covered in the interrupted-Reset test.
- 4 (medium): outbox entries the chain holds are kept as held at their entry's epoch (`Engine::proposal_position`), dropped once a later epoch still holds them, waiting again when a handover superseded them, and evictable after final rejections. Take anchor merges the coordinator's stored vector from the exchange (`OnlineWait.known`) into the old anchor vector. Test `a_proposal_the_old_anchor_sequenced_after_the_handover_point_is_sequenced_again`.
- 5 (medium): `authority::regenerate_recovery_kit` queues the proposal before storing the seed (withdraws it if the store fails), refuses while the waiting kit is active or its proposal still waits; `forget_recovery_seed` refuses a kit the log does not name. Test covers a full outbox, both refusals, and saving after sequencing.
- 6 (low, fixed): a writer retired because its chain was lost marks the high-water `restored`, so one revision ahead is a restore. Test `a_restore_that_loses_one_recorded_entry_is_not_a_rolled_back_seal`.
- 7 (low, not fixed): cutoffs could take the hub's reported vector as a floor (the engine already takes `max(stored, catch_up)`), but writers of the quarantined device the anchor never stored a genesis of have no row and would still get cutoff 0, and every sequencing path would need the hub vector. Left for step 9, where a non-hub anchor exchanges with peers directly.
- 8 (low, fixed): the listener treats an unreadable list as refusing no one, and the app source keeps the last list it read per vault. Test `the_listener_refuses_a_quarantined_device_and_survives_an_unreadable_list` (real TLS listener).
- 9 (low): debug forms of authority actions and answers redact kit codes. The pending command returns the code while the seed waits; documented, and the confirmation that ends it is the step 8 UI.
- 10 (low, not fixed): a charter nonce changes the charter format, golden vectors, and bounds across contracts, handoff, and app. A collision needs a founding in the same millisecond with the same keys and discovery key after a Reset that unlinks every device first; recorded for step 9 or a later contracts change.
- 11 (low, fixed): `quarantine::PullSkips` keeps what pulls turned away across exchanges until the list changes. Test `a_client_pulls_a_quarantined_backlog_once_until_the_quarantine_changes`.
- Commits: 4342ad3f fix(sync): retire a restored anchor's writer, keep superseded proposals, and harden the recovery kit and quarantine; db693d4f docs(sync). One code commit because service.rs and the shared test modules interleave every finding.
- Checks at db693d4f (target engine, CARGO_INCREMENTAL=0): fmt clean; ganbaru-sync 376 (7 ignored); ganbaru-sync-replica 107 (1 ignored); contracts 105; handoff 102; tauri-app 1036; workspace clippy all targets clean; `ganbaru` bin check clean. Android compile check: not run (no toolchain on this host).
- feat/sync moved to 15bbd0bb (milestone 8 step 3, code) during the run, so 30c8b1b4 merges it (`merge: feat/sync into feat/sync-anchor-handover`, clean; protocol stays 14; latest migration 20261025091420 is m8's, this branch adds none). Checks after the merge: fmt clean; ganbaru-sync 387 (7 ignored); replica 107 (1 ignored); contracts 105; handoff 102; ganbaru-db 150; tauri-app 1036; workspace clippy all targets clean; `ganbaru` bin check clean. Android: not run.
- feat/sync moved again to afdcb488 (milestone 9 step 4, chat replication) while the post-merge checks ran; not merged in this round, the orchestrator merges at landing.
- 418021cf merges feat/sync afdcb488 (milestone 9 step 4, manifest 15), clean, protocol stays 14. Checks: fmt clean; ganbaru-sync 400 (7 ignored); replica 107 (1 ignored); contracts 105; handoff 102; ganbaru-db 151; ganbaru-chat 168; tauri-app 1042; workspace clippy clean; bin check clean. Android: not run.

## Delta review fixes (after 418021cf)
- 3205baa4 fix(sync), d761ab6b docs(sync).
- M1 (confirmed): a member pruned only when something waited. `client::prune_and_replay_outbox` prunes first (held entries superseded by a handover wait again) and the app uses it. Test `a_members_proposal_the_handover_superseded_reaches_the_new_anchor` (phone outbox, hub non-anchor holds it, laptop collects it).
- M2 (confirmed): `wait.known` was the local vector. `OnlineWait.hub` now keeps `Exchanged.hub` without writers this client's pulls turned away as quarantined (`authority::without_writers` over `PullSkips`); take anchor merges that. Test `a_new_anchor_catches_up_with_the_hubs_writers_but_not_quarantined_ones`.
- L3 (confirmed): `sequenced_check` treats a record of an earlier epoch than the selected head as current. Unit cases added.
- L4 (fixed): `wait.known` includes the pull skips, so the long poll does not return for writers this client quarantined. No separate test (service loop).
- L5: Reset local data keeps the writer record, and the moved database can be restored, so refusing to found is kept; the message now says to restore a database copy that holds the space or use Reset sync state.
- Checks at d761ab6b: fmt clean; ganbaru-sync 400 (7 ignored); replica 109 (1 ignored); tauri-app 1042; workspace clippy clean. Android: not run.
  - Cloud session reviews (fresh Opus, read-only): first review fix first (11 findings), fix round 4342ad3f and db693d4f; delta review fix first (member outbox prune, hub vector at take-anchor, epoch-aware restore check, pull skips in long polls, founding message), fixed in 3205baa4 and d761ab6b; short delta review merge with follow-ups. Merges of feat/sync 30c8b1b4 and 418021cf. Landed as 78910455 (unsigned, approved for this goal). Android compile check: not run (no toolchain on the cloud host).
  - Deferred to step 9: finding 7 (cutoffs of a non-hub anchor that quarantined a device; needs the coordinator vector in every sequencing path); the epoch shortcut in the writer sequence guard (`writer.rs` ~652) could keep a writer after a crash between seal and commit record followed by a restore while another device took the anchor (narrow; apply the shortcut only in `restore_check`); no service-level tests for the member prune call site (`service.rs` ~2548), the take-anchor hub vector wiring (~1183), pull skips in long polls, or the founding message. Declined: a random nonce in charters (a Reset regenerates admin keys and the recovery seed, so charters already differ).
  - Part 6a (anchor service): done on `feat/sync-anchor-service`, merged with `feat/sync` and checked.
    - Final checks after the merge: `ganbaru-sync --lib` 372 passed (7 ignored), `ganbaru-sync-replica --lib` 93 passed, `ganbaru-sync-contracts --lib` 105 passed, `ganbaru-handoff --lib pairing` 39 passed, `ganbaru-tauri-app --lib` sync, runtime, and carry filters 175 passed, `ganbaru-db --lib` 149 passed; clippy (workspace, all targets) and fmt clean; `ganbaru` desktop bin check passes; Android check passes with only the baseline warnings.
    - Commits: `bef5a8b6 fix(sync): open chains in per-space storage order and drop stale winners` (engine follow-up 4 part C); `5a182d17 feat(sync): count enrolled devices from the authorization log`; `50e72eb2 feat(sync): add the proposal outbox and anchor sequencing`; `121bfbfa feat(handoff): carry the charter at pairing and add sync proposals over protocol 13`; `71e471a5 feat(sync): reconcile through the outbox and sequence proposals sent through the hub`; `0b509f19 feat(sync): reset sync state on the coordinator` (with the carry-forward and orphan-capture tests); `099b6707 docs(sync): describe anchor sequencing, the outbox, and Reset sync state`; signed merge of `feat/sync` at `c77db639` (`merge: feat/sync into feat/sync-anchor-service`; manifest stays 13, protocol 13, migration `20261025051154` still sorts last); `fac580f6 fix(sync): build the proposal answer only on desktop hubs` (Android unused warning).
    - Follow-up 4 part C: apply clears the cached winner after a heads reload, so a stale winner never decides a later group; chain opening orders by a new per-space `sync_ops.stored_order` (migration `20261025051154_sync_stored_order`, index `idx_sync_ops_stored_order`) instead of `stored_at_ms`, which stays wall time for stability and dormancy because a stepped-back clock must not reorder chains. Query plan case added. `ganbaru-db --lib` 149 passed, `ganbaru-sync --lib` 367 passed.
    - Counted set: `Engine::counted_devices` returns every enrolled, not removed device of the selected log besides the local one, the anchor flagged and always counted; enrollment time is the stored time of the selected entry (charter time for revision 0). The app stability report reads it instead of linked peers. Compaction already keeps kind 4 bodies (existing compaction test).
    - Outbox and sequencing: `Outbox` at `app_config_dir/sync/<vault id>/outbox.json`, owner-only atomic writes, signature-verified on load, bounded to 256 proposals and 4 MiB, deduplicated by proposal hash; only `NotAnchor`, `EpochFrozen`, and `HandoverConflict` keep a proposal waiting, the rest are recorded as final. `authority::sequence_outbox` sequences, settles, then prunes proposals the selected chain holds. Revision high-water: `SequenceReservation::reserve_entry` writes the revision and entry hash into the writer record (`sequenced`) before each entry is sealed; successors carry it.
    - Charter at pairing and protocol 13: `Enrolled` carries the anchor's charter and person public key over the pinned TLS session; the client verifies and pins both (`PinnedCharter` in `CoordinatorPin.charter`) and installs the pinned charter on its first pass, refusing a different one. `SyncPropose` and `SyncProposed` carry one signed proposal and its revision or rejection code.
    - Reconciliation each pass: the coordinator compares linked peers with the selected log, queues enroll and remove proposals signed by the admin key into its outbox once per change (`authority::queue_reconciliation`), and sequences them; a crash between pairing and sequencing is repaired by the next pass. Proposals sent through the hub reach the anchor's pass by a queued request with a oneshot reply; clients replay their outbox after each exchange and prune entries the applied chain holds.
    - Reset sync state: `local::reset_sync_state` clears every `SYNC_TABLES` and `SYNC_LOCAL_TABLES` table except the `sync_apply_state` singleton (domain triggers read it; `applying` is set to 0), in one engine write. `reset::reset_database` and `reset::remove_installation_state` (writer keys the record names, record, carry bundle, outbox; device and anchor keys stay) back the desktop-only `sync_reset_state` command, which queues a request answered inside the coordinator's pass; a client refuses it. The pass unlinks every peer linked for the vault first. Fixed on the way: founding read the charter before the space row existed (`UnknownSpace` on a fresh or reset vault), and founding could bypass `refuse_legacy_state`.
    - Earlier-step checks: person key at enrollment, primary branch (the pairing response carries the person key with the charter over pinned TLS and `install_charter` verifies it). Handoff recovery keeps the log and enrollments: already true, no change. Carry-forward includes the anchor's unsent kind 4: held, test `replacement_carries_the_anchors_unsent_authorization_entries`. Step 5 orphan captures: held without a fix, test `rotating_after_an_inactive_writer_seals_every_pending_capture` (captures are not bound to a writer and a refused seal rolls back, so the successor seals them after its genesis, for retired, revoked, and forked).
    - Doc-only notes: a user edit equal to a restated value is a benign restatement and needs no special case; rows created before the row-creations migration have no creation entries, which is acceptable because old development vaults are unsupported.
    - Plan-silent decisions: Reset is a backend command only, with no settings control in 6a; it unlinks linked peers on the coordinator; device and anchor keys survive, so the new charter differs only in founding time; only `NotAnchor`, `EpochFrozen`, and `HandoverConflict` keep a proposal waiting in the outbox.
    - Public APIs for 6b: `reset::{reset_database, remove_installation_state}`, `local::reset_sync_state`, `outbox::Outbox`, `authority::{queue_reconciliation, sequence_outbox}`, `client::{propose, replay_outbox}`, `HubProposals`, `PinnedCharter` in `CoordinatorPin.charter`, `proposed_to_wire` and `proposed_from_wire`, `WriterRecord.sequenced` (revision high-water), `Engine::counted_devices`.
    - Open questions for 6b: the discovery key for removal entries is fresh random rather than read from the log; finally rejected outbox entries linger until pruned by the 256 bound; restore detection can compare `WriterRecord.sequenced` with the selected revision; the client outbox fills only from future admin flows; where the Reset control lives in Data settings.
    - Landed as 419c6128. Review (fresh Opus, read-only): merge with follow-ups, carried into 6b part 1: proposals replayable into a new log after Reset (bind proposals to the charter hash, delete the outbox before keys); `WriterRecord.sequenced` can run ahead of the database or survive Reset (clear it when the charter is absent or changes, treat ahead by one as a crash); queued hub proposal requests unanswered when a pass errors or exits early; pairing compares the charter's anchor instead of the selected log's anchor and accepts a reply without a charter; `stored_order` has no backfill (existing rows 0); final rejections count toward the outbox bound and a full outbox fails the coordinator pass; restatement detection question unanswered; tests for a crash mid Reset and replay after Reset; docs note that content a client never sent before Reset may not return. Step 9: removal entries' discovery key unused; peer exchange must recheck trust in final rejections from any hub.
- [ ] Step 7: Android Keystore wrapping.
- [ ] Step 8: authority UI.
- [ ] Step 9: LAN peer exchange, after the external security review.
- [ ] Step 10: docs and automated acceptance.
- [ ] Step 11: physical acceptance.
