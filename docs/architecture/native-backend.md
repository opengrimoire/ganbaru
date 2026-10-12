# Native backend architecture

Rust owns operations that require durable storage, native authority, bounded filesystem access, provider processes, media playback, or operating-system integration.

## Tauri composition

`apps/client/src-tauri/` is the Tauri package. Its desktop `main.rs` and mobile `lib.rs` delegate to the ordinary Rust library in `apps/client/src-tauri/app/` (`ganbaru-tauri-app`).

The application library has separate desktop and mobile composition roots (`runtime/desktop.rs` and `runtime/mobile.rs`). Target-scoped dependencies keep desktop-only services out of mobile builds. Tauri commands are adapters: they validate transport values, resolve managed state, enforce vault or platform authority, and call focused services.

## Domain crates

The Cargo workspace extracts domains that benefit from Tauri-free contracts and tests:

- `ganbaru-db` owns the SQLite pool registry, connection configuration, embedded migrations, and schema tests.
- `ganbaru-civil-time` converts between instants and zoned wall times for Calendar, Focus, Music, and the distraction blocker. Zones are always explicit, explicit times take the earlier fold and the pre-transition offset in a gap, and generated recurrence times skip nonexistent wall times.
- `ganbaru-pomodoro` owns focus persistence, history, validation, recovery, local activity admission, transactional execution, and adaptive run and phase decisions.
- `ganbaru-quick-notes` owns Quick notes persistence, search windows, revisioned writes, lifecycle and trash retention, ordering, tags, and resolution of replicated conflicts.
- `ganbaru-music` owns Music context assignments per Focus phase for projects, Calendar events, and work environments, and the Music library error shape.
- `ganbaru-music-library` owns Music library persistence: items and sources, playlists and memberships, review and snoozes, search, listening history, transfer and interchange, soundscape definitions and state, desktop local media refresh, relink, repair, and artwork discovery, and the deterministic Music session policy with queue loading and checkpoints. The app supplies device-local root bindings and keeps Android storage refresh, Android and browser playback backends, and the native Music session runtime.
- `ganbaru-music-player` owns desktop local playback: media file inspection and the decoding worker that executes committed Music session effects under the delivery authority the app supplies. It is a desktop-only dependency; Android inspects media through the platform plugin.
- `ganbaru-projects` owns Projects persistence: groups, projects with their managed working folders, structure, tasks, history, custom fields, relationships, reviewed dependency cascades, reordering, workspace reads, and the task rows Calendar scheduling reads and writes. Icon asset files stay in the app.
- `ganbaru-calendar` owns Calendar persistence, iCalendar import and export, native window reads, scoped edit preview and Save preparation, deletion and Undo, and recurrence expansion. The app injects the platform device date, vault identity, and Focus owner clock, and the Focus owner serializes Calendar Save with Focus publication.
- `ganbaru-distractions` owns the distraction blocker wire contracts, desktop application name rules and protected applications, usage limit budgets and bounded usage reads, usage sample and block event normalization, and evidence-bounded elapsed accounting. The app keeps observation, foreground and process control, close authorization, state files, the device-local usage spool, and the Android Guardian runtime.
- `ganbaru-themes` owns user theme persistence: current and seed tokens and event palettes, seed resets, upgrade dismissals, and validation of rows written and read. Built-in themes stay code-pinned in the frontend.
- `ganbaru-notes` owns the Notes graph, persistence, transfers, history, assets, and bounded file operations.
- `ganbaru-chat-contracts` defines provider-neutral identifiers, commands, events, read models, and errors.
- `ganbaru-chat-providers` owns provider processes, transports, normalization, cancellation, and registry behavior.
- `ganbaru-chat` owns Chat persistence, runtime services, Git workspaces, checkpoints, review, and source control.
- `ganbaru-working-folders` owns portable folder identities, bindings, repository kinds, and device-local state shapes.
- `ganbaru-contacts` owns the person key pair, the signed contact card format, contact request signatures, and trust kinds. The app keeps private key storage, contact and request persistence, and request delivery over the handoff transport.
- `ganbaru-handoff` owns the LAN wire protocol between linked devices and the desktop coordinator (bounded control messages and their validation, protocol and schema compatibility, invitation and contact QR encoding, and staged bundle digest checks) and the device-private pairing state: the TLS identity, coordinator pins and peer membership, revocation, and transfer progress and staging paths. On Linux it also owns the explicit firewall authorization for the coordinator port and the privileged helper that applies it. The app keeps the pinned TLS transport, the coordinator, and handoff orchestration.
- `ganbaru-sync-contracts` defines sync identifiers, clocks, version vectors, writer certificates, order keys, bounds, and the signed operation format. It has no database or Tauri dependency.
- `ganbaru-sync` is the Tauri-free sync engine: the replication manifest, rendered capture and guard triggers, sealing, storage, causal apply, merges, conflicts, recovery, and re-sealing, plus the Quick notes domain adapter and the deterministic simulation harness.
- `ganbaru-sync-replica` composes the engine with one vault replica: guarded database access that signals commits, the installation writer with its keys and device-local record, carry-forward of local operations across whole-vault replacement, recovery offer actions, and the hub and client sides of the LAN exchange over the handoff protocol behind a transport trait. It provides an operating-system keyring store for desktop and a private file store, and has no Tauri dependency.
- `ganbaru-native-messaging` is the independent Chromium native messaging host.
- `ganbaru-mobile-*` crates expose narrow Android notification, document, media, and anti-distraction plugins.

The application's sync module owns the per-vault service lifecycle around handoff quiescence, the paired transport that carries client requests to the coordinator, the writer key store choice per platform, and the status, pause, sync now, and recovery commands. Domain modules keep their own sync-facing commands, such as Quick notes conflict resolution. The engine never touches Tauri state, key stores, or the network, so its merge behavior is testable with in-memory databases. See the [sync engine decision](decisions/sync-engine.md) and [Device linking and synchronization](../data/sync.md).

Code stays in the application crate when it is composition, Tauri adaptation, or bound to Tauri-managed platform services. Tauri-free domain logic belongs in a core crate, which keeps its tests independent and bounds the application crate's compiler peak.

## SQLite

The active vault SQLite database is the source of truth for structured data. SQLx embeds timestamped migrations from `crates/ganbaru-db/migrations/`.

The pool registry serializes opening, migration, and closing, so concurrent startup commands share one initialized pool instead of racing to open competing pools. Each pool keeps one SQLite connection with WAL and full synchronization.

There is no generic query bridge to the frontend. Domain services own statements, transactions, validation, and result shapes. Cross-table changes that represent one user action commit atomically where partial success would violate the product contract. Exact columns and indexes belong to migrations; the [schema guide](../data/schema/README.md) documents domain ownership and relationships.

## Filesystem authority

The active vault and explicitly authorized working folders are separate roots. Every command resolves identifiers through the owning service, canonicalizes paths where needed, rejects traversal and unsafe symbolic-link escapes, and bounds bytes, depth, count, or time for user-controlled input.

Managed assets use validated relative paths beneath the vault. External working files remain externally owned. Device-local absolute paths and provider configuration stay outside synchronized vault records.

Vault selection, snapshots, backup, restore, and handoff are serialized: native owners drain and the managed-write and SQLite gates are taken before the active vault changes. Local handoff keeps one pairing state owner with one persisted schema; its submodules are implementation boundaries, not independent stores. The ownership generation and write-fence rules, and the backup archive-to-activation pipeline, are deliberately kept in single modules next to their tests because their correctness depends on reading the coupled rules together. Behavior and acceptance are in [Device linking and synchronization](../data/sync.md) and [Data architecture](../data/architecture.md).

## Asynchronous and blocking work

Tauri asynchronous command executors must not perform unbounded synchronous work. SQLx operations stay on their asynchronous path. Filesystem traversal, process observation, blocking platform APIs, image and media probing, and archive generation run on blocking workers when their cost depends on user input or external state. Examples include Music folder scans, anti-distraction application discovery and process observation, project icon fetching, and Notes import and export.

Worker code must not hold a SQLite transaction or a shared mutex while awaiting a blocking task. Cancellation returns no partial authoritative result, atomic writers publish only complete output, and a replaced request cannot publish results after a newer generation has become authoritative.

## Native runtime owners

Several runtime domains have exactly one native owner per vault. The WebView presents state and sends semantic commands; it never holds execution authority. This keeps behavior correct when a window is closed, reloaded, or throttled, and lets vault changes revoke stale work through one ownership generation.

- **Calendar:** Rust expands recurrence in each event's home zone (using Jiff for timezone facts) and serves bounded windows to the UI, the Pomodoro scheduler, and Android reminders. Edits, creation, Project scheduling, deletion, and Undo go through a semantic native boundary. Preview prepares the same rows as Save, and Save commits Calendar writes with Focus reconciliation and a retry receipt in one transaction. See [recurrence editing](../features/calendar/recurrence-editing.md) and [deletion and Undo](../features/calendar/deletion-and-undo.md).
- **Focus:** the `ganbaru-pomodoro` native owner accepts semantic commands, commits transitions, and publishes snapshots. Recovery uses only committed canonical execution state. Overlays, warnings, sounds, Android notifications, and blocking phases follow accepted state, never a frontend timer. See [Pomodoro](../features/pomodoro/README.md).
- **Music:** one native owner serializes queue intent and playback effects. Automatic background intent cannot authorize playback after restart without fresh admission, and native audio does not depend on the WebView presentation stream. See [Music playback](../features/music/playback.md).
- **Distraction blocker (desktop):** one native owner observes usage, persists interval evidence to a device-local spool before deriving budgets, and makes block and close decisions only from a fresh snapshot of persisted configuration for the current local date. The UI reads a projection and cannot submit usage or request closes. See [Distraction blocker](../features/distractions/README.md).
- **Distraction blocker (Android):** the Guardian service keeps its own operating-system observation and enforcement. A serialized Rust publisher synchronizes its usage journal, derives budgets, and publishes rules; presentation can only supply localized copy. Guardian keeps the last accepted rules when the application process is absent.

Configuration changes, vault selection, handoff, and shutdown revoke obsolete generations so stale work cannot publish effects.

## Other native services

Desktop composition includes the tray, updater, detached windows, native notifications, process control, browser native messaging, Rodio and Symphonia audio, MPRIS or Windows media controls, provider processes, terminals, Git workspaces, and browser previews. Platform media-control protocol state stays inside its target-gated adapter.

Android composition uses plugin adapters and system-owned surfaces instead of desktop implementations. Notification schedules, selected document grants, Media3 playback, and anti-distraction access remain subordinate to canonical vault data.

The browser native messaging host evaluates rules into typed decisions. Extension labels and stored event classifications derive from the same decision rather than from parsing display text.

## Error and security boundary

Native commands return bounded, user-meaningful errors without secrets, full remote bodies, unnecessary home paths, or unchecked provider data. Secrets are stored only through operating-system credential references.

Provider output, external files, archives, browser messages, and mobile plugin responses are untrusted input and are validated before persistence or frontend projection. See [Security](../data/security/README.md).
