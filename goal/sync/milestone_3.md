# Milestone 3: connectivity and Android background sync

**Status: Partial.** Implemented in source on `feat/sync`; its Android paths were not compiled on the build host, and physical acceptance (step 10) is pending. Part of the [concurrent sync goal](README.md). This file is the implementation plan for connectivity: coordinator address refresh, DNS-SD discovery, private and overlay networks on a dual-stack listener, firewall updates, and Android background sync through WorkManager. It needs only [milestone 1](milestone_1.md) and runs on the network track beside [milestone 2](milestone_2.md). [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract, with [network and privacy](../../docs/data/security/network-and-privacy.md) for the network surface and the [Android platform docs](../../docs/platforms/android/README.md) for background execution; each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- A linked device reaches the coordinator after the coordinator's address changes (new DHCP lease, another Wi-Fi, Ethernet instead of Wi-Fi) with no relinking and no user action.
- Devices on the same LAN find the coordinator through DNS-SD even when every remembered address is stale. The advertisement reveals nothing stable to a device that is not a member.
- Sync works over private and overlay networks: RFC 1918, link-local, `100.64.0.0/10` (Tailscale and similar), IPv6 ULA, and on-link global IPv6, over IPv4 and IPv6. Public addresses stay refused on both ends.
- On Linux, one administrator prompt on the coordinator opens TCP for the listener and UDP 5353 for discovery, for IPv4 and IPv6, and the rule survives an address change inside the same subnet.
- Android exchanges in the background: every 15 minutes while a usable network exists, soon after a network change, and right after the app is backgrounded with pending changes. No foreground service, no persistent notification.
- Sync status shows how the device is connected (network and address), the last exchange, and why connectivity is degraded (background restriction, local network denial, firewall authorization).

## Scope

In scope:

- One shared address policy in `ganbaru-handoff` for dialing, pre-TLS listener admission, reported addresses, and discovery candidates.
- A dual-stack wildcard listener with admission keyed by the accepted socket's local address.
- The next handoff protocol version (see the [wire versions rule](README.md#parallel-work-reservations)): `SyncState` carries the coordinator's listener endpoints and the vault's discovery key.
- New `CoordinatorPin` fields for the last successful endpoint, the reported endpoints, and the discovery key (an interim field that milestone 11 removes), and one coordinator dial resolver used by every pinned connection.
- DNS-SD over mDNS (`_ganbaru._tcp`) with hourly member tags: `mdns-sd` on desktop, `NsdManager` on Android.
- Linux firewall scopes with protocol (TCP, UDP), IPv6, prefix destinations, and overlay interfaces, in the app, the root helper, and package removal.
- A Tauri-free sync host so one exchange pass runs without the Tauri app.
- New plugin crate `crates/ganbaru-mobile-sync` with WorkManager scheduling, a network callback, NSD browsing, background status probes, and the headless JNI entry.
- Status view fields, Settings sync section rows, and `sync.ts` strings for connection and degraded reasons.

Out of scope:

- LAN peer exchange and any member advertising or serving (milestone 11). The discovery module is shaped for it; only callers are added there.
- Discovery key rotation on device removal (milestone 11 rotates it through removal entries and removes the `CoordinatorPin` field).
- Record encryption, the relay, and the always-on anchor (milestone 12).
- Contact cards carrying more than one endpoint. Contact delivery keeps its single card endpoint.
- macOS and iOS.
- A foreground service, live Focus companion status, or remote Focus control.

## Prerequisites

- Milestone 1 is committed on `feat/sync` (the hub, `PairedTransport`, `SyncRuntime`, `SyncStatusView`), and the code speaks the current handoff protocol 6. Its physical acceptance (step 8) does not block this milestone.
- New dependencies, each reviewed in the step that adds it (see [dependencies](#dependencies)): `if-addrs` as a direct dependency (step 2), `mdns-sd` with its remaining new transitive crate `socket-pktinfo` (step 5), and `androidx.work:work-runtime` (step 7). `socket2`, `ring`, and `jni` 0.21 are already in `Cargo.lock` and become direct dependencies without new lock entries. Steps 2, 5, and 7 run `pnpm -w run validate:full`.
- `sync.ts` (shared with milestones 2, 10, 11, and 12): this milestone adds keys only under `sync.connection` and `sync.degraded`, and rebases rather than reorders existing keys.

## Current state

Facts this design builds on, verified in source on 2026-10-21. Re-verify before step 1.

- **Protocol.** `PROTOCOL_VERSION` is 6 (`crates/ganbaru-handoff/src/protocol.rs:16`) and `validate_protocol` requires an exact match (:1300). `SyncState { manifest_version, stored, probe_hash }` (:366) answers both `SyncHello` and `SyncWait`. `Enrolled` (:223) is sent by the enrollment path (`app/src/vault/handoff/transport/server.rs:340`) on a connection that is server-pinned only, because an enrollment that presents a client certificate is refused; it is not mutual TLS. Ports are 43821 (production) and 43822 (development) (:28-30).
- **Pin.** `CoordinatorPin { device_id, device_label, endpoint, vault_id, generation, certificate_fingerprint }` (`crates/ganbaru-handoff/src/pairing.rs:107-116`) holds one endpoint, the invitation's, and it never changes after enrollment. Optional fields already use `#[serde(default)]` (`device_label`, `generation`, `LinkedPeer.device_kind`). `PAIRING_STATE_SCHEMA_VERSION` is 2 (:36), and `pairing/storage.rs:115` rejects any other version.
- **Dial rules.** `crates/ganbaru-handoff/src/dial.rs` classifies targets as `PrivateLan`, `Overlay`, `Loopback`, or `GlobalIpv6`. `authorize_dial_target` (:91-112) allows global IPv6 only inside the /64 of the source the OS would use (`ipv6_source_for`, `app/src/vault/handoff/transport.rs:745`) and loopback only in debug builds.
- **Pinned dials.** Every coordinator connection goes through `connect_pinned` (`transport.rs:754`), which calls `TcpStream::connect` on one address with the 10 s handshake timeout (:770). Eight sites in `transport.rs` read the pin, and three of them parse its single `endpoint` and dial: `upload_bundle` (:361), `authenticated_exchange` (:556, shared by `request_bundle`, `probe_coordinator`, `exchange_distractions`, `request_person_key`, and `sync_request`), and `download_bundle_inner` (:613); about 30 `coordinator_pin()` reads exist across the app crate.
- **Listener.** `start_on` (`app/src/vault/handoff.rs:104`) checks `is_lan_address` (:819), binds one `SocketAddr`, and builds `lan_admission(&app, endpoint)` (:414 Linux, :435 other desktops) for that one address. `ensure_desktop_coordinator` (:350) picks the address with `discover_private_lan_address` (:802), a UDP route probe that is IPv4 only, and `start_desktop_when_reachable` (:329) waits until such an address exists. `is_lan_address` allows loopback in release builds and omits `100.64.0.0/10`, so it disagrees with the dial rule. The accept loop (`transport/server.rs:115`) checks `LanAdmission::admits` (:130) before TLS; `transport/server/admission.rs:73-76` admits when the decision cannot be read.
- **Linux firewall.** `crates/ganbaru-handoff/src/network_access.rs` scopes are IPv4 only (`local_address: Ipv4Addr`, :49), routes come from `/proc/net/route` only (:319, :408), UFW and firewalld rules are parsed as TCP only (:905, :932), `privileged_helper_arguments` accepts only the coordinator port (:616-628), and `validate_rule_addresses` (:965) pins the destination to one host address. Network identity is the default gateway's hardware address; a network without one counts as allowed. `handoff_grant_network_access` and `handoff_revoke_network_access` (`handoff.rs:444` onward) and pairing invitations on Linux refuse IPv6.
- **Sync service.** `app/src/sync/service.rs` takes `AppHandle` throughout (26 uses): `setup` (:142), `sync_directory` (:1191), `PairingManager` through `app.state`, `emit_status` and `emit_applied`, and the key stores (:1225-1233; Android uses `FileKeyStore` in the app config directory). Timers are a 750 ms commit debounce, a 60 s fallback, and 1 s, 5 s, 30 s, and 300 s retries (:39-47). `SyncStatusView` (`sync/status.rs`) already has `last_exchange_at_ms` and `pending_local_changes`.
- **Android runtime.** `runtime/mobile.rs:440-470` starts sync, the handoff reconnect refresh (`receiver.rs:13-14`: 1 s connected repoll, 30 s disconnected retry), and contact polling in setup, and on `RunEvent::Resumed` runs reconciliation and `SyncRuntime::sync_now()`. Nothing runs once the process is gone.
- **Android manifest and SDK.** The app manifest declares only `INTERNET` and `CAMERA`; `minSdk` 29 and `targetSdk` 36 (`gen/android/app/build.gradle.kts:50-51`). No AndroidX WorkManager dependency exists. The Rust library is `ganbaru_lib`. Existing JNI exports (`crates/ganbaru-mobile-notifications/src/authority.rs:80`, :97) take primitives only and are listed in `docs/data/security/unsafe-rust.md`.
- **Pool registry.** `DatabasePoolRegistry` keeps one pool per path with one connection; the app manages it as Tauri state (`app/src/db.rs:6`).
- **Docs.** `docs/data/sync.md` names protocol 6 (:98), says Android exchanges only while the app process runs (:116), and plans a connected-device service (:233). `network-and-privacy.md` restricts the listener to a private LAN address (:15) and lists the dial ranges (:18). `docs/platforms/android/native-services-and-data.md:72` reserves WorkManager for future sync. `docs/platforms/desktop/README.md` describes the Linux scoped firewall action (:18) and the Windows per-program prompt (:19).

### Re-verification on 2026-10-21

Corrections and omissions found when the milestone started. Where this list and the bullets above disagree, this list holds.

- Bullet 1: Protocol: holds. PROTOCOL_VERSION = 6 at protocol.rs:16, validate_protocol at :1300 exact match. SyncState (:366). Enrolled :223, ports :28/:30. Enrolled written transport/server.rs:340; enrollment presenting a cert refused server.rs:281-289. SyncState built only in crates/ganbaru-sync-replica/src/hub.rs:432 (state_message); client reads at ganbaru-sync-replica/src/client.rs:116 and :413. New fields must be passed into SyncHub, built as SyncHub::new(open, changes) (APP/sync/service.rs:153).
- Bullet 2: Pin: struct pairing.rs:107-116, PAIRING_STATE_SCHEMA_VERSION = 2 (:36), version check pairing/storage.rs:115. Differs: record_coordinator (pairing.rs:780-806) rewrites the whole pin when a new invitation comes from the same coordinator (test pairing/tests.rs:348-367 "refresh coordinator endpoint"); endpoint changes only on relink. validate_state (pairing/storage.rs:176-180) rejects a pin whose endpoint does not parse as SocketAddr; new fields need validation there.
- Bullet 3: Dial rules: holds. dial.rs authorize_dial_target :91-112. ipv6_source_for transport.rs:745. LOOPBACK_DIAL cfg!(debug_assertions) (transport.rs:726-730). Existing helpers: ipv4_lan_block (dial.rs:144, includes overlay block), shares_on_link_prefix (:115), ipv4_mask (:134), private is_global_unicast (:197). network_access.rs uses ipv4_lan_block and ipv4_mask. PrivateLan includes IPv6 link-local. IPv6 documentation refused at :189.
- Bullet 4: Pinned dials: connect_pinned :754, 10 s timeout :770. 8 coordinator_pin() reads in transport.rs (:223, 353, 432, 472, 516, 533, 553, 607). Three sites parse the endpoint (:359-362, :554-557, :611-614). App crate has 30 reads, 24 outside tests. authenticated_exchange has 9 callers, not 5: also commit_staged_ownership (:273), cancel_prepared_transfer (:303), acknowledge_activation (:323), commit_uploaded_ownership (:407). connect_pinned also used by enroll (:171, invitation endpoint) and unauthenticated_exchange (:500, contact-card endpoint). Every sync message opens a new TCP+TLS connection (one request, one response per connection), so the resolver runs once per request.
- Bullet 5: Listener: start_on :104 (check :120, bind :123), lan_admission :414 and :435, ensure_desktop_coordinator :350, discover_private_lan_address :802 (IPv4 route probe to 192.0.2.1), is_lan_address :819 (allows loopback in all builds, no cfg gate, leaves out 100.64.0.0/10). Differs: start_desktop_when_reachable (:329-345) has no wait loop; calls ensure_desktop_coordinator once; error only logged (runtime/desktop.rs:938-940, vault.rs:667); nothing retries until an invitation or contact-card action calls ensure_desktop_coordinator again (contacts.rs:227). Differs: in the accept loop admission (server.rs:130) runs inside the spawned task after limits.admit(source.ip()) took a pending-budget permit (:118). Fail-open in two places: admission.rs:73-76 (failed spawn_blocking) and network_access::admits_lan_peers (:142) admits when state is unreadable (status returns ManualActionRequired). On Linux lan_admission returns admit-all for an IPv6 endpoint (handoff.rs:418-420).
- Bullet 6: Linux firewall: local_address: Ipv4Addr (:49); routes only from /proc/net/route (:319, :384, parsed :408). TCP-only parsing :905 (UFW), :932 (firewalld, family="ipv4" only :926). validate_rule_addresses :965; gateway hardware address as network identity (current_network :318, allowed_on :172). Differs: privileged_helper_arguments is :616-646, accepts only production COORDINATOR_PORT (43822 refused). parse_privileged_helper_scope hardcodes COORDINATOR_PORT (:758); both parsers only recognize 43821. handoff_revoke_network_access takes no address, does not refuse IPv6; IPv6 refusals in grant (:451), invitation (:401), desktop_network_access_status (:702). AuthorizedScope also has firewall: FirewallKind and zone: Option<String>; source_network is a String; GrantedScope adds networks; state file STATE_SCHEMA_VERSION = 2 (:15), read_state rejects other versions (:1137); is_valid_interface (:448) enforces [A-Za-z0-9._-]{1,15}; lan_scope (:392) narrows to ipv4_lan_block (includes overlay); firewalld rich rules carry no interface (:923-948); builds without the packaged helper call ufw/firewall-cmd directly via pkexec (apply_ufw :995, apply_firewalld :1027); revoke-all wired in packaging/deb/prerm:8 and packaging/rpm/preremove.sh:6. Root helper crates/ganbaru-linking-firewall/src/main.rs has a test that the Cargo.lock closure of ganbaru-handoff contains no GUI crates: anything added to ganbaru-handoff (such as mdns-sd) ends up in the root helper.
- Bullet 7: Sync service: 26 AppHandle occurrences incl. use line; setup :142; sync_directory :1191; timers :39-48. Key stores :1224-1233; Android FileKeyStore in <app_config_dir>/sync. emit_status/emit_applied status.rs:222/:231; status via SyncSubscribers channels. View already has last_exchange_at_ms, pending_local_changes, state Offline, SyncErrorCode (only WriterLimit). Client exchange (ClientSession, SyncTransport, wait), hub, writer, assets, access, carry_forward already live in crates/ganbaru-sync-replica (no Tauri dep; APP/sync.rs:14-16). Only service.rs, status.rs, transport.rs are app-side. Hub long-poll WAIT_TIMEOUT 12 s (hub.rs:27).
- Bullet 8: Android runtime: setup runtime/mobile.rs:440-454, Resumed/Exit :459-469 (Resumed also wakes distractions). Constants receiver.rs:13-14. Loop :475 after initial 5 s sleep, runs probe_coordinator.
- Bullet 9: Android manifest and SDK: app manifest has only INTERNET and CAMERA; minSdk/targetSdk app/build.gradle.kts:50-51 (compileSdk 36 :44); no WorkManager. ganbaru_lib is the lib of crate ganbaru (apps/client/src-tauri/Cargo.toml:12). Third JNI export: crates/ganbaru-mobile-media/src/authority.rs:45 (NativeMusicAuthority.isCurrent), listed in unsafe-rust.md. Exports use raw *mut c_void; no crate uses the jni crate. Each exporting module has a module-level unsafe_code allow (authority.rs:4) and must be an "Owned module" in unsafe-rust.md (check-unsafe-inventory.mjs enforces). check-android-project.mjs:366 requires the app manifest to declare exactly 2 permissions. Merged manifest already declares FOREGROUND_SERVICE (media and notifications plugins). App depends on androidx.lifecycle:lifecycle-process:2.10.0 (app/build.gradle.kts:179). Tauri 2.11.2 Plugin.kt has open fun onStop() at :93. Plugin crates listed in apps/client/src-tauri/Cargo.toml:40-43 and app/Cargo.toml:75-78, registered runtime/mobile.rs:16-19.
- Bullet 10: Pool registry: db.rs:6 pub use ganbaru_db::DatabasePoolRegistry as DatabaseState; max_connections(1) (ganbaru-db/src/lib.rs:185). Managed state runtime/mobile.rs:25, desktop.rs:227. Registry is #[derive(Clone, Default)] around Arcs (lib.rs:66-70). db.rs has process-global VAULT_CONNECTION_GATE and SELECTED_DATABASE.
- Bullet 11: Docs: hold (sync.md:98, 116, 233; network-and-privacy.md:15, 18; native-services-and-data.md:72; desktop/README.md:18-19).
Other: contacts/card.rs:89-105 resolve_delivery_hint uses CoordinatorLifecycle::endpoint() (bound address), also used by invitation, firewall status, and grant; with a wildcard bind it would return 0.0.0.0/::. Linked devices' cards use pin.endpoint (card.rs:91-92). vault.rs has read_app_state_from_path (:186) and cached read_app_state (:295); active_vault/active_vault_id (:880, :974) take AppHandle. VaultOwnershipManager and PairingManager are managed state. Present: openBackgroundExecutionSettings (ganbaru-mobile-notifications/build.rs:15, Kotlin :411), MobileNotificationsPluginTest.kt, src/lib/api/sync.test.ts, src/lib/sync/status.test.ts, i18n sync.ts.
Cargo.lock: not present: mdns-sd, if-addrs, socket-pktinfo. Locked: socket2 0.6.4, ring 0.17.14 (workspace dep, not direct in ganbaru-handoff), jni 0.21.1 and 0.22.4, flume 0.12.0, mio 1.2.1, fastrand 2.4.1, log 0.4.31. WorkManager: none in any gradle file.
Machine note: no Android SDK/NDK on this build host; Android builds cannot run here.

### Decisions taken at start (2026-10-21)

- The root-run firewall helper links `ganbaru-handoff`, and its test inspects the `Cargo.lock` closure, which includes optional dependencies. Discovery therefore splits: the pure parts (record format, member tag HMAC, discovery key type, candidate filtering through the address policy) live in `ganbaru-handoff/src/discovery.rs` (adding only `ring`, already locked), and the `mdns-sd` advertiser and browser live in the app crate under the `desktop` cfg. `mdns-sd` never enters the root helper's closure; step 5 adds a helper test assertion for it.
- No Android SDK or NDK exists on the build host. Step 7 runs clippy, the source-level Android project checks, and the unsafe inventory check; the Android build is recorded as not run on this host and moves to physical acceptance.

## Design

### Address policy

One policy in `dial.rs` (kept in place, its module doc widened from outgoing to all peer addresses) decides every address the link touches:

| Use | Function | Rule |
| --- | --- | --- |
| Outgoing dial | `authorize_dial_target` (unchanged) | Private, link-local, overlay, ULA, on-link global IPv6; loopback in debug builds. |
| Inbound peer before TLS | `admit_inbound_peer(peer, local)` | Same classes; a global IPv6 peer is admitted only when it shares the /64 of `local`, the accepted socket's own address. |
| Reported and advertised address | `reportable_address(address)` | Private IPv4, IPv4 link-local, overlay, ULA, and global IPv6 that is not documentation. Never IPv6 link-local (its scope id is meaningless on another host), never loopback. |
| Discovery candidate | `validate_dial_endpoint` then `authorize_dial_target` | As a dial. IPv6 link-local results are dropped. |

Decisions:

- On-link global IPv6 stays allowed. The README row lists the private and overlay ranges and refuses public addresses; an address inside the device's own /64 is the LAN on IPv6-only home networks, the dial rule and `network-and-privacy.md:18` already allow it, and contact delivery depends on it. Admission mirrors it with the accepted socket's local address, which needs no route lookup.
- `is_lan_address` and `discover_private_lan_address` are deleted. Release builds stop admitting loopback peers.
- Refused inbound peers are dropped right after `accept`, before the pending-connection budget and before any byte is read.
- With a wildcard bind, this address check is the only application gate on which networks reach the listener, so it is pure and never fails open. The Linux firewall decision (`transport/server/admission.rs:73-76`) keeps admitting when its state cannot be read, because it gates network identity rather than address class and mutual TLS still authenticates every peer.

### Listener

- Two sockets built with `socket2`: `0.0.0.0:port` and `[::]:port` with `IPV6_V6ONLY` set, both with `SO_REUSEADDR` off on Windows and on elsewhere as today. If the IPv6 socket fails (IPv6 disabled), the listener runs on IPv4 alone and logs once; if IPv4 fails, start fails as today.
- One accept future selects over both sockets and feeds the existing `serve_with_limits`; limits, budgets, and timeouts are unchanged.
- `LanAdmission` is keyed by the accepted socket's local address: `admits(local: SocketAddr)` keeps a map of at most 8 cached decisions, replacing today's single cached decision; each entry keeps the 5 s recheck. On Linux the decision is `network_access::admits_lan_peers(config_dir, local.ip(), port)`, now taking `IpAddr`. Other desktops admit.
- The listener binds at once instead of waiting for a LAN address, because a wildcard bind never depends on the current address. `start_desktop_when_reachable` loses its wait loop.
- The invitation endpoint is `preferred_invitation_address()`: the IPv4 source the OS routes to a documentation address (the existing probe, kept as a private helper), if `reportable_address` accepts it; otherwise the first reportable IPv4 address from `if-addrs`; otherwise the first ULA. With none, invitation creation fails with the existing "no local network" error.

### Address refresh and protocol bump

`PROTOCOL_VERSION` (6 today) takes the next free number when step 3 merges, following the README [wire versions rule](README.md#parallel-work-reservations). `SyncState` gains two fields:

| Field | Type | Bound |
| --- | --- | --- |
| `coordinator_endpoints` | `Vec<String>` | At most 8 socket addresses; each passes `validate_dial_endpoint` and `reportable_address`, or the message is refused. |
| `discovery_key` | `String` | Base64url of exactly 32 bytes. |

- The hub fills `coordinator_endpoints` from `if_addrs::get_if_addrs()` filtered by `reportable_address`, ordered private IPv4, ULA, global IPv6, then overlay, with the listener port, cached for 5 s. Overlay goes last because a LAN path is faster when both work.
- The key travels in `SyncState`, not in `Enrolled`. `SyncState` is the first authenticated response after enrollment and is mutual TLS; `Enrolled` is server-pinned only. Existing links get a key on their next exchange with no relink. Milestone 11 rotates the key through removal entries and removes the `CoordinatorPin` field, so the pin field below is interim.
- The coordinator keeps one key per vault in its pairing state file (`discovery_keys: BTreeMap<vault_id, String>`), generated with `ring::rand::SystemRandom` the first time a vault needs one, and dropped when a vault has no linked peers left. The file is already private and holds the device's identity, so a 32-byte non-identity secret needs no keyring entry.
- `CoordinatorPin` gains `#[serde(default)]` fields: `last_endpoint: Option<String>`, `reported_endpoints: Vec<String>` (at most 8), and `discovery_key: Option<String>` (interim until milestone 11). `endpoint` stays the invitation endpoint. `PAIRING_STATE_SCHEMA_VERSION` stays 2: absent fields are a valid current state (a pin before its first exchange), not a legacy format, which matches how `device_label` and `generation` were added.
- `PairingManager::record_coordinator_contact(vault_id, fingerprint, update)` writes the pin only when a field changed and only while the pin still names that vault and fingerprint, so an unlink racing an exchange never resurrects a pin. A steady network therefore writes nothing.

### Coordinator dial resolver

One function replaces the three dial sites: `connect_coordinator(manager, identity, discovered) -> (TlsStream, SocketAddr)`.

- Candidates, deduplicated, at most 12: `last_endpoint`, then `reported_endpoints`, then discovered addresses, then the invitation `endpoint`. Each passes `authorize_endpoint` or is skipped.
- Connection is a staggered race: a new TCP attempt starts every 250 ms in candidate order, each with a 3 s connect timeout. Connected sockets go through TLS in completion order; a TLS failure (another device now holds that address, so the pinned fingerprint does not match) moves to the next connected socket. The first authenticated stream wins and the rest are dropped.
- On success the resolver records `last_endpoint` through `record_coordinator_contact`. The exchange records `coordinator_endpoints` and `discovery_key` from `SyncState`.
- When every candidate fails and the pin has a discovery key, the caller runs discovery once (desktop: an `mdns-sd` browse; Android in-app: the plugin's NSD browse; Android headless: the worker, see below), at most once per 60 s, and retries with the results. Discovered addresses are kept in memory for 10 minutes and never persisted; only a successful dial persists, as `last_endpoint`.

The handoff receiver, bundle downloads, owner polls, person key requests, and sync all use the resolver, so every coordinator path follows address changes.

### Discovery

Pure logic lives in `crates/ganbaru-handoff/src/discovery.rs`, role-agnostic so milestone 11 adds callers only:

- `member_tag(key, fingerprint, hour) -> String`: HMAC-SHA256 with `ring::hmac` over the domain `ganbaru-sync/discovery/v1` followed by a zero byte, the 32 fingerprint bytes, and the hour bucket (`unix_seconds / 3600` as big-endian `u64`), truncated to 16 bytes and hex encoded (32 characters, inside the 63-byte DNS label limit).
- `expected_instance_names(key, fingerprints, now) -> Vec<String>`: tags for the hours h minus 1, h, and h plus 1, which tolerates an hour of clock skew.
- `advertisement(key, fingerprint, port, addresses, now) -> Advertisement`: instance name (the tag), a random host name `<16 hex>.local.` chosen once per daemon start so the OS host name never leaks, TXT `v=1`, the port, and the reportable addresses.
- `match_candidates(expected, records) -> Vec<SocketAddr>`: keeps records whose instance name is expected and whose addresses pass the dial policy, at most 8.

The instance name carries the tag, so browsers filter by name and resolve only matching services. That keeps Android resolves, which are serialized before API 34, to one per coordinator.

Desktop runner (`app/src/vault/handoff/discovery.rs`, desktop only):

- One `mdns_sd::ServiceDaemon` per app run, created lazily on first need, with loopback interfaces disabled and `default-features = false` (no `async`, no `logging`). A dedicated std thread drains the daemon's `flume` receivers into a tokio channel.
- The coordinator advertises while its listener runs and the active vault has at least one linked peer. It registers explicit reportable addresses (never `enable_addr_auto`, which would publish every interface address), re-registers on each hour boundary for the new tag, and re-registers when `DaemonEvent::IpAdd` or `IpDel` arrives. It unregisters when the listener stops.
- A linked client browses only on demand, for 3 s, when the resolver has no reachable candidate. It never advertises in this milestone.

Android uses `NsdManager` from the plugin: `discoverServices("_ganbaru._tcp", PROTOCOL_DNS_SD)` for at most 3 s, resolve only services whose name is in the expected set (`registerServiceInfoCallback` on API 34 and newer, `resolveService` below), and return name, port, and IPv4 and ULA addresses. NSD needs no multicast lock. Rust re-checks names and addresses with `match_candidates`.

Privacy: a non-member sees that a Ganbaru coordinator exists on the LAN (service type and port), a name that changes every hour, a random host name, and its LAN addresses. It cannot link two hours of advertisements to one device or vault.

### Linux firewall

`AuthorizedScope` becomes `{ protocol: Tcp | Udp, interface, local_network: (IpAddr, prefix), source_network: (IpAddr, prefix), port }`:

- The destination is the local network prefix, narrowed to the policy block like the source, instead of one host address, so a new lease inside the same subnet keeps working without another prompt.
- IPv6 routes come from `/proc/net/ipv6_route`. IPv6 scopes on an interface use the same network identity as that interface's IPv4 default gateway; an interface with no IPv4 gateway (IPv6-only or overlay) has no identity and counts as allowed, as today.
- Overlay interfaces (an address in `100.64.0.0/10`, or a ULA on an interface with no default route) get an interface-qualified rule with the overlay block as source. Tailscale keeps its routes in table 52, so overlay scopes are built from the interface address and block, not from a main-table connected route.
- The coordinator grant asks once for every scope it needs: TCP on the listener port for each allowed interface and family, plus UDP 5353 from the same sources to `224.0.0.251` and `ff02::fb`. A failed or unsupported UDP rule leaves discovery degraded and never blocks the TCP grant.
- The helper's argument validator accepts exactly two shapes: TCP on 43821 or 43822, and UDP on 5353. Interface names must match `[A-Za-z0-9._-]{1,15}`. Parsers for UFW and firewalld read both protocols and families, and accept a host destination as a /32 or /128 prefix, so rules from earlier builds are still recognized and removed.
- Revoke-all in `deb` prerm and `rpm` preremove finds the new shapes by the same comment, so removing the package removes every rule.
- Client desktops request no firewall rule in this milestone: they listen on no port until milestone 11, and inbound mDNS responses are multicast, which Ubuntu's UFW `before.rules` and firewalld's default zones already accept. If a client's browse finds nothing while a firewall is active, status shows the firewall reason instead of prompting.

Windows keeps the per-program prompt; the wildcard sockets trigger it the same way the single address did.

### Tauri-free sync host

A cold WorkManager start has no Tauri app, so the service gets a host abstraction instead of a parallel implementation:

- `trait SyncHost: Clone + Send + Sync + 'static` in `apps/client/src-tauri/app/src/sync/host.rs`, beside the service loop it parameterizes ([milestone 12](milestone_12.md) step 1 moves both into `ganbaru-sync-replica`), with `config_dir`, `pairing`, `pools`, `key_store`, `active_vault`, `emit_status`, and `emit_applied`. `AppHandle<R>` implements it with today's behavior; `HeadlessHost` implements it from a config directory, with no-op emitters.
- Every `AppHandle` use in `sync/service.rs` and the client exchange moves behind the trait. The hub stays desktop only and keeps `AppHandle`.
- Android needs only files for this: the vault path and ownership come from `app-state.json` and the vault marker, writer keys from `FileKeyStore`, and the TLS identity from the pairing state file. Functions in `vault` and `vault::ownership` that the pass calls gain config-directory variants; the `AppHandle` versions call them.
- Milestone 2's post-apply hooks are host-free: `MaterializeHook` receives the connection and a change sink trait, never an `AppHandle`. Step 6 wraps them: each host implements the change sink (the `AppHandle` host forwards to `emit_applied`, `HeadlessHost` drops the notices), so the headless pass runs the same registered hooks. Step 6 starts after milestone 2 step 7 merges; the network track does steps 1 to 5 first.

Process rules on Android:

- The pool registry becomes a process-global `Arc` (`OnceLock`) shared by the Tauri app and the headless entry, so the process never has two pools on the vault. The connection hook is installed on that registry before any pool opens, by whichever starts first.
- A process-global async mutex is the pass gate. The service's pass and the headless pass both hold it; vault handoff quiescence already stops the service and so waits on it too.
- The Tauri runtime registers a weak handle to its `SyncRuntime` at setup and clears it on `RunEvent::Exit`. When a headless call finds a live handle, it calls `sync_now()` and waits up to 60 s for the next status publication instead of running its own pass.
- A headless pass seals, pushes, pulls, applies, and moves assets within the existing per-pass bounds, then returns. It never opens a long poll, never runs whole-vault refresh, and leaves pools open for the rest of the process.

### Android background sync

New crate `crates/ganbaru-mobile-sync` in the existing plugin pattern (`Cargo.toml`, `build.rs` with `.android_path("android")`, `src/lib.rs`, `src/mobile.rs` with `register_android_plugin`, Kotlin under `android/`). The JNI entry lives in the app crate (`app/src/sync/headless.rs`, Android only), because it calls the sync service.

Rust tells the plugin its state through one command, `configure { linked, pending, configDir }`, sent when the linked state or `pending_local_changes > 0` changes. The plugin stores `linked` and `configDir` in its private preferences and keeps `pending` in memory.

| Trigger | Mechanism |
| --- | --- |
| Periodic | Unique periodic work `ganbaru-sync-periodic`, 15 minutes, `NetworkType.CONNECTED`, policy `UPDATE`; enqueued when linked, canceled when unlinked. WorkManager reschedules it after reboot. |
| Network change, process alive | `registerDefaultNetworkCallback` from the plugin; `onAvailable` and `onLinkPropertiesChanged` enqueue unique one-time work `ganbaru-sync-now` (policy `KEEP`, `CONNECTED`). |
| Network change, process dead | No callback exists. The periodic job's network constraint and any pending `ganbaru-sync-now` job, whose constraint waits for a network, cover it. |
| Backgrounded with pending changes | Plugin `onStop` with `pending` enqueues `ganbaru-sync-now`, expedited with `RUN_AS_NON_EXPEDITED_WORK_REQUEST` on API 31 and newer, plain below. Expedited work on API 29 and 30 needs a foreground service, which this design forbids. |

The worker (`CoroutineWorker`, main process):

1. Returns success at once unless linked and the active network has a Wi-Fi, Ethernet, or VPN transport. Cellular alone cannot reach a LAN, and Tailscale on cellular reports VPN.
2. Loads `ganbaru_lib` and calls `SyncNative.runPass(configDir, null)`, which returns a small JSON result `{ outcome, discoveryNames }` with outcome `synced`, `idle`, `unreachable`, `delegated`, or `failed`.
3. On `unreachable` with names, browses NSD for 3 s and calls `runPass(configDir, records)` once more.
4. Returns `Result.retry()` on `unreachable` or `failed` for the first two attempts of one-time work (WorkManager backoff from 30 s), and success otherwise.

The JNI export uses `jni` 0.21 types in its signature (`JNIEnv`, `JString`, returning `jstring`) with no `unsafe` block; the only unsafe attribute is `export_name`, as in the existing callbacks. Local references never outlive the call. Input is bounded (config directory at most 4 KiB, records JSON at most 16 KiB and 16 records) and validated before use; panics are caught and reported as `failed`. The export is added to the unsafe inventory.

Manifest: `ACCESS_NETWORK_STATE` (normal permission, needed by the network callback and WorkManager constraints). WorkManager's merged `SystemForegroundService` is removed with `tools:node="remove"`, and a bundle contract check asserts that the merged manifest declares no WorkManager foreground service.

Background status probes, read by the plugin command `connectivityStatus` on start and resume: `ActivityManager.isBackgroundRestricted()` and `UsageStatsManager.getAppStandbyBucket()` equal to the restricted bucket. Battery optimization alone is not reported, because WorkManager still runs in Doze maintenance windows.

### Status and UI

`SyncStatusView` gains:

- `connection: Option<SyncConnectionView { network: Lan | Overlay, address: String }>`, set by the last successful dial in this run. The network comes from the dial class.
- `degraded: Vec<SyncDegradedReason>`: `BackgroundRestricted`, `StandbyRestricted` (Android), `LocalNetworkDenied` (a connect that fails with `EPERM` or `EACCES`, and on Android 17 the permission check), and `FirewallAuthorizationRequired` (Linux coordinator whose listener scopes are not granted, or a client browse that found nothing behind an active firewall).

The Settings sync section shows "Connected over local network" or "Connected over overlay network" with the address, the existing last exchange time, and one row per degraded reason with its recovery action: Android battery and background settings through the existing `openBackgroundExecutionSettings` command of `ganbaru-mobile-notifications`, and the Linux firewall action already in Data settings. Strings go in `sync.ts` under `sync.connection` and `sync.degraded`, in English and Spanish. The frontend reader validates both fields like the existing ones.

### Dependencies

- **`if-addrs` 0.15**, pinned exactly, added in step 2 for interface enumeration (invitation address and reported endpoints). Accepted: established (about 31 million downloads, maintained by `messense`), small FFI over `getifaddrs` and `GetAdaptersAddresses`, and the same crate `mdns-sd` uses, so step 5 adds no second enumeration library. Hand-written enumeration would mean owning that FFI.
- **`mdns-sd` 0.21.6**, pinned exactly, `default-features = false`. Accepted. It is `#![forbid(unsafe_code)]`, about 15k lines, actively released since 2021, about 6.5 million downloads, MIT or Apache-2.0. Its risk is a single owner and publisher (`keepsimple1`); exact pinning, cargo-audit, and a reviewed diff on every bump contain it. Its remaining new transitive crate is `socket-pktinfo` 0.4 (single owner, created 2024, about 600 lines with packet-info FFI), which only handles link-local multicast sockets behind the address policy and mutual TLS. The review is recorded in this plan's progress.
- **Fallback if the review rejects `mdns-sd`**: desktop does not advertise or browse, and address refresh carries reconnection. A hand-written mDNS responder is not an option, because a multicast DNS parser is a worse surface to own than a reviewed crate. Android NSD then has nothing to find, so the plugin's browse is left out too.
- **`androidx.work:work-runtime`**, pinned to the newest stable release at least two weeks old. Official AndroidX, already the documented choice for deferrable work. It brings Room and AndroidX Startup into the APK.
- `socket2` 0.6, `ring` 0.17, and `jni` 0.21 are already locked transitive or workspace crates.

## Testing

- **Address policy** (`ganbaru-handoff`): every class for dial, admission, and reporting; IPv4-mapped IPv6; global IPv6 admitted only within the local /64; IPv6 link-local never reportable; loopback refused for admission in release policy.
- **Protocol bump**: `SyncState` round trips; nine endpoints, a public endpoint, a malformed key, or a 31-byte key refuse the message; the previous protocol version is refused with the update request.
- **Pairing**: a pin without the new fields loads; contact updates write only on change; an update for a different fingerprint or vault after unlink writes nothing; coordinator keys are created once per vault and dropped with the last peer.
- **Resolver**: candidate order, deduplication, the cap, skipped unauthorized candidates; with two loopback listeners in a debug test, a wrong-fingerprint listener first in order still yields the right one; `last_endpoint` recorded.
- **Listener**: dual binding, IPv4-only fallback when the IPv6 socket fails (injected), admission keyed by local address.
- **Discovery**: golden tag vectors, hour skew acceptance at h minus 1 and h plus 1 and refusal at h plus 2, name filtering, address filtering, advertisement contents (random host name, no auto addresses). The desktop runner gets one loopback-free unit test of the hour re-registration schedule with an injected clock; real multicast is manual.
- **Firewall**: UFW and firewalld parsers for UDP, IPv6, prefix and host destinations, and interface-qualified rules; the helper validator rejects other ports, protocols, interfaces, and public sources; revoke-all finds every shape.
- **Headless host**: on the desktop test target, `HeadlessHost` from a temporary config directory runs one client pass against an in-process hub and converges, running a registered fixture hook through its change sink; a pass while the gate is held waits; delegation to a live runtime handle runs no second pass.
- **Frontend**: `sync.test.ts` and `lib/sync/status.test.ts` for the new fields and reason mapping.
- **Kotlin** (JVM unit tests in the plugin, following `MobileNotificationsPluginTest.kt`): trigger decisions (linked, pending, transport, API level choosing expedited or plain work), worker outcome to `Result` mapping, and NSD name filtering. Real WorkManager scheduling, Doze, and NSD on a network are covered by manual acceptance.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement). Steps 2, 5, and 7 change dependencies and also run `pnpm -w run validate:full`.

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-address-policy` | `admit_inbound_peer` and `reportable_address`, pre-TLS refusal in the accept loop, `is_lan_address` removed. | `cargo test -p ganbaru-handoff --lib -j 4 dial -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 handoff -- --test-threads=4` |
| 2 | `feat/sync-dual-stack-listener` | `socket2` wildcard sockets, admission keyed by local address, `if-addrs` review and dependency, invitation address choice, `discover_private_lan_address` and the reachability wait removed. | `cargo test -p ganbaru-tauri-app --lib -j 4 handoff -- --test-threads=4`, `cargo check -p ganbaru --bin ganbaru -j 4` |
| 3 | `feat/sync-address-refresh` | Next protocol version, `SyncState` endpoints and key, coordinator keys, pin fields, `record_coordinator_contact`, the coordinator dial resolver at every pinned site. | `cargo test -p ganbaru-handoff --lib -j 4 protocol -- --test-threads=4`, `cargo test -p ganbaru-handoff --lib -j 4 pairing -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4` |
| 4 | `feat/sync-firewall-scopes` | Scope model with protocol, IPv6, prefix destination, overlay interfaces; helper validator; parsers; revoke-all shapes; IPv6 invitations and grants on Linux. | `cargo test -p ganbaru-handoff --lib -j 4 network_access -- --test-threads=4`, `cargo test -p ganbaru-linking-firewall --bin ganbaru-linking-firewall -j 4 -- --test-threads=4` |
| 5 | `feat/sync-discovery` | `mdns-sd` review and dependency, `discovery.rs`, desktop advertiser and on-demand browser, resolver integration. | `cargo test -p ganbaru-handoff --lib -j 4 discovery -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 discovery -- --test-threads=4` |
| 6 | `refactor/sync-host` | `SyncHost` trait, `HeadlessHost`, change sink implementations for the post-apply hooks, config-directory variants in `vault`, process-global registry and pass gate on Android, runtime handle registration. Needs milestone 2 step 7. | `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 vault -- --test-threads=4` |
| 7 | `feat/sync-android-background` | `crates/ganbaru-mobile-sync`, WorkManager, network callback, NSD browse, status probes, JNI `runPass`, manifest and Gradle checks, unsafe inventory. | `cargo clippy --workspace -j 4 -- -D warnings`, the Android build in `docs/testing/android.md` |
| 8 | `feat/sync-connectivity-status` | Status view fields, degraded reasons from desktop and Android, Settings rows and actions, `sync.ts` strings. | `pnpm --dir apps/client exec vitest run src/lib/api/sync.test.ts --maxWorkers=1`, `pnpm --dir apps/client exec vitest run src/lib/sync/status.test.ts --maxWorkers=1`, `pnpm --dir apps/client run check` |
| 9 | `docs/sync-milestone-3` | Remaining docs, automated acceptance mapping, goal README update. | `pnpm -w run validate` |
| 10 | None | Physical acceptance on the devices below, recorded here. | Manual |

Steps 1 to 5 are desktop work and can merge while step 6 is in review. Step 6 is the largest and riskiest; it lands before any Kotlin so the Android step only wires triggers to a tested pass. The protocol bump (step 3) requires every test device to update together, as each protocol bump does.

## Documentation

- `docs/data/sync.md`: the pairing paragraph (:15) says the coordinator listens on every private and overlay address; the Linux firewall paragraph (:23) gains UDP 5353, IPv6, prefix destinations, and overlay interfaces; the exchange section names the new protocol version (:98) and gains a short connectivity paragraph (address refresh, dial order, discovery with member tags); the client trigger bullet (:116) replaces "Android exchanges only while the app process runs" with the WorkManager triggers; the keys section (:209) states that mutual TLS is the confidentiality boundary for direct links over any private or overlay range and that record encryption gates only intermediaries that store records (README "Docs differ" row for M3); the Android delivery paragraph (:233) becomes WorkManager only, with no foreground service, and lists the degraded reasons (README row for M3); the delivery table row "Delivery beyond the LAN and Android sync" (:245) drops Android background sync and the native background runtime from its remaining work.
- `docs/data/security/network-and-privacy.md`: the listener bullet (:15) describes the dual-stack wildcard listener and pre-TLS refusal of peers outside the allowed ranges (README row for M3); the dial bullet (:18) says the same ranges govern admission, reported addresses, and discovery candidates; a new bullet describes DNS-SD advertisement contents, member tags, and what a non-member learns (README discovery row for M3).
- `docs/platforms/android/permissions-and-security.md`: `ACCESS_NETWORK_STATE` in the permission plan (purpose, no denial path), no foreground service for sync, the removed WorkManager service, and the JNI entry's input bounds.
- `docs/platforms/android/native-services-and-data.md` (:72): WorkManager owns background sync; it still never owns deadlines, reminders, playback, or enforcement.
- `docs/platforms/desktop/README.md`: the Linux bullet covers the UDP discovery rule and IPv6; the Windows bullet notes the dual-stack listener and that the per-program rule also covers discovery (or the verified exception).
- `docs/data/security/unsafe-rust.md`: the `runPass` JNI export, its object references, and its bounds.
- `docs/testing/android.md` and `docs/testing/vault-handoff.md`: the background sync and address change rows of the manual matrix.
- `docs/features/README.md` and `docs/ROADMAP.md`: Android background sync and discovery status.
- `AGENTS.md`: the map line `ganbaru-mobile-*/` becomes "Android plugins for documents, the distraction blocker, media, notifications, and background sync" (the README reserves this line for milestone 3), and the `ganbaru-handoff` line mentions discovery tags.
- This folder: progress below and the goal README status.

## Manual acceptance

Two desktops (A, Linux with UFW, the coordinator; B, Windows, linked) and one Android phone (P, Android 14 or newer, plus one Android 10 device for the API 29 path). A and P also run Tailscale for the overlay rows.

| Scenario | Expected |
| --- | --- |
| A gets a new DHCP address in the same subnet while B and P are online | Both reconnect within one retry interval through reported addresses; no firewall prompt on A. |
| A moves to another subnet while P is closed; P opens later | P's remembered addresses fail, discovery finds A within 5 s, sync resumes; status shows the new address. |
| A is on Wi-Fi and Ethernet at once | `SyncState` reports both; P keeps using its last successful address. |
| P and A on different networks, both on Tailscale | Sync works over `100.x`; status says overlay network. |
| A's firewall active, fresh grant | One administrator prompt covers TCP and UDP, IPv4 and IPv6; P connects over IPv6 ULA where available. |
| Another device takes A's old address | The stale candidate fails TLS and the resolver continues; no error surfaced. |
| A non-member device browses `_ganbaru._tcp` for two hours | Sees two unrelated names and random host names. |
| P edits, then goes to home screen | Change reaches A within about a minute on API 31 and newer, and within the next job window on API 29. |
| P idle overnight with the app closed | A's edits arrive within about 15 minutes of P waking or on the next maintenance window; no notification shown. |
| P reboots and is not opened | Periodic sync continues after unlock. |
| P toggles airplane mode off with pending changes and the app closed | Sync runs once the network returns. |
| P on cellular only without Tailscale | Workers return without connecting. |
| P with background restriction enabled | Status shows the reason and opens the matching setting. |
| P opened while a background pass runs | No second pool, no error; the app shows fresh status after the pass. |
| P unlinked | Periodic work canceled; no workers run. |
| Uninstall the package on A | Every TCP and UDP rule, IPv4 and IPv6, is gone. |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Reconnect through a reported address after an address change | Under 2 s once the next pass starts |
| Discovery reconnect when every remembered address is stale | Under 5 s (3 s browse plus dial) |
| Background pass with nothing to exchange, Android | Under 1.5 s wall time |
| Background pass battery, Android | Not listed among the top consumers in system battery usage over a 24 hour idle day |
| Coordinator mDNS traffic | Responses to queries only, plus one announcement per hour and per address change |

## Risks

| Risk | Mitigation |
| --- | --- |
| The host refactor of the sync service touches every pass path | Its own step with no behavior change on desktop, headless tests against an in-process hub, and milestone 2 hooks host-free from the start (connection plus change sink). |
| OEM battery managers stop WorkManager on some phones | Status reports background restriction and standby bucket; existing battery guidance intents give the recovery action; foreground sync on resume is unchanged. |
| Single-maintainer crates in the discovery stack | Exact pins, cargo-audit, reviewed bumps, multicast-only exposure, and a fallback that drops discovery without losing sync. |
| `mdns-sd` and another responder (Avahi, Windows mDNS) share port 5353 | Shared binding is supported by the crate; verified on both desktops; failure degrades discovery only. |
| Overlay traffic blocked or bypassed by the overlay's own firewall chain | Verified with Tailscale on Linux; interface rules only when needed. |
| WorkManager adds Room and a foreground service declaration | The service is removed from the merged manifest and a Gradle check keeps it out; Room size is accepted. |
| The protocol bump forces all devices to update together | Same exact-version handling and status message as earlier bumps; acceptable before external users. |
| A stale discovery key after a device is removed lets it recognize advertisements | It still cannot authenticate; milestone 11 rotates the key through removal entries. |

## Deferred

- Any member advertising and serving peers, and peer address exchange (milestone 11).
- Discovery key rotation through removal entries and removal of the `CoordinatorPin` key field (milestone 11).
- Relay and always-on anchor reachability across networks without an overlay (milestone 12).
- Multiple endpoints in contact cards.
- macOS firewall and discovery behavior.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- **Runtime:** Android local network protection: if the compile SDK in use defines `ACCESS_LOCAL_NETWORK` and the app targets an SDK that enforces it, declare it, request it when linking starts (next to the camera request), and map denial to `LocalNetworkDenied`. Otherwise keep `targetSdk` and detect `EPERM` only.
- **Runtime:** Windows Defender: confirm the per-program rule covers both wildcard sockets and inbound UDP 5353. If UDP is not covered, document that Windows coordinators rely on address refresh and add no rule.
- **Runtime:** `mdns-sd` on Windows beside the system mDNS responder, and on Linux beside Avahi: if either cannot share 5353, disable discovery on that platform and document it.
- **Runtime:** Tailscale on Linux: if its `ts-input` chain or nftables table already accepts traffic on `tailscale0`, overlay interfaces report no rule needed; otherwise add the interface rule from the design.
- **Runtime:** Whether Ubuntu's UFW `before.rules` and the target firewalld zones accept mDNS already; if they do, the UDP scope is still added on the coordinator (harmless duplicate) and client desktops never need one.
- **Runtime:** That Tauri's async runtime can be used before any Tauri app exists (`tauri::async_runtime::block_on` from the JNI entry). If not, the headless entry builds one current-thread tokio runtime per call and the gate stays process-global.
- **Runtime:** That Tauri's Android plugin base class exposes an `onStop` hook; if not, use `ProcessLifecycleOwner` from `androidx.lifecycle:lifecycle-process` (already pulled in by WorkManager).
- **Runtime:** WorkManager's merged manifest entries for the pinned version: remove only `SystemForegroundService`, unless the version declares another component that starts foreground work, in which case remove it too and extend the bundle contract check to cover it.
- **Runtime:** Whether the Tauri runtime survives Activity destruction on Android; the runtime handle must be cleared whenever `SyncRuntime` stops, not only on `RunEvent::Exit`.
- **Earlier step:** That `vault::ownership` and active-vault resolution need nothing from Tauri beyond the config directory; anything else found becomes a `SyncHost` method.

## Progress

- [x] Step 1: address policy.
  - 2026-10-21, `feat/sync-address-policy`, commits 3bd3618f and 84bf1734. `admit_inbound_peer(peer: IpAddr, local: IpAddr, loopback: LoopbackPolicy)` takes IP addresses (the peer port is ephemeral) and the loopback policy as a parameter like `authorize_dial_target`, so release policy is testable from debug runs; mapped IPv6 is canonicalized on both sides. `reportable_address` returns the class (for step 3 ordering) and refuses IPv6 link-local with a new `DialTargetError::LinkLocalIpv6`. The app const is `LOOPBACK_PEERS` (dial and admission); `serve_with_limits` takes the policy so a test can force `Reject`.
  - The address gate runs synchronously in the accept closure before `limits.admit`, drops silently (no per-connection log), and refuses when `local_addr` fails. `LanAdmission` stays in the spawned task with its fail-open kept, per Design. `discover_private_lan_address` is kept for step 2 (Work breakdown) and checks `reportable_address`, so an overlay route source is now accepted for the bind; `start_on` checks `reportable_address` too. Docs: `network-and-privacy.md` listener and dial bullets, `sync.md:15`.
- [x] Step 2: dual-stack listener.
  - 2026-10-21, branch `feat/sync-dual-stack-listener` from `feat/sync` at 19315b48. Commits: 42cc7666 `build(handoff): add if-addrs and socket2 for the dual-stack listener`, 0aeddae1 `feat(handoff): listen on dual-stack wildcard sockets with admission per local address`.
  - Implementation check: the "no wait loop" branch held (start_desktop_when_reachable never looped); it is kept as is, and only `discover_private_lan_address` was removed. `ensure_desktop_coordinator` now returns the port (`Result<Option<u16>, String>`), and `CoordinatorLifecycle::endpoint()` is replaced by `port()` and `reportable_endpoint()`.
  - if-addrs review: 0.15.0 pinned exactly with the `link-local` feature (the same feature set mdns-sd 0.21 requests, so step 5 adds no variant). Single owner and publisher `messense` (account since 2012), crate since 2020, 24 versions, about 31M downloads, latest release 2026-02-08. About 1.2k lines; unsafe is FFI over getifaddrs/if_nametoindex and GetAdaptersAddresses with read-only pointer walks. Dependencies are libc 0.2 and windows-sys 0.61.2, both already locked, so the only new lock entry is if-addrs itself; registry checksum matches. No RustSec advisories. socket2 0.6.4 was already locked via tokio; its only advisory (RUSTSEC-2020-0079) is fixed since 0.3.16. dependency-audits.md lists only active exceptions and warnings, so it was not changed. `pnpm -w run audit`: no vulnerabilities, 10 pre-existing allowed warnings.
  - Invitation address (`vault/handoff/local_addresses.rs`): reportable routed IPv4 source, else a private IPv4 interface address before IPv4 link-local or overlay, else the first ULA; global IPv6 is never chosen. Interfaces reported down, not present, or lower-layer down are skipped. Contact card delivery hints and Linux firewall status and grant use the same choice.
  - Linux: `admits_lan_peers` takes `IpAddr` and admits IPv6 local addresses, and firewall status reports not required when the chosen address is IPv6 or none exists (interim until step 4 adds IPv6 scopes). Grant still errors without an IPv4 address.
  - Checks passed: `cargo test -p ganbaru-tauri-app --lib handoff` (67), `cargo test -p ganbaru-handoff --lib network_access` (19), `cargo check -p ganbaru --bin ganbaru`, clippy on both crates with `--all-targets -D warnings`, `cargo fmt --check`, `pnpm -w run audit`.
  - Note for later steps: the shared CARGO_TARGET_DIR can hold a ganbaru-handoff artifact built from another worktree (Cargo hashes path crates without their location), which surfaced as missing step 1 symbols in clippy; touching the crate sources forced a correct rebuild.
- [x] Step 3: address refresh and protocol bump.
  - Date: 2026-10-21 to 2026-10-22. Branch `feat/sync-address-refresh` (from feat/sync at 7c63c0a2).
  - Commits: 2838648c `feat(handoff): report coordinator endpoints and discovery key in sync states`, a39247c8 `feat(handoff): dial the pinned coordinator across its known endpoints`, 43beba8f `refactor(handoff): name the sync hub through the app sync module`. All signed; the first commit was checked to compile on its own.
  - Checks: ganbaru-handoff and ganbaru-sync-replica full tests, ganbaru-tauri-app `handoff`, `sync`, `contacts` filters, clippy `--all-targets -D warnings` on the three crates, `cargo fmt --check`.
  - PROTOCOL_VERSION 6 to 7. `SyncState` gains `coordinator_endpoints` (at most 8, each must pass `validate_dial_endpoint` and `reportable_address`) and `discovery_key` (32 random bytes, unpadded base64url).
  - Pairing state schema stays 2: new `CoordinatorPin` fields (`last_endpoint`, `reported_endpoints`, `discovery_key`) and the coordinator-side `discovery_keys` map are serde defaults, validated on load. Discovery keys are created on first request per vault, refused for vaults without a linked peer, and pruned with the last peer.
  - `record_coordinator` sets `last_endpoint` to the invitation endpoint and keeps reported contact on a same-coordinator relink. `record_coordinator_contact` writes only on change and only for the current pin (vault and fingerprint), rolling back on persist failure.
  - Hub contact is injected through `SyncHub::with_contact` from `vault/handoff.rs`, so `sync/service.rs` is untouched. A hub without a contact source answers Hello and Wait with `sync_unavailable`.
  - Reported order (plan silent on ties): private IPv4, IPv4 link-local, ULA, global IPv6, overlay; the routed IPv4 source leads its class; cached 5 s.
  - Resolver `connect_coordinator(manager, pin, identity, discovered)` serves upload, download, and every authenticated exchange (sync included). Candidates: last endpoint, reported, discovered (always empty until step 5), invitation; deduplicated, capped at 12, unauthorized ones skipped. At most 4 dials in flight (below the coordinator's 8 pending per source), new dial every 250 ms or on failure, 3 s TCP timeout, first pinned TLS handshake wins and is recorded as `last_endpoint`. One failure keeps its original message; several are summarized under "could not connect to the coordinator at any known endpoint", which the frontend still classifies as a connectivity failure.
  - Sync state contact is recorded in `sync_request`; failures are logged and never fail the sync.
  - Status `coordinatorEndpoint` and contact card delivery hints use `current_endpoint()` (last endpoint, else invitation endpoint).
  - Notes for step 5: pass discovered endpoints into `connect_coordinator`; the discovery key is on the pin and served by `PairingManager::discovery_key` on the coordinator.
- [x] Step 4: firewall scopes.
  - Date: 2026-10-22. Branch `feat/sync-firewall-scopes`, commit d720bf0f (`feat(handoff): firewall scopes for IPv6, prefixes, overlays, and discovery`).
  - Scope model: `AuthorizedScope { firewall, protocol, interface, destination, source, port, zone }` with `NetworkPrefix` (aligned address and length) for both families. Named `destination`/`source` rather than local network/source network because the UDP destination is the multicast group.
  - State schema version 3 with no shim: a version 2 file is refused with a message to delete it. Earlier-build host-destination rules (`to <address>`, no `proto` order constraint) still parse as /32 or /128 and revoke-all removes them from UFW and every runtime and permanent firewalld zone.
  - Helper validator: TCP only to 43821 or 43822 with a source inside its policy block that covers the destination; UDP only to 5353 and the mDNS group of the source family; source must sit in a private, link-local, unique local, CGNAT overlay, or global IPv6 /64 block and never wider than it; public IPv4, documentation, multicast, loopback, and unspecified sources are rejected; interface and zone names validated; at most 64 scopes per run.
  - Rule shapes: LAN address opens its connected network (route table, else interface prefix, narrowed to the block) to itself; overlay addresses (100.64.0.0/10, or unique local IPv6 on an interface with no default route) admit the whole overlay block to their own prefix on their interface with no discovery rule. IPv6 discovery source is fe80::/10. UFW without `IPV6=yes` gets no IPv6 scope.
  - Discovery (UDP) rule failure is logged and does not fail the grant; status and admission count only TCP listener scopes. Global IPv6 sources accepted as the on-link /64.
  - Network identity stays the interface's IPv4 default gateway hardware address; IPv6 scopes on that interface share it. Interim IPv6 "admit" and "not required" behaviour from step 2 is replaced: admission checks the matching interface address of either family; invitation and Data settings status and grants use every reported interface address.
  - App: `local_addresses::reported_interface_addresses()` and `firewall_interface_addresses()` replace `preferred_ipv4_address`. `MDNS_PORT` is private to `network_access`; step 5 may want to share it.
  - Implementation checks: Tailscale ts-input: primary branch taken (interface rule added), not verifiable on this host. UFW before.rules mDNS: not verified on this host, manual acceptance.
  - Checks passed: ganbaru-handoff lib (80), linking-firewall helper, tauri-app handoff (77), clippy -D warnings on the three crates, fmt, desktop bin check.
- [x] Step 5: discovery.
  - Date: 2026-10-22.
  - Branch: `feat/sync-discovery` (from `feat/sync` at 9b2871c6), not merged.
  - Commits: 58678d42 `feat(handoff): discover the coordinator over DNS-SD with hourly member tags`.
  - Checks (network track target dir): `cargo test -p ganbaru-handoff --lib` (90 pass, including 10 discovery tests with golden HMAC vectors computed independently), `cargo test -p ganbaru-tauri-app --lib vault::handoff` (82 pass, including discovery advertiser schedule, client cache and rate limit, untried endpoint retry, and a live dial that reaches a moved coordinator only through discovery), `cargo test -p ganbaru-linking-firewall --bin ganbaru-linking-firewall` (3 pass; new test asserts mdns-sd and socket-pktinfo are in the app closure and absent from the root helper), clippy `--all-targets -D warnings` on ganbaru-handoff, ganbaru-tauri-app, ganbaru-linking-firewall, `cargo fmt --check`, `cargo check -p ganbaru --bin ganbaru`: all pass. `pnpm -w run audit`: npm no known vulnerabilities; cargo audit 752 crates, only the 10 existing allowed warnings, nothing for mdns-sd, socket-pktinfo, or flume.
  - Implementation checks: `connect_coordinator(manager, pin, identity, discovered, discover)` takes a lazy discovery future that runs only after every candidate failed, then dials only discovered endpoints not already tried and authorized; `dial_coordinator` wraps it for the three call sites with remembered discovered endpoints and the desktop browse (non-desktop stubs return nothing).
  - Decisions where the plan was silent: `advertisement` takes the host name as a parameter (the responder draws it once per app run); the advertiser rechecks linked peers, active vault, and addresses every 60 s, at each hour boundary, and on responder `IpAdd`/`IpDel`; address changes reach it through a `tokio::sync::watch` from one std thread instead of a tokio mpsc; the responder starts lazily and lives for the app run; a browse stops at the first expected match; an empty browse keeps earlier endpoints until their 10 minutes expire; the 60 s rate limit and cache are per coordinator (vault id and fingerprint); a coordinator device that holds a coordinator pin never advertises.
  - Supply-chain review: mdns-sd 0.21.6 exact pin, `default-features = false` (no async, no logging). Single owner keepsimple1, published since 2021, about 6.5M downloads, MIT OR Apache-2.0, `#![forbid(unsafe_code)]`, no build script, no process, filesystem, environment, or HTTP use; 0.21.5 to 0.21.6 diff reviewed (event channel capacity and try_send, multicast join refactor, subscription cleanup), benign. New transitive socket-pktinfo pinned at 0.4.0 in Cargo.lock (0.4.1 needs libc 0.2.189+ and only adds FreeBSD and a cmsg loop rewrite): single owner pixsperdavid, about 3.8M downloads, MIT, no build script, about 620 lines, unsafe limited to IP_PKTINFO setsockopt and recvmsg control message parsing on Unix and WSARecvMsg on Windows; registry source matched an independent download. Other deps (flume without defaults, if-addrs, mio, socket2, fastrand) were already locked. Accepted.
  - Docs: discovery bullet and dial-range wording in `docs/data/security/network-and-privacy.md`, discovery sentences in the `docs/data/sync.md` connectivity paragraph (Android discovery labeled planned), AGENTS.md ganbaru-handoff line.
  - Next step notes: Android NSD (step 7) can reuse `discovery::expected_instance_names` and `match_candidates`; the app `discover_coordinator_endpoints` non-desktop stub is where the Android plugin browse plugs in. Physical acceptance must confirm multicast reaches Windows (firewall per-program rule) and that UFW's UDP 5353 rule lets the browse responses through. mdns-sd resolves every `_ganbaru._tcp` instance on the LAN and filters names afterward (resolving only expected names is an Android NSD property). A first-match browse can return a record cached earlier in the same hour if the coordinator moved mid-hour; the 60 s retry bounds that.
  - `pnpm -w run validate:full` for steps 2 and 5 (2026-10-22, feat/sync at 80e125e5): audit, check, Rust and frontend tests passed; the Android bundle contract failed on two modules from milestone 5 step 2, fixed by raising the ceilings in caba0012 and rechecked.
- [x] Step 6: Tauri-free sync host.
  - Date 2026-10-22, branch `refactor/sync-host` (base ae364e50), commits 88d8a8de `refactor(sync): run sync passes through a host with a headless variant` and bea8a78e `style(sync): keep the headless pass doc to current behavior` (both signed).
  - Check (Earlier step) ownership and active-vault resolution need only the config dir: primary branch held. `ownership::initialize_in` and `active_status_in` and the `vault::*_in` variants read only the config dir; the remaining Tauri-held state (pairing manager with its link probe, pools, key store, person key, emitters) became `SyncHost` methods, and `HeadlessHost::open` rebuilds pairing and ownership from the config dir as app startup does.
  - Check (Runtime) Tauri async runtime before any app exists: not confirmable here (Android JNI is step 7). Desktop evidence for the primary branch: the headless tests call `tauri::async_runtime::spawn` and `spawn_blocking` (through `blocking`) with no Tauri app built, and the pass gate is process-global either way.
  - Check (Runtime) Tauri runtime surviving Activity destruction: not confirmable here. The design covers both branches: the live handle is a `Weak<RuntimeState>`, so it stops upgrading once the managed state drops; `RunEvent::Exit` clears it explicitly; and a registered service whose task is stopped (`task` is `None`) fails the delegation at once instead of being waited on. A stopped service does not make the headless pass run its own pass, because the only in-process stop is a vault transition.
  - Other implementation checks (local network permission, Windows Defender, mDNS sharing, Tailscale, UFW and firewalld mDNS, `onStop`, WorkManager manifest) belong to other steps.
  - Host: `sync/host.rs` defines `trait SyncHost`, implemented by `AppHandle<R>` and by `HeadlessHost` (cfg mobile or test) built from the config dir with FileKeyStore, the person key read from its file, no identity creation or key request, and no-op status and change emitters. Both build engines from one `vault_hooks()`; the app forwards hook notices to `emit_applied`, the headless host drops them.
  - Service: everything in `service.rs`, the client exchange, and `vault_assets.rs` is generic over `SyncHost`. The hub stays desktop-only on `AppHandle`. `devices::read_with` takes the device id and pairing manager.
  - Config-dir variants: `vault.rs` (`ensure_device_id_in`, `active_vault_path_in`, `active_vault_id_in`, `active_database_path_in`, `replicated_asset_path_in`, private app-state helpers), `ownership.rs` (`initialize_in`, `active_status_in`), `db.rs` (`connect_vault_for_sync_in`), `identity.rs` (`person_key_path_in`, `person_key_file_matching`).
  - Pools: `db.rs` holds a process-global `vault_pools()` registry with the vault connection hook installed at construction and a shared `vault_commits()` Notify; both runtimes manage a clone, and the service signals use the shared Notify.
  - Gate: a process-global async `PASS_GATE` held by every pass, by `seal_quiesced`, and by `stop_for_vault_handoff` (which awaits an aborted task before waiting for the gate within the same deadline).
  - Live handle: `setup` registers a `Weak<RuntimeState>`; `RunEvent::Exit` clears it on desktop and mobile. `headless_pass` delegates to a live, running service: it requests `sync_now` and waits up to 60 s for a pass numbered after the request, never running a second pass; a stopped service or a timeout is an error.
  - Headless pass: seals, pushes, pulls, applies, and moves assets through the shared `ServiceState::pass`; the returned `Next::Online` is dropped, so no long poll, no whole-vault refresh, and pools stay open. `more` reports a byte-bounded exchange.
  - Decisions: the headless host refuses to open while an outgoing handoff transfer is recorded (the app reconciles it at startup); mobile re-export and the JNI entry are left to step 7, so `headless_pass` and `HeadlessPass` carry a mobile dead-code allowance until then.
  - Tests (`sync/service/headless_tests.rs`, desktop): convergence of a HeadlessHost from a temp config dir against an in-process hub over the real paired transport with a counting hook on Quick notes; a pass waits while the gate is held; delegation to a live runtime runs no second pass and never opens a host; a non-answering runtime times out without taking the gate; a stopped runtime fails. The transport test harness (`LocalPair`, `TestDirectory`) is now crate-visible, and `vault::activate_test_vault` creates an active test vault.
  - Checks: app lib `sync` 14 ok, `backup` 22 ok, `ownership` 25 ok, `vault` 229 ok with 1 pre-existing failure (`receiver::tests::receive_modes_map_to_the_single_transport_purpose` asserts protocol 7 while base ae364e50 ships protocol 8); replica lib 33 ok; `cargo check -p ganbaru --bin ganbaru` ok; clippy all targets on both crates clean; `cargo fmt --check` ok.
  - Docs: `docs/architecture/native-backend.md` (host abstraction) and `docs/data/sync.md` (pass gate, headless pass, delegation, Android entry Planned).
  - Not compiled here: mobile-gated code (AppHandle FileKeyStore key store, `headless_pass`, mobile run-event exit hook); no Android target is installed.
  - Merge: one conflict in `sync/vault_assets.rs` imports with milestone 5 step 3 (kept the host and the artwork check); the stale protocol 7 assertion in `vault/handoff/receiver.rs` from milestone 2 step 8 was fixed on `feat/sync` before merging.
- [x] Step 7: Android background sync.
  - 2026-10-22, branch `feat/sync-android-background` from `feat/sync` at 80ed6ace; commits 90679347 (`feat(sync): run Android background sync passes through WorkManager`), 5041b210 (`docs(sync): describe Android background sync and its JNI boundary`), and ecbf1df4 (`docs(security): name the background sync entry among unsafe attributes`). The `onStop` and `onDestroy` overrides were checked against Tauri 2.11.2's `Plugin.kt`.
  - New crate `crates/ganbaru-mobile-sync` (Tauri plugin, commands `configure` and `connectivityStatus`, Kotlin plugin, `CoroutineWorker`, NSD browser, scheduling helpers with JVM unit tests); work-runtime pinned to 2.12.0; manifest adds only `ACCESS_NETWORK_STATE`.
  - JNI entry `SyncNative.runPass` in `app/src/sync/headless.rs`: safe body over `jni` 0.21 wrappers, only unsafe attribute `export_name`, bounds 4 KiB config dir, 16 KiB and 16 records (8 addresses each), panics contained; added to the unsafe inventory. The dead-code allowances on `headless_pass` are gone.
  - Implementation checks held: clippy with denied warnings, fmt, focused `sync` tests (28 pass, 12 new in `sync::headless`), `check-android-project.mjs`, `check-unsafe-inventory.mjs`, vitest for `android-work-manager.test.mjs` (6 pass). `pnpm -w run validate` not run per brief.
  - Decisions: a pass repeats up to 4 times while the exchange reports more and was not delegated; offline maps to `unreachable` even when delegated; discovery records reach the dialer only during the pass (Android `remembered_discovered_endpoints` reads them); the foreground Android app does not browse. `linked` is `SyncHost::sync_enabled()`; startup sends a default configuration before `sync::setup`, and later status emits send it from `spawn_blocking` with latest-wins delivery because `run_mobile_plugin` blocks on the Android main thread. Expedited work only on API 31 and newer, since older expedited work needs the removed foreground service. Retry only for one-time work, at most two attempts, linear backoff from 30 s. The worker runs only on Wi-Fi, Ethernet, or VPN transports.
  - Bundle contract: the source manifest must remove `SystemForegroundService` with `tools:node="remove"`, and any merged manifest found under `app/build/intermediates` must not declare it; the validate gate builds no APK, so the merged check bites only after a Gradle build.
  - Physical acceptance (not compiled here, no Android target or SDK): the JNI export and every `target_os = "android"` path, the Kotlin plugin, worker, NSD browser and JVM tests, Gradle build of the new module, manifest merge, and the background sync rows in `docs/testing/android.md`.
  - Follow-ups: the Turbo `bundle-contracts` inputs do not cover `gen/android` or the mobile crates (existing gap, left unchanged pending measurement); iOS builds will see `headless_pass` as unused; status and Settings reporting of the degraded reasons belongs to step 8.
- [x] Step 8: connectivity status.
  - Date: 2026-10-22. Branch `feat/sync-connectivity-status` from `feat/sync` 28faadb1, commit 08e5ba45 (signed).
  - Implementation check: compileSdk and targetSdk are 36, so the fallback branch held: Android detects a local network denial only from a permission error on connect and declares no `ACCESS_LOCAL_NETWORK` permission.
  - `SyncStatusView` gained `connection` (network `lan` or `overlay` plus address) and `degraded` (backgroundRestricted, standbyRestricted, localNetworkDenied, firewallAuthorizationRequired, sorted and unique). The frontend reader validates both and rejects repeated reasons.
  - Connection comes from a process-local coordinator dial report keyed by vault id and certificate fingerprint, recorded by `connect_coordinator`, not from the persisted last endpoint. It survives a failed dial, so the UI shows it only for a client while idle or syncing.
  - LocalNetworkDenied is set when the latest dial reached nothing and any candidate failed with `PermissionDenied`; a successful dial clears it.
  - Firewall reason: on the hub when the Linux listener state is AuthorizationRequired, NetworkConfirmationRequired, or ManualActionRequired (Settings offers "Allow network access" only for the first two, reusing the existing confirmation in VaultHandoffPanel); on a client when its latest failed dial's DNS-SD browse found nothing while UFW or firewalld is active (message only, no client grant flow).
  - Linux firewall probes are cached for 60 seconds; granting or revoking network access clears the cache and runs a pass.
  - Android restrictions are read each pass through the mobile sync plugin on a blocking thread; the battery settings action uses `openMobileBackgroundExecutionSettings("battery")`. Android code paths could not be compiled on this host.
  - The intermediate Syncing publish carries the previous connection and reasons to avoid flicker.
  - Docs: `docs/data/sync.md` Settings and Android delivery section.
- [x] Step 9: docs and automated acceptance.
  - 2026-10-22, branch `docs/sync-milestone-3` from `feat/sync` at 1652c89f; commits bc056a57 `docs(sync): describe connectivity status, Android discovery, and the direct link confidentiality boundary` and ad67d653 `docs(testing): add connectivity and background sync acceptance cases` and 010285ff `docs(sync): name mutual TLS as the confidentiality protection of direct links` (all signed). Docs only; no code changed, so no editor-check or cargo run; `pnpm -w run validate` (this step's plan check) was not run per the task, the orchestrator runs it at milestone close. `check-unsafe-inventory.mjs` passes (23 entries, `runPass` already listed by step 7).
  - Doc audit: already landed by steps 1 to 8 and left alone: sync.md pairing, connectivity, firewall, protocol 8, WorkManager triggers, Android delivery and degraded reasons; network-and-privacy listener and dial bullets; Android permissions (`ACCESS_NETWORK_STATE`, removed WorkManager service, JNI bounds); native-services-and-data WorkManager line; desktop Linux bullet; unsafe inventory; AGENTS.md map lines. Filled: sync.md keys section (mutual TLS is the direct-link confidentiality boundary, record encryption gates only storing intermediaries) and a Connectivity row in the delivery table; network-and-privacy discovery bullet (was "desktop only", now names the Android background NSD browse, no advertising); desktop Windows bullet (dual-stack listener, program rule expected to cover UDP 5353, unverified until physical acceptance); Android README capability table; features README and ROADMAP status sentences; vault-handoff.md "Connectivity cases"; android.md airplane mode and app-opened-during-pass rows.
  - Automated acceptance mapping (Rust tests in `apps/client/src-tauri/app/src/vault/handoff/transport/tests.rs` unless noted):
    - New DHCP address in the same subnet: `coordinator_candidates_put_the_last_working_endpoint_first_and_the_invitation_last`, `a_coordinator_dial_passes_foreign_and_silent_endpoints_and_remembers_the_one_that_answered`, `sync_states_refresh_the_remembered_coordinator_contact`; `sync_states_carry_the_coordinator_contact_from_its_source` (`crates/ganbaru-sync-replica/src/tests/exchange.rs`); `coordinator_contact_writes_only_on_change_and_only_for_the_current_pin` (`crates/ganbaru-handoff/src/pairing/tests.rs`); `lan_addresses_open_their_connected_network_within_their_block` (`crates/ganbaru-handoff/src/network_access.rs`, prefix destination means no new prompt). Physical: real lease change and retry timing.
    - Coordinator moves subnet, phone closed: `discovery_reaches_a_coordinator_that_moved_after_every_known_endpoint_failed`, `discovery_retries_dial_only_endpoints_not_tried_and_authorized`; `client_discovery_browses_at_most_once_a_minute_per_coordinator`, `client_discovery_keeps_found_endpoints_for_ten_minutes_and_per_coordinator` (`app/src/vault/handoff/discovery.rs`); `only_an_unreachable_coordinator_carries_discovery_names`, `provided_records_reach_the_dialer_only_during_the_pass`, `discovery_names_are_the_tags_the_coordinator_advertises` (`app/src/sync/headless.rs`); Kotlin `anUnreachableCoordinatorIsBrowsedOnceWhenItHasNames` (`SyncSchedulingTest.kt`). Physical: NSD on a real network, 5 s budget, status address.
    - Wi-Fi and Ethernet at once: `reported_addresses_rank_lan_before_global_and_overlay_with_the_route_leading_its_class`, `reported_addresses_drop_duplicates_unreportable_routes_and_extra_entries` (`app/src/vault/handoff/local_addresses.rs`); `sync_state_round_trips_reported_endpoints_and_discovery_key` (`crates/ganbaru-handoff/src/protocol/sync.rs`); `coordinator_candidates_put_the_last_working_endpoint_first_and_the_invitation_last`.
    - Tailscale overlay: `private_link_local_and_overlay_addresses_are_lan_peers`, `inbound_peers_follow_the_dial_classes` (`crates/ganbaru-handoff/src/dial.rs`); `overlay_addresses_admit_their_block_on_their_interface_without_discovery` (network_access.rs); `a_connection_names_the_network_class_of_the_endpoint_reached` (`app/src/sync/connectivity.rs`); `maps an overlay connection and every degraded reason` (`src/lib/api/sync.test.ts`). Physical: real Tailscale hosts and the `ts-input` chain.
    - Fresh firewall grant, one prompt, TCP and UDP, IPv4 and IPv6: `one_helper_invocation_carries_every_scope_of_a_flow`, `one_flow_applies_its_scopes_in_one_call_and_a_failed_call_stays_revocable`, `scopes_list_listeners_first_once_each_for_every_active_firewall`, `a_grant_stops_at_a_failed_listener_rule_but_not_at_a_failed_discovery_rule`, `the_validator_accepts_only_listener_and_discovery_rules_from_local_sources` (network_access.rs); `both_families_reach_one_listener_on_one_port`, `a_failed_ipv6_socket_leaves_an_ipv4_listener` (`transport/server/listener.rs`); `the_wildcard_listener_asks_admission_about_the_address_each_peer_reached`. Physical: the pkexec prompt, real UFW and firewalld, phone over ULA.
    - Another device takes the old address: `a_coordinator_dial_passes_foreign_and_silent_endpoints_and_remembers_the_one_that_answered`, `a_single_dial_failure_keeps_its_message_and_several_are_summarized`.
    - Non-member browses two hours: `member_tags_match_the_golden_vectors`, `tags_change_at_each_hour_boundary`, `browsers_accept_one_hour_of_skew_either_way_and_refuse_two`, `random_host_names_are_sixteen_hex_digits_under_local`, `advertisements_carry_the_tag_version_and_reportable_addresses_only` (`crates/ganbaru-handoff/src/discovery.rs`); `the_discovery_advertiser_registers_a_new_tag_at_each_hour_boundary_only` (app discovery.rs). Physical: observed multicast traffic.
    - Phone edits then home screen: `expeditedWorkIsRequestedOnlyWhereItRunsAsAJob`, `passesRunOnlyForLinkedDevicesOnLocalNetworks` (Kotlin); `background_configuration_is_sent_only_when_it_changes`, `deliveries_send_the_latest_state_once_and_retry_after_a_failure` (headless.rs); `headless_pass_converges_with_the_hub_and_runs_hooks` (`app/src/sync/service/headless_tests.rs`). Physical: delivery timing on API 31+ and API 29.
    - Idle overnight, reboot, airplane mode off: `completedPassesFinishAndFailuresRetry`, `onlyOneTimeRunsRetryAndOnlyBeforeTheirLastAttempt`, `registeringANetworkCallbackIsNotAChange`, `aLossBeforeTheFirstObservationIsNotAChange` (Kotlin); `passes_continue_a_bounded_exchange_up_to_the_pass_limit` (headless.rs). Physical: Doze, reboot, WorkManager constraint behavior.
    - Cellular only without Tailscale: `passesRunOnlyForLinkedDevicesOnLocalNetworks` (Kotlin). Physical: real transports.
    - Background restriction: `connection_and_degraded_reasons_reach_the_frontend_in_its_wire_names` (`app/src/sync/status.rs`); `explains each degraded reason with its recovery action` (`src/lib/sync/status.test.ts`); `rejects malformed connections and degraded reasons` (sync.test.ts). Physical: the system probes and the settings intent.
    - App opened during a background pass: `headless_pass_waits_for_the_pass_gate`, `headless_pass_hands_its_work_to_the_live_runtime`, `a_live_runtime_that_does_not_answer_fails_without_a_second_pass`, `a_stopped_live_runtime_fails_the_delegation` (headless_tests.rs). Physical: process-global pool and gate on Android.
    - Unlinked: `passesRunOnlyForLinkedDevicesOnLocalNetworks` (Kotlin), `background_configuration_is_sent_only_when_it_changes` (headless.rs). Physical: WorkManager cancellation.
    - Uninstall on the coordinator: `package_removal_finds_every_app_ufw_rule_shape`, `package_removal_finds_every_app_firewalld_rule_shape` (network_access.rs). Physical: the real prerm and preremove run.
    - Also covered: no WorkManager foreground service (`apps/client/scripts/bundle-contracts/android-work-manager.test.mjs`), helper closure without the responder (`the_root_helper_links_no_mdns_responder`, `crates/ganbaru-linking-firewall/src/main.rs`), protocol bump (`sync_state_refuses_unreportable_endpoints_and_malformed_keys`, `sync_requests_of_the_previous_protocol_ask_for_an_update`), JNI bounds and panics (`invalid_config_directories_fail_without_running_a_pass`, `records_beyond_their_bounds_or_shape_fail_without_running_a_pass`, `panics_and_panicking_payload_destructors_report_failed`).
  - Remain physical (step 10): every phone row, because no Android target or SDK exists on the build host (the JNI export, Kotlin plugin, worker, NSD browser, JVM tests, Gradle build, and merged manifest were never compiled here); the pkexec prompt, real UFW and firewalld behavior, and package removal; Tailscale on real hosts and its `ts-input` chain; Windows Defender coverage of both sockets and UDP 5353; `mdns-sd` coexistence with Avahi and the Windows mDNS service; whether UFW `before.rules` and firewalld zones already accept mDNS; observed non-member advertisements; every timing and battery budget in Performance budgets.
  - Out of scope, reported to the orchestrator: `docs/features/README.md` (edited here for connectivity only; its Quick notes only clause and "other domains remain planned" belong to milestone 4), `docs/ROADMAP.md:36`, and the Quick notes bullet and "Planned network surfaces" in `docs/data/security/network-and-privacy.md` still name Quick notes as the only concurrently synced domain although preferences and themes replicate (milestone 4 doc gap).
  - Goal README status sentence: Milestone 3 (connectivity and Android background sync) is implemented in source on `feat/sync`; its Android paths were not compiled on the build host, and physical acceptance (step 10) is pending.
- [ ] Step 10: physical acceptance.
