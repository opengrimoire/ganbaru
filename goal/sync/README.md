# Concurrent sync goal

**Status: Partial.** [Milestone 1](milestone_1.md) (engine, operation format, LAN hub, Quick notes) is implemented; only its physical acceptance (step 8) is pending. Milestones 2 (engine generalization) and 3 (connectivity and Android background sync) are implemented in source on `feat/sync` with physical acceptance pending; milestone 4 (preferences, themes, Contacts) is implemented in source with physical acceptance pending; milestone 5 (Music) is implemented in source with physical acceptance pending; milestone 6 (Projects) is implemented in source with physical acceptance pending; milestone 7 is implemented in source with physical acceptance pending; milestone 8 has steps 1 to 6 done, milestone 9 steps 1 to 6, milestone 10 steps 1 to 4, and milestone 11 steps 1 to 8; the rest of milestones 5 to 12 is in progress or planned, each with its own plan file. This folder is the working plan for replacing single-writer whole-vault handoff with concurrent, offline-first sync between one person's devices, shaped so that the [multi-person sharing goal](../sharing/README.md) is a configuration of the same engine.

Handoff: a new orchestrator starts at [handoff/HANDOFF.md](handoff/HANDOFF.md), which holds the current state, in-flight branches, procedures, and user decisions.

The contract lives in `docs/` and is not repeated here: [Device linking and synchronization](../../docs/data/sync.md) (contract and target), [Sync engine decisions](../../docs/architecture/decisions/sync-engine.md), [Sync topology and authority decisions](../../docs/architecture/decisions/sync-topology-and-authority.md), [Sync merge rules](../../docs/algorithms/sync/README.md), and [Sync schema](../../docs/data/schema/sync.md).

## End state

- Every linked device edits all portable vault data offline: preferences, themes, Contacts, Quick notes, Music library, Projects, Calendar, Focus history, distraction rules and usage, Notes, Chat organization, and managed assets. Changes reach the other devices automatically and converge with no silent loss.
- Device-bound state (execution, receipts, presentation, paths, credentials) stays on its device, inside the vault database where it needs transactional atomicity.
- Whole-vault handoff, the vault owner, and read-only replicas are gone. A snapshot copy remains only as bootstrap for a new or long-dormant device.
- Devices find each other on the LAN, follow address changes, and connect over any private or overlay network (for example Tailscale). Android syncs in the background.
- Authority is anchored (admin key, removal cutoffs, late changes), and every member serves its peers, so a switched-off coordinator does not stop sync.
- An optional self-hosted relay carries end-to-end encrypted records across networks; an optional always-on anchor is a headless member that holds keys and keeps sync available.
- Storage stays bounded: winner values are stored once (the version table keeps only hashes for winners), and operation bodies are compacted after causal stability.

## Decisions for this plan

These are new or changed since the docs were written. Rows marked with a milestone differ from current `docs/` until that milestone lands and edits the named doc; do not change code back toward the old text.

| Decision | Reason | Docs differ until |
| --- | --- | --- |
| A `Local` table and column class inside the vault database. Never captured, never replicated, writable on guarded replicas; each Local column declares the value materialize writes for new rows. Snapshot copies empty Local tables and reset Local columns to those values; a refresh or handoff replacement keeps the receiver's Local rows, copied from its live database. | Device state such as Focus execution and Chat processes must commit atomically with domain writes, and SQLite transactions across attached WAL databases are not atomic. Rollback copies are pruned and can fail to retain, so they cannot be the restore source. | M2 (`docs/data/sync.md`, `docs/data/schema/sync.md`) |
| Every foreign key into a replicated table is `CASCADE` or `SET NULL`, including between replicated tables; `NO ACTION` foreign keys are rebuilt by the milestone that converts their table, and commands keep "in use" refusals as explicit queries. Replicated tables never reference Local or Unconverted tables. Reference cycles are allowed; a cycle's tables materialize under deferred foreign keys. | A remote delete must never fail apply, and the Calendar, iCalendar, Projects, and Notes graphs contain cycles. | M2 |
| Guard triggers skip writes made while the engine applies (`sync_apply_state.applying = 1`). Every other non-engine trigger on a converted or Local table declares a policy (`SameRow`, `Gated`, `ApplierVisible`, or `DerivedIndex`) checked by conformance; triggers that write other non-derived tables are gated or moved to Rust by the converting milestone. | A replicated parent delete must be able to cascade into unconverted children on a replica; ungated cross-table triggers would create rows during apply. | M2 |
| All replicas of a vault descend from one database. Built-in seed rows therefore exist identically everywhere and need no merge rule; per-item copies made from seeds (for example a theme's seed snapshot) are ordinary user data. Merging two independently created vaults stays an explicit import. | Removes every "deterministic seed id" special case. | None |
| Rows whose key derives from content (membership pairs, imported iCalendar UIDs, default channels) declare a derived key and an existence register instead of tombstones, so remove then add again works. A table with a single text `id` uses it as the row key. A replicated table with a composite primary key gains `sync_key TEXT NOT NULL UNIQUE`, filled in Rust by one helper in `ganbaru-sync-contracts` (`{prefix}:` plus the first 32 hex digits of SHA-256 over the NUL-joined parts); the converting milestone fills existing rows in its Rust conversion pass before capture seeding. | Published tombstoned keys are never reused, which breaks deterministic keys. SQLite triggers cannot hash, and plain joined parts exceed the 128-byte row key bound. | M2 (`docs/algorithms/sync/README.md`) |
| No Counter kind. Usage and statistics are per-device rows keyed by device and bucket, merged with `Max`. | Each row has one writer, so existing kinds suffice. | M2 |
| Trees are a parent `Reference` plus `Position`, with deterministic cycle repair: in a cycle, the node whose winning parent version has the greatest `(clock, writer)` moves to the root, and a sealing replica writes the repair, like duplicate repair. | Gives the same guarantees as a move log for one person's devices with far less machinery. | M2 (`docs/data/sync.md` conflict semantics) |
| A tombstoned parent hides its replicated `CASCADE` children deterministically. Sealing replicas write repair tombstones that name the cause, and children's recovery entries are evaluated against the cause's context. | Same pattern as unique repair; concurrent edits inside a deleted subtree stay recoverable. | M2 |
| Winner values are elided from `sync_register_versions` and read from the domain row, falling back to the stored operation envelope (checked against the value hash) while a capture is pending or the row disagrees. Every codec must round-trip byte-exactly (a conformance test). A tombstone, a fork re-seal, and compaction hydrate affected versions first. Sealing skips groups whose value equals the current winner. | Removes the second copy of every replicated value before domains add volume; whole-row rewrites in Calendar and Notes would otherwise seal unchanged groups. | M2 (`docs/data/schema/sync.md`) |
| The engine track owns every engine shape the domains need, delivered in milestone 2: composite row keys, nested owned tables, coupled groups containing owned tables and references, parent publication, Tree depth limits and acyclic edge sets, hook-maintained derived columns, a per-domain replica write flag, and streamed assets up to 64 MiB. Domain milestones never edit shared engine files; a missing shape found later goes back to the engine track as its own step. | Several domain agents editing `apply.rs` and `materialize.rs` in parallel would produce divergent versions of the same feature. | None |
| Notes block text and Quick notes bodies stay surfaced registers. Character-level merging with Yrs moves to the sharing goal. Block text is its own field group keyed by a stable block id, so a later Text kind replaces one group. | One person rarely edits the same block on two devices at once, a surfaced conflict loses nothing, and Yrs needs a multi-week editor rewrite plus a dependency. | M8 (`docs/data/sync.md` Notes editing) |
| Discovery is DNS-SD over mDNS (`_ganbaru._tcp`): the `mdns-sd` crate on desktop, `NsdManager` on Android. Records carry a member tag, an HMAC of the certificate fingerprint and the hour under a random discovery key. The key travels in `SyncState` over mutual TLS (the pairing reply is only server-pinned), so existing links receive it without relinking; milestone 11 rotates it through removal entries. Identity stays the pinned certificate. | Native on Android without a multicast lock, reusable by peer exchange, and records reveal nothing stable to non-members. | M3 |
| Direct connections are allowed over any private or overlay range: RFC 1918, link-local, `100.64.0.0/10`, IPv6 ULA and link-local, and on-link global IPv6 inside the device's own prefix (already relied on by contact delivery), for both dialing and accepting. Other public addresses stay refused. Mutual TLS is the confidentiality boundary for direct links; record encryption gates only intermediaries that store records. | Tailscale and similar overlays give cross-network sync with no project infrastructure and no new crypto. | M3 (`docs/data/security/network-and-privacy.md`, `docs/data/sync.md` keys) |
| Android syncs through WorkManager only: periodic, on network change, and after backgrounding with pending changes (expedited on API 31 and newer, plain constrained work below, since expedited work on API 29 and 30 needs a foreground service). No foreground service for sync. | A persistent service costs battery and permissions for no correctness gain; live Focus companion status is Focus feature work, outside this goal. | M3 (`docs/data/sync.md` Android delivery) |
| Focus runs replicate once closed, through a monotonic publication predicate. The open run is Local. A replicated per-device activity row shows what each device is focusing on. Remote control is out of scope. | Keeps the single-open-run invariant per device without a controller ownership protocol. | M7 (`docs/data/sync.md` storage ownership) |
| Managed project folder contents are device-local working copies after handoff retires. Bootstrap copies them once; afterwards Git remotes or a user's folder sync tool move files. Working folder bindings replicate with Projects. | A general file tree sync engine is out of proportion; Chat and checkpoints already rely on Git. | M10 |
| Bootstrap is a scrubbed whole-database snapshot from any serving member, then operation exchange closes any gap. No typed checkpoint bootstrap. | Reuses the proven handoff path; the snapshot already contains engine state. | M10 (`docs/data/sync.md` bootstrap) |
| Operation bodies are dropped after causal stability. Stable implies final because the anchor's acknowledgement is part of stability. A writer silent for 90 days is dormant: it leaves the stability computation and must re-bootstrap with carry-forward. Tombstones are kept forever. Kind `3` becomes a signed acknowledgement carrying the applied vector. | Makes compaction actually run without conflicting with removal cutoffs. | M10 |
| No process other than the app writes the vault. The native messaging host writes the spool only. | Capture, guards, and the single-connection invariant assume one writer process. | M7 |
| `config.json` is emptied in steps: milestone 4 moves portable preferences to a replicated table and device preferences to the app config directory, milestone 7 moves the distraction section, and milestone 9 moves the Chat section. Whichever of 7 and 9 lands last deletes the file and its loader. | Each section moves with the domain that owns its rules. | M4, M7, M9 |
| `contacts_local_identity` becomes an Engine table, immutable after creation; it anchors the personal space and never takes part in a merge. | The space context is read from it; a merge would change who owns the space. | M4 |
| Operation kinds: `0` changes, `1` genesis, `3` signed acknowledgement (milestone 10), `4` authorization entry sealed in the anchor's chain (milestone 11). Kind `2` (revoke) is removed by milestone 11 and its number stays unassigned. Authorization entries carry an entry kind; unknown entry kinds are stored, forwarded, and kept in the chain, and contribute nothing to derived authorization state until understood, so milestone 12 adds key and server entries without a format change. | Removal cutoffs move into the authorization log; one extensible entry type avoids a kind per authority fact. | M10, M11 |
| Domain adapters stay in `ganbaru-sync` (`src/domains/`, `src/manifest/vault/`); the engine never depends on domain crates. | The simulation harness merges real values, and engine builds stay light. | None |

## Milestones

| Milestone | Outcome | Needs |
| --- | --- | --- |
| [1](milestone_1.md) | Engine, format, writers, mixed mode, LAN hub, Quick notes | Done |
| [2](milestone_2.md) | Engine generalized for every domain: adapter registry, data-driven conformance, Local class, derived keys, tree and cascade repair, dependency-ordered apply, generic owned children, publication predicates, deletion holds for Undo, post-apply hooks for derived indexes, assets, conflict and recovery presentation, winner elision, multi-domain simulation | 1 |
| [3](milestone_3.md) | Connectivity: discovery, address refresh, overlay networks, firewall updates, Android background sync | 1 (step 6: 2 step 7) |
| [4](milestone_4.md) | Preferences split out of `config.json`, themes, Contacts | 2 |
| [5](milestone_5.md) | Music library | 2 |
| [6](milestone_6.md) | Projects core and working folder bindings | 2, 5 |
| [7](milestone_7.md) | Calendar and iCalendar, Focus history, distraction rules and usage | 6 (distraction steps: 2) |
| [8](milestone_8.md) | Notes | 6 |
| [9](milestone_9.md) | Chat organization | 4, 6, 7 step 4 |
| [10](milestone_10.md) | Handoff retired: ownership and guards removed, snapshot bootstrap, acknowledgements, compaction, dormant writers, cross-process fencing | 2 (steps 5 to 7: 4 to 9 and 11 step 9) |
| [11](milestone_11.md) | Anchored authority, admin key, removal cutoffs and late changes, quarantine, LAN peer exchange, Android Keystore | 2, 3, 10 steps 1 to 4 |
| [12](milestone_12.md) | Space keys and record encryption, encrypted backups, self-hosted relay and always-on anchor | 11 (step 8: 10) |

Tracks for parallel agents: the engine track runs 2, then milestone 10 steps 1 to 4, then 11, then 12 and the rest of 10 (milestone 10 steps 5 to 7 wait for 4 to 9 and milestone 11 step 9; milestone 12 step 8 waits for all of 10); the network track runs 3 beside 2; domain agents run 4 and 5 after 2, then 6, then 7, 8, and 9 in parallel. Only one agent at a time edits shared engine files outside its domain module (`apply.rs`, `merge.rs`, `materialize.rs`, `seal.rs`, `triggers.rs`); a domain milestone that finds a missing engine shape stops that step and asks the engine track for it as its own step.

## Parallel work reservations

| Resource | Rule |
| --- | --- |
| `TableId` ranges | Quick notes 1 to 15, preferences and themes 16 to 31, Contacts 32 to 47, Music 48 to 95, Projects and working folders 96 to 159, Calendar and iCalendar 160 to 223, Focus 224 to 271, distractions 272 to 287, Notes 288 to 351, Chat 352 to 447, future domains from 448. Test fixture domains use 60000 and above. Field ids are per table. |
| Manifest files | Each domain owns `src/manifest/vault/<domain>.rs` and `src/domains/<domain>.rs` and adds one line to the table list. `VAULT_MANIFEST_VERSION` increments by one per step that changes the manifest; when a worktree merge collides, take the next number. |
| Wire versions | Plans never fix a number for the handoff `PROTOCOL_VERSION`, `FORMAT_VERSION`, or `RELAY_PROTOCOL_VERSION`. Each step that changes the wire takes the next free number when it lands on `feat/sync`. |
| Migrations | Name with the UTC time of writing and never rename after it lands. SQLx applies unapplied migrations regardless of order, so parallel milestones only need disjoint tables. Engine tables (`sync_*`) belong to the engine track. |
| Localization | Domain conflict and recovery strings go in the domain's catalog (`apps/client/src/lib/i18n/messages/{en,es}/<domain>`). `sync.ts` belongs to milestones 2, 3, 10, 11, and 12. |
| `AGENTS.md` map | Milestone 3 extends the `ganbaru-mobile-*/` line with background sync (`crates/ganbaru-mobile-sync/`); milestone 12 adds `crates/ganbaru-sync-relay/`. Domain milestones edit only their own map line. |
| Owning docs | Each milestone edits the `docs/data/sync.md` rows and sections it changes, plus its domain's schema and feature docs. `docs/features/README.md` status rows are edited by the milestone that changes them. |

## Exit criteria for every milestone

- The simulation harness covers every merge kind and domain converted so far and passes its seed set.
- Converted tables pass data-driven conformance, crash-boundary, and codec round-trip tests.
- A physical acceptance checklist on two desktops and one Android device for what the milestone changed, written in its plan. The maintainer runs it and records the results; a pending checklist does not block later milestones.
- The owning docs describe the new current state, including every "Docs differ" row above that names the milestone.
- `pnpm -w run validate` passes; dependency changes also pass `pnpm -w run audit`.

## Working agreement

- All work lands as signed commits on `feat/sync`, one or more per plan step, with no pull requests until every milestone is done; then one pull request takes `feat/sync` into `dev`. Commits are never amended or rewritten.
- A step that runs in parallel works in its own worktree on the branch its plan's "Worktree branch" column names, cut from `feat/sync`, and merges back locally once its checks pass. At most two implementing agents run at once, only on tracks this README marks parallel.
- Each step runs its focused checks and updates the docs its contract changes before it is committed. `pnpm -w run validate` runs at the end of each milestone, `validate:full` after dependency changes, and one fresh agent reviews the milestone's diff against its plan before the milestone closes.
- When a milestone starts, re-check its "Current state" section against source, because earlier milestones change the code later plans describe, and update the plan before coding. Implementation checks record which branch held.
- The Progress section of each plan is the durable state and is updated with every step.
- Durable decisions move into `docs/` as they land. This folder keeps the plan and progress, not a second contract.
- `goal/` is local and ignored by Git. Nothing in `docs/`, `AGENTS.md`, code, commits, or pull requests links to it.

## Resuming work

1. Read the Progress sections and continue from the first unfinished step whose "Needs" are done, one agent per track, starting from its plan's "Current state" and "Implementation checks" sections (milestone 1 keeps its original "Verify during implementation" heading).
2. Milestone 1's physical acceptance ([step 8](milestone_1.md#progress)) runs whenever the maintainer has the devices; it does not block later milestones.
3. When milestone 11 lands, write the first plan of the [multi-person sharing goal](../sharing/README.md).
