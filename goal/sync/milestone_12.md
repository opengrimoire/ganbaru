# Milestone 12: space keys, encrypted backups, relay, and always-on anchor

**Status: Planned.** Part of the [concurrent sync goal](README.md). This file is the implementation plan for space content keys with epochs, the recovery identity and recovery kit, encrypted backups, the self-hosted blind relay, and the always-on anchor, with one new crate, `crates/ganbaru-sync-relay`, for the server binary. It runs last on the engine track after [milestone 11](milestone_11.md). Every step except the anchor step needs only milestone 11; the anchor step needs all of [milestone 10](milestone_10.md). [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract, with [network and privacy](../../docs/data/security/network-and-privacy.md) for the network surface and encryption, [sync topology and authority decisions](../../docs/architecture/decisions/sync-topology-and-authority.md) for the relay and anchor roles, and [supply-chain security](../../docs/data/security/supply-chain.md) for the dependency review. Each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- Two devices that never share a network converge through a relay that the user runs on a VPS, NAS, or home server. The relay host sees ciphertext, sizes, timing, and opaque routing identifiers, never content, device names, or membership.
- The same binary in anchor mode is an enrolled member on hardware the user controls. It is the space's anchor, a hub reachable from every network, a continuous backup, and a bootstrap source for new devices.
- Removing a device rotates the space key in the same seal. The removed device can read nothing pushed to the relay afterwards and can no longer push to it.
- Every backup is encrypted. Desktop gains backup and restore to a user-chosen file; Android Downloads backups use the same format. Restore defaults to an independent copy and never rewinds a linked space.
- One printed recovery kit opens every backup and restores admin authority after every device is lost.
- A non-technical user sets up a server with one container command or one binary and one QR scan, with no domain name, certificate authority, or account.

## Scope

In scope:

- Device encryption keys, space content keys with epochs, the key chain, HPKE key envelopes, rotation on removal and on kit regeneration, the recovery identity, and the recovery kit.
- The relay record format and the relay asset chunk format.
- A Tauri-free member stack (listener and sync service) that the app and the server binary share.
- `crates/ganbaru-sync-relay`: relay mode (blind store and forward) and anchor mode (headless enrolled member).
- Client relay passes on desktop and Android, with a dial exception for admin-configured server endpoints.
- The encrypted backup format, desktop backup and restore commands, the Android format change, and the restore rules.
- Delivery status that distinguishes saved, received by relay, and confirmed.
- Settings flows for servers, the recovery kit, and backups, in English and Spanish.
- Static Linux binaries, a container image, and a setup guide.
- The supply-chain review of every new crate, measurements, and physical acceptance across networks.

Out of scope:

- Multi-person spaces, contact request delivery through a relay, and the relay address in contact cards (sharing goal).
- Padding and cover traffic against size and timing analysis.
- More than one relay per space, relay federation, and relay-to-relay replication.
- Native macOS or Windows server builds; the container covers those hosts.
- Encrypting the vault at rest on devices or on the anchor.
- Bootstrapping a new device through the relay. Enrollment and bootstrap always use direct links: enrollment reaches the anchor (milestone 11 pairs only on the anchor), and bootstrap comes from any serving member, including the always-on anchor.
- A shared-folder transport (rejected, see [Rejected alternatives](#rejected-alternatives)).

## Prerequisites

- [Milestone 2](milestone_2.md): asset classes (`AssetClass { name, max_bytes }`, `{sha256}.{ext}` paths) and adapter `asset_refs` (step 8), asset transfers streamed to temporary files under the 64 MiB `MAX_SYNC_ASSET_BYTES`, host-free `MaterializeHook` post-apply hooks with a change sink, `ContentAddressedStore`, and the generic `prepare_snapshot_copy` that empties Local tables and resets Local columns to their materialization values.
- [Milestone 3](milestone_3.md): the address policy, `connect_coordinator`, its protocol bump (the next protocol version, per the README [wire versions rule](README.md#parallel-work-reservations)), `SyncHost` with `HeadlessHost`, `crates/ganbaru-mobile-sync` with its WorkManager workers and JNI `runPass`.
- [Milestone 11](milestone_11.md): the anchor-sequenced, hash-chained authorization log carried as kind `4` operations, whose extensible entry kind lets receivers store, forward, and chain unknown kinds while deriving nothing from them; device keys and writer certificate v2; admin keys and the recovery kit; anchor epoch and handover; removal cutoffs and the anchor mark; Android Keystore wrapping for installation secrets; every member serving peers.
- [Milestone 10](milestone_10.md): steps 1 to 4 merge before milestone 11, so step 4 here can rely on the asset sweep, its async mutex, and archive creation (backup and bootstrap snapshot), all in `ganbaru-sync-replica`. Step 8 needs all of milestone 10: scrubbed whole-database snapshot bootstrap, signed acknowledgements (kind `3`) carrying applied vectors, the 90-day dormancy rule, compaction after causal stability, and retired handoff and ownership.
- An external review of the protocol composition. It is an exit gate, not a step: until it passes, the relay setup flow is available in release builds only behind a Settings toggle labeled as unreviewed. Encrypted backups and anchor mode are not gated, because backups replace plaintext archives and the anchor adds no transport cryptography beyond mutual TLS.

## Current state

Verified on 2026-10-21; re-verify when the milestone starts.

- The operation header already carries `authorization_revision: u64` and `key_epoch: u32`, documented as "zero until multi-person spaces" and "zero until operation encryption" (`crates/ganbaru-sync-contracts/src/op.rs:361-364`), encoded at :498 and decoded at :760. `FORMAT_VERSION` is `1` (:42); kind `3` is reserved (:64, commented as checkpoints; milestone 10 assigns it to signed acknowledgements).
- Signing domains live in `crates/ganbaru-sync-contracts/src/signing.rs:13-23` (`ganbaru-sync/op/v1`, `op-hash/v1`, `value/v1`, `writer-certificate/v1`, `personal-space/v1`, `writer/v1`); Ed25519 comes from `ring` (:125). The crate depends only on `ganbaru-contacts`, `ring`, and `sha2`.
- Writer keys use the keyring service `com.ganbaru.sync` with account `writer-key:{vault_id}:{writer_id}` (`crates/ganbaru-sync-replica/src/writer.rs:46-59`); the file store replaces `:` with `-` (:102). `FileKeyStore` is compiled only for `mobile` and tests and `KeyringStore` only for `desktop` (:51-53, :97-105), and `keyring` is a dependency on every non-mobile target (`Cargo.toml:29-30`), so anchor mode ungates the file store and the server binary does not link `keyring`.
- `ganbaru-handoff` defines `PROTOCOL_VERSION = 6` and a 1 MiB control frame (`crates/ganbaru-handoff/src/protocol.rs:16-17`), 600 KiB sync pages and asset chunks, and a 32 MiB asset cap (`src/protocol/sync.rs:11`, :163, :167). It already depends on `rustls`, `rcgen`, `qrcode`, and `tokio`.
- The listener lives in the app crate. `CoordinatorServices` holds `CoordinatorSender`, `ContactSender`, `SyncHub`, `EnrollmentApprovals`, and `LanAdmission` (`apps/client/src-tauri/app/src/vault/handoff/transport/server.rs:81-88`), and its imports reach app modules (:3-21), although it names no Tauri type. The sync service imports `AppHandle` and spawns on `tauri::async_runtime` (`apps/client/src-tauri/app/src/sync/service.rs:35`, :221).
- The hub opens its vault through `OpenHubVault` and answers waits after 12 s (`crates/ganbaru-sync-replica/src/hub.rs:27-47`).
- Backups exist only on Android. `vault_backup_to_downloads` and `vault_pick_and_restore_backup` are `#[cfg(target_os = "android")]` (`apps/client/src-tauri/app/src/vault/backup.rs:1078-1223`); the other stubs exist only for tests and iOS (:1224-1236), and `runtime/desktop.rs:599-631` registers no backup command. The archive is a plaintext zip with MIME `application/zip` (:27) and a 100 GiB cap (:29). The Android backup snapshots the database without `prepare_snapshot_copy` (:1086-1087), while `create_handoff_archive` runs it (:329-348). Restore replaces the only vault in place (`replace_vault`, :1185); Android has one vault path (`vault.rs:714`).
- Docs already describe desktop backups as current: `docs/data/architecture.md:37` and `AGENTS.md:154`. The Android platform doc says system backup excludes the "unencrypted live vault until a deliberate encrypted design exists" (`docs/platforms/android/native-services-and-data.md:19`).
- Rollback copies: `.database-rollbacks/`, at most 3 replacement copies (`apps/client/src-tauri/app/src/vault/database_rollback.rs:21-26`), with `retain_replaced_vault` at :309.
- `Cargo.lock` already holds `ring` 0.17.14, `rustls` 0.23.46, `tokio-rustls` 0.26.4, `chacha20` 0.10.2, `hkdf` 0.13.0, `sha2` 0.11.1, `getrandom` 0.4.2, `rand_core` 0.10.1, `hybrid-array` 0.4.15, `subtle` 2.6.1, `zeroize` 1.8.2, and `zip` 9.0.4. It has no `chacha20poly1305`, `aead`, `poly1305`, `hpke`, `x25519-dalek`, or `curve25519-dalek`. `rustls` 0.23.46 exposes `export_keying_material` on both connection sides.
- The contract still says there is no relay or internet traversal (`docs/data/sync.md:15`), plans the relay and `ganbaru-sync-relay` (:144-146), lists operation key epochs (:184) and relay receipts (:186), the encryption composition (:209-211), encrypted asset manifests (:223), backups with "backup pins" (:225), Settings for anchor and relay (:231), and the delivery row (:245). [Sync engine decisions](../../docs/architecture/decisions/sync-engine.md) require a relay to store only opaque records (:10) and accept plaintext operations at rest (:42).
- No "Docs differ until" row in the [goal README](README.md#decisions-for-this-plan) names milestone 12.

## Design

### Trust boundaries

- Direct links between members, including the always-on anchor, stay protected by pinned mutual TLS only. Operations travel and rest as plaintext signed envelopes, as today.
- Record encryption applies only where an intermediary stores records: the relay. A member encrypts at relay ingress and decrypts at relay egress.
- The always-on anchor holds keys and plaintext. The setup guide says so and recommends relay mode for rented servers and anchor mode for hardware the user owns.
- Backups are encrypted because they leave the device's protection (shared Downloads, user-chosen folders, cloud drives).

### Keys

**Device encryption key.** Each installation holds one static X25519 key pair per vault, generated at first need. It is stored like the writer key: keyring service `com.ganbaru.sync`, account `encryption-key:{vault_id}` on desktop; the milestone 11 Keystore-wrapped file store on Android; a `0600` file in the server data directory in anchor mode. It is separate from the milestone 11 Ed25519 device key, which signs. Its key id is `SHA-256("ganbaru-sync/device-key/v1", 0, public_key)[..16]`. The device publishes it in a `DeviceKey` authorization entry signed by its milestone 11 device key; the anchor sequences it when that device is enrolled and neither removed nor quarantined. No admin signature is needed, because the entry only adds a recipient for an already enrolled device.

**Space content keys.** Each epoch has a 32-byte random key `K_e`, with epochs numbered from 1; 0 means no key. The anchor generates epoch 1 when it sequences the space's first `RecoveryKey` entry, because encryption matters only once a backup or server exists. An unlinked vault has no anchor and stays at epoch 0. Every use takes a separate subkey from `K_e` through HKDF-SHA256 (`ring::hkdf`) with info `ganbaru-sync/<purpose>/v1`, a zero byte, then `space_id`, for the purposes `relay-record`, `relay-asset`, `relay-access`, and `backup-wrap`. The asset id key derives from `K_1` (`relay-asset-id`), so asset ids stay stable across rotations.

**Envelopes.** HPKE base mode, single shot, suite DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305, with info `ganbaru-sync/space-key/v1`, a zero byte, then `space_id`, `epoch`, and `recipient_key_id`, and empty AAD. Envelopes are short-lived wraps of a 32-byte key, so the 96-bit nonce AEAD inside HPKE is correct here.

**Key chain.** A rotation entry carries `wrapped_previous = XChaCha20-Poly1305(K_{e+1}, K_e)` with AAD `ganbaru-sync/key-chain/v1`, a zero byte, then `space_id` and `e`. A member that holds the current key walks back to any older epoch, so enrollment and rotation seal only the current epoch to each recipient. A removed device holds old keys but cannot walk forward.

**Entries.** Milestone 12 adds four entry kinds beside the milestone 11 kinds (`enroll`, `remove`, `set_role`, `resolve_late`, `handover`). They travel as kind `4` authorization operations in the anchor's chain with no format change; a receiver that predates them stores and forwards them, keeps them in the inner chain, and derives nothing from them until it upgrades, as milestone 11 specifies. Milestone 10 never compacts kind `4` bodies, so every entry stays readable on any copy of the log.

| Entry | Signed by | Carries |
| --- | --- | --- |
| `DeviceKey` | The device's milestone 11 device key | Device id, X25519 public key; the sequenced entry also carries the anchor's envelope of the current epoch for that key |
| `KeyEpoch` | The anchor, sealed as the revision right after the `remove` or kit regeneration `set_role` it follows | New epoch, `wrapped_previous`, one envelope per remaining device key and one for the recovery key, the relay access public key of the new epoch |
| `RecoveryKey` | Admin key | Recovery X25519 public key |
| `ServerConfig` | Admin key | Mode (relay or anchor), up to four `host:port` endpoints, certificate fingerprint; an empty entry removes the server |

Rotation is sealed with the removal it follows. A milestone 11 `remove` cannot carry the new epoch, so when the anchor sequences a `remove` in a space with an epoch (1 or later), it generates `K_{e+1}` and seals the `remove` and its `KeyEpoch` as consecutive revisions in one seal transaction. Both travel in the anchor's contiguous writer chain, so any delivery that brings the removal brings the `KeyEpoch` too, and a member applies such a removal only once the following `KeyEpoch` is present. A space at epoch 0 applies `remove` alone. Kit regeneration rotates the same way after its `set_role`.

**Storage.** The engine table `sync_space_keys (space_id, epoch, wrapped_previous, own_envelope, sequenced_revision)` is derived from applied authorization entries and rebuilt from the log. The only secret on a device is its device private key; plaintext epoch keys live in memory as `Zeroizing` buffers, opened from `own_envelope` at service start and walked back on demand.

**`key_epoch` in the operation header stays zero.** An operation is never bound to one epoch: any member re-encrypts a stored operation under its current epoch when it pushes to the relay, so the record's epoch lives in the record header and its AAD. The header field stays reserved for per-resource keys in the sharing goal. This changes the meaning of `docs/data/sync.md:184`.

### Recovery kit

- The kit is the [milestone 11](milestone_11.md) recovery kit, not a second one: its 32-byte seed `R`, grouped Crockford base32 code with checksum, and recovery admin key stay as milestone 11 defines them. This milestone adds a QR code on a printable page beside the code, and creation asks the user to type the last group back before it completes.
- `R` also derives the recovery X25519 key through HKDF-SHA256 with info `ganbaru-sync/recovery-hpke/v1` and a zero byte, distinct from the milestone 11 recovery admin derivation (`ganbaru-sync/recovery-admin/v1`). The space stores only its public key (`RecoveryKey`), so the kit never goes stale when epochs, servers, or devices change.
- Milestone 11 deletes the seed once the user confirms saving the kit, so a space whose kit predates this milestone has no `RecoveryKey`. The first backup or server setup in such a space asks for the kit once on an admin device, or offers to regenerate it, and publishes the `RecoveryKey` entry. A linked non-admin device that needs the kit says to use it on an admin device.
- An unlinked vault has no charter yet; its first backup creates the kit and stores the kit's two public keys (recovery admin and recovery X25519) in the vault database. Milestone 11 charter creation at first link takes the recovery admin public key from there, and the anchor then sequences the `RecoveryKey` entry, which starts epoch 1. Backups made before that are at epoch 0 and open only with the kit; the backup dialog says so.
- Regenerating the kit (the milestone 11 `set_role` with the new recovery admin public key) also replaces the `RecoveryKey` entry and rotates the epoch. Backups made earlier still open with the old kit, and the dialog says so.
- The kit opens any backup of the space. Admin keys are never copied: the kit's recovery admin key is in the admin set of every copy of the log, so using it enrolls a new admin desktop as milestone 11 specifies. The relay is not a recovery path: it is a bounded mailbox, not an archive.

### Relay record and asset formats

Both formats live in `ganbaru-sync-contracts` next to the operation format, with the same codec, bounds style, and golden vectors.

Relay record, magic `GBSR`, version 1:

| Field | Bytes |
| --- | --- |
| Magic, version | 4, 1 |
| Space id, writer id | 16, 16 |
| Sequence | 8 |
| Epoch | 4 |
| Nonce | 24 |
| Ciphertext and tag | Up to the plaintext cap plus the 16-byte tag; the plaintext cap is `MAX_OPERATION_BYTES` plus milestone 11's `MAX_CERTIFICATE_BYTES` plus two u32 length prefixes |

The AEAD is XChaCha20-Poly1305 under the `relay-record` subkey of the stated epoch, with a random nonce from `ring::rand::SystemRandom` and AAD `ganbaru-sync/relay-record/v1`, a zero byte, then every header byte before the nonce. The plaintext is the stored operation envelope followed by the writer's signed certificate, each prefixed with its u32 length, so a receiver needs no second record kind for certificates. A receiver decrypts, checks that the inner operation's space, writer, and sequence equal the clear header, and feeds the operation and certificate into the same store path as a hub pull, with the same refusal codes.

Relay asset chunk, magic `GBSA`, version 1: space id, asset id (32), epoch, chunk index (u32), chunk count (u32), nonce (24), ciphertext. The asset id is `HMAC-SHA256(asset-id key, plaintext SHA-256)`, so the relay cannot match a known file and two spaces holding the same image get unrelated ids. AAD covers every header byte before the nonce, so chunks cannot be reordered, truncated, or moved between assets. Plaintext chunks are `SYNC_ASSET_CHUNK_BYTES`, and an asset is at most the milestone 2 `MAX_SYNC_ASSET_BYTES` (64 MiB); the class and content type come from the referencing operation. Clients stream chunks to a temporary file as milestone 2 transfers do, and the receiver checks the reassembled SHA-256 against the reference before the rename.

### Relay protocol and access

The relay speaks the existing framed protocol from `ganbaru-handoff` over TLS 1.3 on one TCP port, with its own `RELAY_PROTOCOL_VERSION` (numbered by the README [wire versions rule](README.md#parallel-work-reservations)) and message kinds. There is no HTTP stack: clients already have the frame codec, pinning, and bounds, and pinned TLS needs no domain name. The codec (`protocol.rs:953-1010`) is a u32 length prefix over JSON typed to `ControlMessage`, so step 6 adds a generic `read_frame<T>`/`write_frame<T>` with the same bounds and the relay defines its own message enum; the client pin verifier `PinnedServerVerifier` (`app/src/vault/handoff/transport.rs:55`) moves to `ganbaru-handoff` with the server-side `PinnedClientVerifier` in step 1.

- **Server identity.** The relay generates a long-lived self-signed certificate with `rcgen` on first start and keeps it in its data directory. Clients pin its SHA-256 fingerprint from the setup code. TLS termination by a reverse proxy is unsupported; TCP passthrough works.
- **Claim.** `ganbaru-sync-relay setup` prints a QR code and a text code with the endpoints, the fingerprint, and a one-time setup token valid for 15 minutes. An admin device scans it and sends `RelayClaim` with the token, space id, the space's admin set (the log's admin keys plus the recovery admin key), and the epoch's relay access public key, signed by its own admin key, which must be in that set. The relay keeps that admin set per space. One relay may host several spaces; each claim needs a new token.
- **Access credential.** Each epoch's relay access key pair is Ed25519 from the epoch's `relay-access` subkey as seed. A client authenticates by signing `ganbaru-sync/relay-auth/v1`, a zero byte, then the TLS exporter (32 bytes, label `ganbaru relay auth`), `space_id`, and `epoch`, which binds the proof to the TLS session. The relay learns no device identity: every member of the space looks the same. TLS is server-authenticated only.
- **Epoch registration.** Registration is proved by possession of the new key, not by the admin key, so any up-to-date member can do it and the anchor needs no admin key for it. A member authenticated for epoch `e` sends `RelayEpoch` with the access public key of `e+1` and the sequenced `KeyEpoch` entry, signed by the `e+1` access private key over `ganbaru-sync/relay-epoch/v1`, a zero byte, then the TLS exporter, `space_id`, and `e+1`. Only members that applied the rotation hold `K_{e+1}`, so a removed device cannot register. The relay accepts only the current epoch plus one; a member two epochs ahead registers twice. The relay cannot validate the opaque entry and only stores it for `RelayEpochs`; members verify it against the log.
- **Admin messages.** `RelayClaim`, `RelayAdminKey`, `RelayWipe`, and `RelayRemoveSpace` are signed by a key in the space's relay admin set and carry a per-space admin counter that the relay requires to increase, so they cannot be replayed. `RelayAdminKey` adds or removes one key. `RelayState` reports the set and the counter; after each `enroll`, `remove`, or `set_role`, the next relay pass of any admin device sends the `RelayAdminKey` messages that make the relay's set equal to the log's admin keys plus the recovery admin key. Admins share the counter, so a stale counter is refused and the sender rereads `RelayState` and retries. Because the recovery admin key stays in the set, a desktop recovered with the kit can manage the relay.
- **Read and write rule.** A credential for epoch `e` may push only while `e` is the current registered epoch. It may pull only records with epoch at most `e`, and fetch the `KeyEpoch` entries after `e`. A lagging member therefore catches up through the entries it can open, then re-authenticates; a removed device reads nothing sealed after its removal and cannot push.
- **Messages.** `RelayHello` (version, space), `RelayAuth`, `RelayState` (per-writer contiguous range, current epoch, stored bytes, quota, admin set, admin counter), `RelayPush` (records up to one page), `RelayPull` (from a vector, with `more`), `RelayWait` (long poll with the hub's 12 s timeout), `RelayAssetQuery`, `RelayAssetPut`, `RelayAssetGet`, `RelayEpochs`, `RelayEpoch`, and the admin-signed `RelayClaim`, `RelayAdminKey` (add or remove one admin public key), `RelayWipe`, and `RelayRemoveSpace`. Authentication refuses with `stale_epoch` when the client is behind the relay and `unknown_epoch` when it is ahead.
- **Push validation.** Bounds, the header parse, the space and epoch matching the authenticated credential, and per-writer contiguity (`gap` otherwise). An occupied slot with identical bytes is a duplicate; with different bytes it answers `slot_taken`, which honest members treat as delivered, because another member pushed the same operation under a different nonce. The relay verifies nothing inside the ciphertext.
- **Retention.** The relay is a bounded mailbox. Each space has a byte quota (configuration, default 2 GiB) and a maximum age (configuration, default 90 days, matching the milestone 10 dormancy threshold). The oldest records go first, trimming each writer from the front and raising its floor so ranges stay contiguous. A pull below a floor answers `behind_floor`; the member then catches up from a peer or the anchor, or through milestone 10 bootstrap, and status says it needs a direct connection. Assets expire by age and quota in order of last read.
- **Storage.** sqlx SQLite with the crate's own embedded migrations: `relay_spaces`, `relay_epochs`, `relay_records`, `relay_floors`, and `relay_assets`. Relay-mode storage does not use `ganbaru-db`; the crate still links it through `ganbaru-sync-replica` for anchor mode.
- **Bounds.** Connection budgets follow the coordinator listener: a global and per-source pending budget before authentication, 16 KiB frames before authentication, and the existing handshake and idle timeouts.

### Client relay pass

The pass lives in `ganbaru-sync-replica` as `relay.rs`, beside `client.rs`, and runs on every member whose space has a relay `ServerConfig`.

1. Dial the configured endpoints in order and pin the certificate.
2. Authenticate with the current epoch. On `stale_epoch`, fetch and apply the later `KeyEpoch` entries, then retry once. On `unknown_epoch`, authenticate with the relay's epoch (walked back through the chain), register each newer epoch in order, and re-authenticate.
3. Read `RelayState`, and push for each writer the stored operations above the relay's range, in order, encrypted under the current epoch. Operations whose bodies were compacted are skipped; a member that needs them catches up from a direct peer or the anchor, or through milestone 10 bootstrap, as in Retention. Kind `4` bodies are never compacted, so authorization entries always reach the relay. Before pushing an operation that references assets, query and upload the missing ones.
4. Pull records above the local vector, decrypt, and store them through the normal path. Missing assets join the milestone 2 transfer queue with the relay as a source.
5. While the app is in the foreground, hold a `RelayWait`; otherwise return.

Members push to the relay even when a direct peer is reachable, so remote devices receive changes without waiting for that peer.

**Dial exception.** `connect_coordinator` and the milestone 11 peer dialer keep refusing public addresses. Endpoints named in a sequenced `ServerConfig` entry are the one exception: hostnames resolve through the system resolver, public addresses are allowed, loopback stays refused in release builds, and the pinned fingerprint decides identity. The entry is admin-signed, so an untrusted QR code or contact card can never widen the dial policy.

**Android.** A relay worker in `ganbaru-mobile-sync` runs with the `CONNECTED` network constraint, unlike the milestone 3 LAN worker that returns on cellular. On a metered network it moves records and assets up to 1 MiB; larger assets wait for an unmetered network. It shares the milestone 3 pass gate and triggers (periodic, network change, expedited after backgrounding with pending changes).

### Tauri-free member stack

The anchor must run the same listener and sync service as the app without linking Tauri, and the app must not have two copies. Step 1 moves them into `ganbaru-sync-replica`:

- `serve`: the TLS acceptor, connection limits, the pinned client verifier, frame IO, and the accept loop, with a `Routes` trait. The app supplies its contact and enrollment routes through it; the anchor supplies enrollment and sync only. Contact request receipt stays with the anchor role through this milestone, and anchor mode does not serve it, so after a handover to the server contact requests wait until the sharing goal moves receipt to any device; the handover option says so.
- `service`: the sync service loop generic over the milestone 3 `SyncHost`, spawning on plain `tokio`. The app crate keeps the `AppHandle` implementation of `SyncHost` and nothing else of the loop.

The move changes no behavior and keeps the existing app tests passing unchanged.

### Always-on anchor

`ganbaru-sync-relay anchor` runs a headless enrolled member from a data directory holding a vault folder (`vault.json`, `ganbaru.sqlite`, `assets/`), the server's TLS identity, writer key, device encryption key, and its configuration.

- **Enrollment runs in reverse.** A server cannot scan a code, so `ganbaru-sync-relay anchor setup` prints a QR code and text code with endpoints, certificate fingerprint, and a one-time secret. An admin device scans it, dials out, proves the secret, receives the server's milestone 11 device public key, TLS fingerprint, and admin public key, and proposes the milestone 11 `enroll` entry with role admin. The server cannot dial members, so the bootstrap transfer runs in reverse: the admin device prepares a snapshot exactly as milestone 10 bootstrap does and uploads it over its setup connection, and the server activates it like a milestone 10 activation. Milestone 10 only downloads snapshots; this upload direction is new in this milestone and step 8 owns it. The server then mints its own writer certificate v2 (`ganbaru-sync/writer-certificate/v2`) with its device key (it never receives the person key), publishes its `DeviceKey`, and receives the current epoch.
- **Anchor role.** The flow ends with a milestone 11 handover to the server, on by default with an option to keep the current anchor. Milestone 11 requires the anchor to be an admin and never copies admin keys between devices, so the server creates its own admin key at setup and is enrolled as an admin; this extends the milestone 11 desktop-only admin rule to the server device kind.
- **Endpoints.** The admin device writes a `ServerConfig` entry with mode anchor, so every member learns the public endpoints and dials them under the exception above.
- **Behavior.** The server applies every operation through the engine with the full vault manifest and stores every managed asset, so it is a full replica. It serves sync requests and never dials peers, because members behind NAT are not reachable. It authors only acknowledgements, authorization entries it sequences, and rotations.
- **Derived state.** Post-apply hooks are host-free and registered by the host (milestone 2); the anchor registers none, because their indexes and derived columns serve the app. Snapshots it serves carry a `derived_state_stale` flag, and a receiving app runs every registered hook in full rebuild mode before the vault opens. The milestone 2 `MaterializeHook` has no full rebuild entry point, so step 8 adds `rebuild_all` to the trait as an engine-track change, and every registered hook implements it.
- **Backup command.** `ganbaru-sync-relay anchor backup --out <file>` writes the encrypted backup format below, for users who want file backups by cron.
- **Recovery after losing every device.** `anchor setup` prints a fresh secret on demand. A new desktop proves it, derives the milestone 11 recovery admin key from the kit in memory, signs its own `enroll` as admin (sequenced by the server as anchor), and bootstraps from the server.
- **Versions.** The server speaks the app's sync protocol and ships with the same version as the app. An app that meets an older server shows that the server needs an update.

### Encrypted backups

**Format** `GBBK`, version 1:

- Header: magic and version, space id, epoch, the file key wrapped under the epoch's `backup-wrap` subkey, and an HPKE envelope of the file key to the recovery key with info `ganbaru-sync/backup/v1`, a zero byte, then `space_id`.
- Body: the existing zip layout, cut into 1 MiB plaintext chunks. Each chunk is a u32 length, a random 24-byte nonce, and XChaCha20-Poly1305 ciphertext under the file key with AAD made of the header's SHA-256, the chunk index (u64), and the last flag (u8). Reordering, truncation, and header swaps fail authentication. Encryption streams, so the 100 GiB cap and constant memory still hold.
- A backup always has the recovery envelope, so creating one requires a recovery identity. The epoch wrap lets any current member open it without the kit; a backup at epoch 0 has no epoch wrap.
- Extension stays `.ganbaru-backup`; the MIME type becomes `application/octet-stream`. Plaintext zip backups are not read: there are no external users, so no fallback reader is kept.

**Creation** reuses milestone 10's archive creation in `ganbaru-sync-replica` (`VACUUM INTO`, then `prepare_snapshot_copy`), so a backup has the same shape as a bootstrap snapshot and never carries installation state or Local rows, and Local columns hold their materialization values. Creation holds milestone 10's asset sweep lock (the async mutex in `ganbaru-sync-replica` that the sweep shares with archive creation) until the archive finishes, so no referenced bytes are swept mid-archive; there are no lasting backup pins. Desktop adds `vault_backup_to_file` with a save dialog under the existing `reserve_vault_transition` guard and snapshot quiescence (`vault/quiescence.rs:13-18`); Android keeps `vault_backup_to_downloads` with the new format. The format code lives in `ganbaru-sync-replica::backup` so the anchor command reuses it.

**Restore** decrypts into staging with the device's epoch keys or the kit, then runs the existing archive validator. Two outcomes exist:

| Outcome | When | Result |
| --- | --- | --- |
| Independent copy (default) | Always offered on desktop. On Android, whenever the active vault is unlinked. | New vault id and space, scrubbed sync state with a fresh genesis, no recovery identity or servers. Desktop opens it from a user-chosen folder as another vault; Android replaces the active vault and keeps the replaced database in `.database-rollbacks/`, restorable from Settings > Data; step 4 adds the cross-vault retention this needs, because `retain_replaced_vault` (`vault/database_rollback.rs:305-318`) keeps nothing when the vault ids differ. |
| Restore in place | Only when the backup's space equals the active space and neither the active log nor the backup's log has another enrolled device. | Replaces the database under the existing rollback and successor writer rules. Local tables start empty and Local columns hold their materialization values, because backups always run `prepare_snapshot_copy`. |

A linked Android device refuses restore and says to restore on a desktop or unlink first, because Android holds one vault and replacing it would drop the device's membership. A lost linked device is recovered by enrolling again and bootstrapping from a member or the anchor, not from a backup. These rules mean a restore never rewinds acknowledgements, resurrects tombstones in a live space, or restores credentials.

### Status and Settings

- Sync status gains two counts: local changes not yet received by the relay, and changes received by the relay but not yet confirmed by the anchor (the milestone 11 anchor mark, which milestone 10's signed acknowledgements let peers relay). The UI shows them as saved, received by relay, and confirmed, beside milestone 11's received by a peer.
- Settings, Sync, Server: add a server by scanning or pasting its setup code (the code says relay or anchor); show reachability, last contact, version, and for a relay the stored bytes against the quota; remove the server, which writes an empty `ServerConfig` and sends `RelayRemoveSpace`. A link opens the setup guide.
- Settings, Sync, Recovery kit: create, view status, regenerate.
- Settings, Data on desktop: back up to a file and restore from a file. Android keeps its existing rows and gains the kit prompt.
- Strings go in `sync.ts` under `sync.server`, `sync.recoveryKit`, and `sync.delivery`, and in the `data` section of `i18n/messages/{en,es}/settings/general.ts` for backup rows, in English and Spanish. Frontend readers validate every new status field.

### Packaging and setup

- The release build job produces static musl binaries of `ganbaru-sync-relay` for x86_64 and aarch64 Linux, attached to the release and covered by the signed `SHA256SUMS`.
- `crates/ganbaru-sync-relay/Containerfile` builds a `scratch` image with the static binary, a non-root user, one volume for the data directory, and one port. A separate publication job pushes it to GitHub Container Registry with its own token, following the job separation in [supply-chain security](../../docs/data/security/supply-chain.md); the setup guide references images by digest.
- Configuration is one JSON file in the data directory (listen address and port, quotas, retention) with defaults in typed constants; command-line flags override it. The binary logs to stderr and sends nothing anywhere except to connecting members.
- The setup guide covers a VPS with Docker, a NAS or home server with a port forward, a systemd unit for the bare binary, upgrades, and backing up the data directory. It says when Tailscale alone is enough, which needs no server at all.

### Dependencies

| Crate | Use | Review |
| --- | --- | --- |
| `chacha20poly1305` 0.11.0, pinned exactly, default features off | XChaCha20-Poly1305 for records, assets, chain, and backups | RustCrypto organization, about 99 million downloads, released 2026-06-28 after a year of release candidates, pure Rust with internal SIMD. Reuses the locked `chacha20` 0.10.2, `zeroize`, `subtle`, and `hybrid-array`; adds `aead` 0.6, `poly1305` 0.9, and `cipher` 0.5. `ring` cannot replace it: it has no XChaCha. |
| `hpke` 0.14.1, pinned exactly, `default-features = false`, features `alloc`, `getrandom`, `x25519`, `chacha` | Key envelopes and backup recovery envelopes | RFC 9180 implementation by Michael Rosenberg, about 12.7 million downloads, released 2026-09-06. Every primitive is optional, so this feature set leaves out `ml-kem`, the NIST curves, and AES. Reuses the locked `hkdf` 0.13, `sha2` 0.11, `getrandom` 0.4, and `rand_core` 0.10. Single publisher: contained by the exact pin, RFC 9180 known-answer tests for the chosen suite in our tests, cargo-audit, and a reviewed diff on every bump. |
| `x25519-dalek` 3.0.0 with `curve25519-dalek` (through `hpke`) | X25519 with static secrets | dalek-cryptography organization, about 86 million downloads. A second Curve25519 implementation beside `ring` is an accepted cost: `ring` only offers ephemeral X25519 private keys, so it cannot decapsulate an envelope with a stored device key. |

The relay and anchor add no other crates: `tokio`, `rustls`, `tokio-rustls`, `rcgen`, `sqlx`, `serde_json`, and `qrcode` are already locked, and arguments are parsed by hand for the few subcommands. HKDF, HMAC, randomness, and Ed25519 stay on `ring`. The fallback if the review rejects `hpke` is `hpke-rs` with its RustCrypto provider; writing HPKE by hand is not an option. The review is recorded in this plan's progress and summarized in a new decision record (see [Documentation](#documentation)).

### Rejected alternatives

- **Shared-folder transport.** Android document providers are not synced local folders: access is per document, background access is limited, and a cloud drive's provider fetches on demand. Consumer sync clients create conflict copies and expose partial uploads. A folder cannot refuse a removed device. The relay covers the same need with one small binary.
- **Encrypting every operation, including direct links and the local log.** Mutual TLS already protects direct links, the vault is plaintext at rest by policy, and per-seal encryption would cost every write for no new protection.
- **An HTTP API with WebPKI and ACME.** It needs a domain name and an ACME client, and adds an HTTP server surface; pinned TLS works with a bare IP and reuses the existing codec.
- **A relay that verifies writer signatures.** It would need plaintext envelopes (writer certificates, clocks, change indexes, value hashes) and would forbid forwarding by any member other than the author. The blind relay row in the [topology decision record](../../docs/architecture/decisions/sync-topology-and-authority.md) already rules it out.
- **Per-device relay grants.** They expose the device inventory and need one admin message per enrollment, while the epoch credential gets revocation from rotation for free.
- **Passphrase-protected backups.** They need a password KDF dependency and a second secret users forget; the kit covers recovery.
- **Group key agreement (MLS or TreeKEM).** Spaces have a handful of devices, so one envelope per member per rotation is cheap and far simpler.

## Testing

- **Formats** (`ganbaru-sync-contracts`): golden vectors for `GBSR`, `GBSA`, `GBBK`, envelopes, chain links, and kit encoding; every header field tampered, nonce reuse rejected by tests that inject a fixed RNG, truncated and reordered chunks, wrong space or epoch in AAD, inner operation mismatching the clear slot, oversize ciphertext, unknown version held.
- **HPKE**: RFC 9180 known-answer vectors for DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305 base mode, plus our info strings.
- **Keys**: chain walk across five epochs; a removed device's keys open nothing after its removal; a `remove` and its `KeyEpoch` are sealed together or not at all under a crash at seal; a member holding a `remove` without the following `KeyEpoch` does not apply it; a space at epoch 0 applies `remove` alone; `DeviceKey` signed by another device's key refused; kit checksum catches one-character errors and transpositions.
- **Relay store** (`ganbaru-sync-relay`): claim with expired or reused token, admin counter replay and the stale counter retry, admin set add and remove, an admin message signed by a key outside the set refused, epoch registration only by a holder of the next key and only for the current epoch plus one, `unknown_epoch` fallback, push validation and contiguity, `slot_taken`, quota and age eviction keeping contiguous ranges, `behind_floor`, the read rule for older credentials, and the write rule after rotation.
- **Relay end to end**: an in-process relay on a test-only loopback listener; two replicas converge through it; a third replica removed mid-run cannot push or read new records; channel binding fails when the exporter differs; assets round-trip and survive eviction by re-upload.
- **Simulation harness**: the relay as one more delivery path with drops, duplicates, reordering, delays, eviction, and rotation; all seeds converge with and without direct links.
- **Backups**: round trip at 0, 1, and many chunks; open with epoch key and with kit; wrong kit; truncated file; an unlinked epoch 0 backup opens only with the kit; restore outcome matrix for linked and unlinked, same and other space, desktop and Android; asset sweep lock held during creation and released after failure.
- **Member stack and anchor**: after step 1, the existing app sync and handoff tests pass unchanged; a headless anchor enrolls through the reversed flow, bootstraps, serves a pull, and serves a snapshot whose stale flag triggers the rebuild path; kit-based admin recovery enrolls a new device.
- **Frontend**: `sync.test.ts` and `lib/sync/status.test.ts` for the delivery counts, server status, and kit status readers.
- **Kotlin**: relay worker constraint and metered asset decision in JVM unit tests.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement). Steps 2 and 10 change dependencies or release workflows and also run `pnpm -w run validate:full`.

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `refactor/sync-member-stack` | Listener (`serve`) and sync service moved into `ganbaru-sync-replica` behind `Routes` and `SyncHost`, and both TLS pin verifiers move to `ganbaru-handoff`; the app becomes one host. No behavior change. Needs milestone 11. | `cargo test -p ganbaru-sync-replica --lib -j 4 -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 handoff -- --test-threads=4`, `cargo check -p ganbaru --bin ganbaru -j 4` |
| 2 | `feat/sync-space-keys` | Dependency review and crates; device encryption keys in every key store; `crypto` module with derivations, envelopes, chain; the four entry kinds beside the milestone 11 kinds; `KeyEpoch` sealed with removals and the removal wait; `sync_space_keys`. | `cargo test -p ganbaru-sync-contracts --lib -j 4 crypto -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 authorization -- --test-threads=4`, `cargo test -p ganbaru-sync-replica --lib -j 4 writer -- --test-threads=4` |
| 3 | `feat/sync-recovery-kit` | Milestone 11 kit extended with the recovery X25519 derivation, QR printable page, confirmation, kit entry for spaces whose kit predates this milestone, kit creation on unlinked vaults with its public keys stored for charter creation at first link, regeneration with rotation, non-admin message, `sync.ts` strings. | `cargo test -p ganbaru-tauri-app --lib -j 4 recovery_kit -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/api/sync.test.ts --maxWorkers=1` |
| 4 | `feat/vault-encrypted-backups` | `GBBK` streaming format, desktop `vault_backup_to_file` and restore, Android format change, restore outcomes, cross-vault rollback retention for the Android independent copy, asset sweep lock during creation, Data settings rows. Needs 3. | `cargo test -p ganbaru-sync-replica --lib -j 4 backup -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 backup -- --test-threads=4` |
| 5 | `feat/sync-relay-records` | `GBSR` and `GBSA` formats, encrypt and decrypt paths, simulation delivery path. Needs 2. | `cargo test -p ganbaru-sync-contracts --lib -j 4 relay -- --test-threads=4`, `cargo test -p ganbaru-sync --lib -j 4 sim -- --test-threads=4` |
| 6 | `feat/sync-relay-server` | `crates/ganbaru-sync-relay` with relay mode: storage and migrations, protocol, claim and epoch registration, the per-space admin set, retention, setup code, configuration. Needs 5. | `cargo test -p ganbaru-sync-relay --lib -j 4 -- --test-threads=4` |
| 7 | `feat/sync-relay-client` | Relay pass, relay admin set upkeep by admin devices, `ServerConfig` dial exception, Android relay worker, delivery counts in status. Needs 6. | `cargo test -p ganbaru-sync-replica --lib -j 4 relay -- --test-threads=4`, `cargo test -p ganbaru-handoff --lib -j 4 dial -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/sync/status.test.ts --maxWorkers=1` |
| 8 | `feat/sync-always-on-anchor` | Anchor mode, reversed enrollment with the server's own admin key, snapshot upload and server activation, handover, derived-state rebuild flag and `MaterializeHook::rebuild_all`, `anchor backup`, kit-based admin recovery. Needs 1, 4, 6, and milestone 10. | `cargo test -p ganbaru-sync-relay --lib -j 4 anchor -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4` |
| 9 | `feat/sync-server-settings` | Server, recovery kit, and delivery UI; unreviewed toggle for relay setup; catalogs. Needs 7. | `pnpm --dir apps/client exec vitest run src/lib/api/sync.test.ts --maxWorkers=1`, `pnpm --dir apps/client run check` |
| 10 | `build/sync-relay-packaging` | Musl targets, `Containerfile`, release build and separate image publication jobs, `SHA256SUMS` coverage, `check:workflows` allowlist. Needs maintainer approval for workflow changes. | `pnpm -w run check:workflows`, `cargo check -p ganbaru-sync-relay -j 4` |
| 11 | `docs/sync-milestone-12` | Remaining docs, setup guide, measurements, `AGENTS.md` map line, goal README. | `pnpm -w run validate` |
| 12 | None | Physical acceptance below, recorded here. | Manual |

Every step except 8 needs only milestone 11 (which already follows milestone 10 steps 1 to 4), so this milestone can start while milestone 10 steps 5 to 7 are in progress; step 8 waits for all of milestone 10. The external review starts once steps 2, 4, 5, and 6 are merged and gates removing the unreviewed toggle.

## Documentation

- `docs/data/sync.md`: the opening (:15) drops "no relay, internet traversal" and names the optional relay and anchor; the relay paragraphs (:144-146) become current, with the bounded mailbox, retention, and the pinned TLS transport; the always-on anchor row (:158) describes reversed enrollment and plaintext at rest; the key epoch line (:184) says the epoch lives in relay records and the operation header field stays zero; the receipt line (:186) names the three delivery states; the keys section (:209-211) records device keys, epochs, the chain, envelopes, the epoch-derived relay credential, rotation sealed with removals, and the kit; the assets line (:223) replaces encrypted manifests with keyed asset ids and chunk AAD; the backup paragraph (:225) states the format, the restore outcomes, and the asset sweep lock held during creation in place of backup pins; the Settings line (:231) lists server, kit, and backup rows; the delivery row (:245) becomes Implemented with physical acceptance noted; the security review sentence (:249) records the review status.
- `docs/data/security/network-and-privacy.md`: current surfaces gain relay and anchor traffic with exactly what the relay host learns; the dial bullet gains the `ServerConfig` exception; the planned list drops the relay; "Future synchronization encryption" becomes current with the composition and review status; "Local data encryption" says backups are encrypted and the always-on anchor stores plaintext.
- `docs/architecture/decisions/sync-encryption.md` (new, Reference): the composition, why encryption stops at intermediaries, the relay credential, rejected alternatives, and the dependency review summary. Linked from `docs/architecture/decisions/README.md`.
- `docs/architecture/decisions/sync-engine.md`: the constraint (:10) links the new record; the limitation (:42) says operations stay plaintext at rest and on direct links by decision.
- `docs/architecture/decisions/sync-topology-and-authority.md`: the relay row says the relay authenticates the epoch credential and verifies nothing inside records; the shared-folder transport joins the rejected alternatives.
- `docs/data/schema/sync.md`: `sync_space_keys` and the four authorization entry kinds.
- `docs/data/architecture.md` (:37) and `docs/platforms/android/native-services-and-data.md` (:13-19): encrypted backups, the restore outcomes, and the linked Android refusal.
- `docs/platforms/server/README.md` (new): the setup guide, linked from `docs/platforms/README.md` and from Settings.
- `docs/data/security/unsafe-rust.md`: only if the Android relay worker adds JNI surface beyond `runPass`.
- `docs/data/security/dependency-audits.md`: only if the new crates bring advisories or warnings.
- `docs/operations/release/README.md` and `distribution.md`: the relay binaries and the image publication job.
- `docs/features/README.md` (:26) and `docs/ROADMAP.md`: relay and encrypted backup status.
- `docs/performance/results.md`: the measurements below.
- `AGENTS.md`: the reserved map line `ganbaru-sync-relay/: self-hosted relay and always-on anchor server binary`, the backup sentence in the folder structure (:154), and the "Planned work" sentence that still lists the relay.
- This folder: progress below, the goal README status, and the milestone 12 row.

No README "Docs differ until" row names milestone 12.

## Manual acceptance

Devices: A, Linux desktop, coordinator and admin; B, Windows desktop, linked; P, Android 14 or newer, linked; V, a rented x86_64 VPS with a public IPv4 address running the relay container; H, an arm64 home server (Raspberry Pi 4 or a NAS) running the anchor binary behind a router port forward.

| Scenario | Expected |
| --- | --- |
| Start the relay container on V; A scans its setup code | Claimed within one pass; Settings shows reachable and quota. |
| P on cellular, A at home, B at another site; edit on each | All three converge through V within one pass after each edit. |
| Inspect V's database and traffic capture | Only ciphertext, ids, sizes, and timing; no device names, text, or file names. |
| P offline for a week, then online on cellular | Catches up from V; large assets wait for Wi-Fi. |
| Remove B on A while B is offline; B comes online on another network | B cannot authenticate for push; it reads nothing sealed after removal; A and P keep syncing. |
| V's quota set low; P offline past it | P reports it needs a direct connection; on the home LAN it catches up from A. |
| Relay unreachable for a day | Editing continues; LAN sync continues; status shows the relay as unreachable and pending counts. |
| Install the anchor on H; enroll from A; accept handover | H bootstraps, becomes anchor; B and P sync with H from outside the home network. |
| A switched off; P and B edit remotely | Both sync through H; removals proposed on B wait only if B is not an admin device. |
| Create a backup on A and on P | Files open only with the kit or a member's keys; a hex dump shows no zip structure. |
| Restore A's backup on A (linked) | Independent copy opens as another vault; the linked vault is untouched. |
| Fresh Android install, restore P's backup with the kit | Independent copy replaces the starter vault; no link to the old space. |
| Lose A, B, and P; new device with the kit and H's setup code | Kit's recovery admin key enrolls the new device as admin; it bootstraps from H. |
| Regenerate the kit on A | Epoch rotates; old backups still open with the old kit; new backups need the new one. |
| Upgrade the app on A only | A reports that H needs an update until H is upgraded. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Relay pass with nothing to exchange | Under 1 s including the TLS handshake, on broadband |
| 1,000 queued operations from P to B through V | Under 15 s end to end on a 10 Mbit/s uplink |
| Record encryption and decryption | Under 5% of client pass CPU time |
| Backup creation on A | Within 15% of the plaintext archive time for the same vault |
| Relay memory | Under 64 MiB resident with 10 connected devices |
| Relay storage overhead | Under 64 bytes per record beyond ciphertext |
| Anchor on H | Applies a 100,000-operation catch-up in under 5 minutes; under 150 MiB resident when idle |
| Android relay worker battery | Not listed among the top consumers over a 24 hour idle day, as in milestone 3 |

## Risks

| Risk | Mitigation |
| --- | --- |
| A composition flaw in new cryptography | Only standard constructions from reviewed crates, known-answer tests, domain-separated subkeys, the external review as an exit gate, and the unreviewed toggle until it passes. |
| A member not yet removed squats relay slots with garbage | Other members still deliver over direct links and the anchor; removal ends its access once any remaining member registers the new epoch, which its next relay pass does; the admin can wipe the relay space and members push again. Accepted limitation, documented. |
| Single publisher of `hpke` | Exact pin, known-answer tests, audited bumps, and `hpke-rs` as a ready fallback. |
| Losing the recovery kit and every device | The always-on anchor and any member's keys still open backups; the kit dialog states the consequence before creation completes. |
| The member stack move breaks the app's sync | Step 1 is a pure move with the existing tests unchanged, landed before anything depends on it. |
| Users expose the anchor (plaintext) on an untrusted VPS | The setup guide and the add-server flow recommend relay mode for rented servers. |
| Container image publication adds a release target | A separate job and token, digest references, `check:workflows` coverage, and maintainer approval of the workflow change. |
| Version skew between the app and a server | Same release version, a clear "needs update" status, and newer formats held rather than misapplied. |
| Metadata visible to the relay host | Stated plainly in network and privacy docs; padding deferred. |

## Deferred

- Relay delivery for multi-person spaces and contact requests, and the relay address in contact cards (sharing goal).
- Padding and cover traffic.
- Several relays per space.
- Native macOS and Windows server builds.
- Scheduled backups from the app; the anchor command covers scheduling on servers.
- A post-quantum HPKE suite; the envelope version allows adding one.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- **Earlier step:** Milestone 11 keeps member listeners in the app crate and milestone 3 puts `SyncHost` in `app/src/sync/host.rs`, so step 1 moves the listener, the service, and the trait; confirm the trait's method types are Tauri-free before the move.
- **Earlier step:** Whether held newer-format operations are still served by a hub: if yes, a lagging server stays useful; if no, the app stops using a server older than itself and says so.
- **Earlier step:** `cargo tree` after step 2: if `chacha20poly1305` 0.11.0 does not reuse `chacha20` 0.10.2, accept a duplicate only within the RustCrypto organization; if the `hpke` feature set pulls `ml-kem`, NIST curves, or AES, or has an open advisory, switch to `hpke-rs`.
- Settled from source: the writer key reference is `writer-key:{vault_id}:{writer_id}` (`crates/ganbaru-sync-replica/src/writer.rs:46-48`); the device encryption key uses `device-encryption-key:{vault_id}`, under 80 characters and far below the Windows generic credential target limit of 32,767.
- **Runtime:** Static musl builds with bundled SQLite and `ring` for aarch64 in CI: if cross builds fail, ship glibc builds with a documented minimum glibc and keep the container on a matching base.
- **Runtime:** The Android document picker with `application/octet-stream`: if it cannot filter by extension, accept any file and rely on the magic check.

## Progress

- [ ] Step 1: Tauri-free member stack.
- [ ] Step 2: space keys and authorization entries.
- [ ] Step 3: recovery kit.
- [ ] Step 4: encrypted backups.
- [ ] Step 5: relay record and asset formats.
- [ ] Step 6: relay server.
- [ ] Step 7: relay client and Android worker.
- [ ] Step 8: always-on anchor.
- [ ] Step 9: server, kit, and delivery settings.
- [ ] Step 10: relay packaging.
- [ ] Step 11: docs and measurements.
- [ ] Step 12: physical acceptance.
