# Milestone 4: preferences, themes, and Contacts

**Status: Partial.** Implemented in source on `feat/sync`; physical acceptance (step 5) is pending. Part of the [concurrent sync goal](README.md). This file is the implementation plan for splitting portable preferences out of `config.json` into a replicated table, moving device-local preferences into the device's app config directory, and converting user themes and Contacts to concurrent replication. [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract; the domain contracts are [Themes](../../docs/features/themes/README.md), [Profile](../../docs/features/profile.md), [Localization](../../docs/features/localization.md), [Contacts and invitations](../../docs/features/collaboration/README.md), and [Supporting domains](../../docs/data/schema/supporting-domains.md). The progress checklist is at the end.

## Outcome

When this milestone is done:

- Profile, language, time format, rhythm defaults, favorites, sidebar orders, and the other portable preferences are editable on every linked device, offline or online, and a change appears in every open window of every device within the normal sync delay.
- Font family, font scale, layout state, title bar visibility, notification delivery toggles, and the active theme stay on the device that set them and are never sent to another device.
- User themes are created, edited, renamed, reset, deleted, and dismissed from upgrade prompts on any device. Two devices saving the same theme concurrently show a conflict instead of silently dropping one editing session.
- Contacts, contact requests, and contact card rotation work from any linked device. Received requests still arrive only at the coordinator, and a block always beats a concurrent accept.
- `config.json` holds only the `distractions` and `chat` branches.

## Scope

In:

- A typed native preference registry that classifies every preference key as portable or device-local and validates every value.
- Table `vault_preferences` (table 16), one row per portable preference, and the per-vault device preference file in the app config directory.
- First-open import from `config.json`, a merged native preference view for native readers, and frontend refresh on remote changes.
- Themes (table 17) with their owned token and palette rows, random UUID ids for new themes, and upgrade dismissals folded into the theme row.
- Contacts (table 32), contact requests (table 33), and the contact card state (table 34); the person identity row as an Engine table.
- Profile picture bytes through the managed asset support from [milestone 2](milestone_2.md).
- Docs, the `AGENTS.md` folder structure line for `config.json`, and physical acceptance.

Out:

- The `distractions` branch of `config.json` and its owner-only rules; [milestone 7](milestone_7.md) moves it.
- The `chat` branch of `config.json`; it stays native-owned there until [milestone 9](milestone_9.md). Whichever of milestones 7 and 9 merges last deletes `config.json` and its loader.
- Set-valued merging for list preferences (favorites, stars, orders); see [Deferred](#deferred).
- Request receipt or delivery from devices other than the coordinator. Receipt stays on the coordinator ([milestone 11](milestone_11.md)'s anchor) through this goal, and [milestone 12](milestone_12.md)'s always-on anchor does not serve it; receipt by any device belongs to the [multi-person sharing goal](../sharing/README.md).
- Person key rotation and Android Keystore wrapping.
- Any engine change: everything here uses features milestone 2 provides.

## Prerequisites

- [Milestone 2](milestone_2.md) is merged: adapter registry and data-driven conformance, the per-domain replica write flag and declared materialization values for `Local` columns (step 1), derived keys with an existence register, owned children with custom codecs in Immutable, Register, and Coupled groups beside parent columns (step 7), Coupled priorities from the adapter, post-apply hooks that receive the connection and a change sink (step 7), capture seeding at vault open and batched sealing (step 6), managed assets through `AssetClass { name, max_bytes }` with engine-owned `{sha256}.{ext}` file names and `asset_refs` in any group value (step 8), unchanged-group elision at seal (step 10), winner elision, guards that skip while `applying = 1`, and the generic conflict and recovery presentation.
- Every replicated table here has a single text key (`key`, `id`, or `public_key`), so none needs the composite `sync_key` convention; the theme token and palette tables have composite primary keys but travel as owned children inside their parent's group value.
- No other prerequisite. [Milestone 3](milestone_3.md) is independent.

## Current state

Verified against `feat/general-tweaks` at `e2bdcca1` on 2026-10-21; re-verify before starting.

- `config.json` writes go through `mutate_active_vault_config` (`apps/client/src-tauri/app/src/vault/config.rs:70`), which requires `active_writable_vault_path` (line 75), so only the owner writes preferences. A `distractions` change commits the blocker runtime (lines 106 and 112) and a `preferences` change invalidates the Focus calendar (line 121). `chat` is a native-owned branch the frontend cannot patch (lines 17 and 139).
- The frontend reads and writes dotted keys through `getConfigKey` and `setConfigKey` (`apps/client/src/lib/vault/config.ts:160,170`), debounced into `vault_patch_config`, and relays changes between windows through the `vault-config-changed` channel (line 39) and `onConfigChangedElsewhere` (line 237).
- Native readers parse `config.json` directly: the contact card profile (`contacts/card.rs:44`), the profile image path (`profile_images.rs:280`), Focus presentation preferences (`pomodoro/native_runtime/presentation.rs:325`), and the Music session (`music/session/runtime.rs:1583`).
- Key inventory by owning module: `stores/preferences.svelte.ts:81-126` (profile, appearance, Calendar, Focus, Music, Notes, sidebar orders, Contacts trust defaults, title bar), `stores/theme.svelte.ts:67-69` (active and quick-toggle theme ids), `stores/updates.svelte.ts:32-33`, `projects/ui-preferences.ts:4-10`, `notes/pages/navigation.ts:3-7`, `notes/pages/selection.ts:4`, `notes/navigation/tree.ts:19`, `notes/database/view-tab-display.svelte.ts:8`, `stores/notes/archive.svelte.ts:8`, `stores/chat/sidebar-order.svelte.ts:17-19`, `stores/chat/channel-navigation-controller.svelte.ts:10`, `i18n/translator.svelte.ts:18`, `components/icon-picker/IconPicker.svelte:161-163`, `components/notes/pages/NotesPageCoverMenu.svelte:54-55`, `components/music/MusicPlaylistBuilderPage.svelte:137-151` (`music.sources.*`, `music.review.autoplay`), and `stores/distractions.svelte.ts:32`.
- Themes live in `themes`, `theme_tokens`, `theme_event_palette`, `theme_seed_tokens`, `theme_seed_event_palette`, and `theme_upgrade_dismissals` (`crates/ganbaru-db/migrations/20261004220000_baseline_schema.sql:3789-3843`). Seeds are written at creation by `cloneTheme` (`apps/client/src/lib/themes/operations.ts:96-105`) and import, and `replace_content` (`crates/ganbaru-themes/src/lib.rs:300`) rewrites them with every save. `record_dismissal` uses `INSERT OR REPLACE` (line 399) and the store keeps only the highest dismissed engine version per theme.
- New theme ids are `theme-` plus six base36 characters from `Math.random` (`apps/client/src/lib/themes/serialization.ts:50-56`), checked only against local ids. Import rejects an id that already exists (`docs/features/themes/README.md:69`).
- Theme commands (`apps/client/src-tauri/app/src/themes.rs`) have no write gate; replicas are protected only by guard triggers. Quick notes gates writes with `can_write || replicated_writes` (`quick_notes.rs:48`).
- `contacts_local_identity` holds `public_key`, `card_nonce`, and `card_revision` (`crates/ganbaru-db/migrations/20261007120000_contacts.sql:4-11`). Its `public_key` is the personal space identity, read by `space_anchor` (`crates/ganbaru-sync/src/local.rs:38-50`). Creation requires `can_write` and no coordinator pin (`contacts/identity.rs:130-140`). `rotate_card` (`contacts/card.rs:169-185`) replaces the nonce, and received requests are checked against it (`contacts/requests.rs:156`).
- `contacts` has a random `id` and a unique `public_key` (`contacts.sql:13-15`). Accepting upserts with `ON CONFLICT(public_key)` and never touches a blocked row (`contacts/store.rs:284-311`); unblocking deletes the row.
- `contact_requests` uses the requester's id as the received row id (`contacts/requests.rs:190-195`), has a partial unique index on `(direction, public_key) WHERE state = 'pending'` (`contacts.sql:73`), and stores `last_attempt_at` and `last_error_code` per poll through `touch_request_attempt` (`contacts/store.rs:716`), which also bumps `updated_at`. Polling and delivery run only while the device can write (`contacts/requests.rs:756-761`).
- Every foreign key to `themes` comes from its owned child tables and uses `ON DELETE CASCADE` (`20261004220000_baseline_schema.sql:3806-3840`); nothing references `contacts`.
- Both Contacts tables have self-revision triggers (`contacts.sql:38,78`) that also fire on applier writes.
- The frontend already receives `applied { tables }` notices through `listenSyncApplied` (`apps/client/src/lib/api/sync.ts:258`).

### Re-verification on 2026-10-22

Checked against `feat/sync` after milestone 2 steps 1 to 5. `app/` is `apps/client/src-tauri/app/src`.

- Holds: the `config.rs` and `config.ts` bullets, `listenSyncApplied`, the key inventory, theme schema and child foreign keys, `replace_content`, `record_dismissal`, theme ids and import rule, ungated theme commands, the Contacts schema, readers, request polling gate, accept upsert, `touch_request_attempt`, and the seed implementation check.
- Differs: Quick notes gates with `status.can_write_domain(QUICK_NOTES_DOMAIN)` (`app/quick_notes.rs:49`); `replicated_writes` no longer exists. The Music reader is `music/session/runtime.rs:1578` through `vault_read_config`; `distractions.rs:127` and `chat/settings_commands_mobile.rs:44` also read `config.json`, but only branches that stay there. `space_anchor` is `crates/ganbaru-sync/src/local.rs:45-57`. The identity creation gate is `identity.rs:133-143`, and the identity row also has `created_at` and `updated_at`. `save_profile_image_bytes` requires `active_writable_vault_path` (`profile_images.rs:164`), which step 2 changes. `userThemeToWrite` (`stores/theme.svelte.ts:312-352`) re-normalizes seed rows on save, harmless once step 3 stops writing seeds.
- Milestone 2 APIs now: `DomainManifest` with `replica_writable` and per-domain `engine` tables, `VAULT_DOMAINS`, `vault_adapters`, `LocalColumn`, `DerivedKey { exists_group, key_inputs }`, `DomainAdapter::priority`, recovery `Presentation`, `can_write_domain`, `canWriteSyncDomain`. Not yet: `MaterializeHook`/`ChangeSink`, `seed_captures`, asset classes, owned codecs, elision. There is no engine table list in `vault.rs`; engine tables are declared per domain.
- Conformance constraints the Contacts and Themes design must meet: a derived-key table may not declare duplicate repair or redirects, so `contacts.id` (unique, `CHECK id = 'contact-' || public_key`) needs a column derived from the row key; a group stores either columns or owned rows, Immutable groups store columns, and an owned table's primary key must be `(parent, order)`; nullable text has no column type; a revision column needs a group with `bumps_revision`; Engine tables are not guarded (`tests/guards.rs:46` asserts the identity table is guarded); unique indexes must be non-partial; each new table updates the full masks in `tests/conformance.rs:923-930`.

### Decisions taken at start (2026-10-22)

- Step 1 starts now on the domain track; it needs no milestone 2 work. Steps 2 to 4 wait for milestone 2 steps 6 to 9 (step 3 also for step 10), per the dependency in each step.
- Table ids: the README's range 16 to 31 splits into the `preferences` domain (table 16) and the `themes` domain (17 to 31: `themes` 17, owned and seed tables after it), so each has its own replica write flag. Contacts keeps 32 to 34.
- Engine shapes requested from milestone 2 step 7: an `OptionalText` column type; owned tables that keep their own primary key under a custom codec, in a group beside parent columns; and a column derived from the row key (for `contacts.id`) whose unique index and `CHECK` are implied by the key and need no resolution. If step 7 cannot provide one of them, the matching step here takes the closest form the engine allows (for example `contacts.id` as a Local column filled by a hook) and records why.
- Step 4 names the groups that bump each revision column, updates the identity guard test for the Engine class, and keeps the app-side gate in `identity.rs` as the barrier.
- Milestone 2 step 7 delivered `OptionalText` and owned tables under their own primary key with a custom codec (`OwnedOrder::Key`); the column derived from the row key for `contacts.id` comes in milestone 2 step 8, so step 4 waits for it. Domain code that deletes rows cascading into a generic owned table renumbers the owner's remaining ordinals from 0 in the same transaction, as materialize writes them (themes tokens and palettes if they use the generic codec).

## Design

### Preference registry

One native module, `apps/client/src-tauri/app/src/vault/preferences.rs` with `preferences/` (`registry.rs`, `device.rs`, `view.rs`, `import.rs`), owns the split. No new crate: values are opaque JSON to the engine, and the registry is application policy.

```rust
pub(crate) enum Scope { Portable, Device, VaultConfig }
pub(crate) enum Shape { Exact, PerChild { max_children: u16 } }
pub(crate) struct PreferenceSpec { pub path: &'static str, pub scope: Scope, pub shape: Shape, pub rule: ValueRule }
```

- `ValueRule` covers `Bool`, `Integer { min, max }`, `Choice(&[&str])`, `Text { max_bytes }`, `IdList { max_items, max_id_bytes }`, and `Json { max_bytes }` for small structured values (`profile.imageCrop`, cover picker settings). Canonical JSON text, at most 64 KiB per value.
- `Exact` maps the whole value at `path` to one row. `PerChild` maps each child of the object at `path` (for example `notes.customOrderByProject.<projectId>`) to its own row, so concurrent edits for two projects never collide. Row keys stay within 128 bytes; a longer child key is refused.
- `VaultConfig` covers exactly `distractions` and `chat`, which stay in `config.json`.
- A write to an unregistered path fails with an error naming the path. `config.ts` treats a refused patch like any failed patch: it restores the cached keys from the effective view, fires `onConfigChangedElsewhere` for them, and logs the error with the path. Every key in the inventory is registered in the same step that introduces the registry.
- Remote values are not validated at apply: a newer app may add keys. A winner that fails the local rule, or a key this build does not know, is left in the table and ignored by the effective view, so older and newer devices coexist without shims.

Classification:

| Scope | Keys |
| --- | --- |
| Portable | `profile.*`; `preferences.language`, `eventTimezoneDisplay`, `calendarTimeFormat`, `calendarDimPastEvents`, every `focus*` and `musicPause*` key, `quickNotesDefaultColor`, `notesDefaultOpenMode`, the three sidebar order keys, `notesShowPropertyIcons`, `projectsShowPropertyIcons`; `contacts.trustDefaults.*`; `theme.quickToggleLightId`, `theme.quickToggleDarkId`; `updates.autoNotifications`; `projects.starredProjectIds`, `starredViewIds`, `iconPicker.askEveryTime`, `iconPicker.defaultColor`; `notes.favoritePageIds`, `favoriteFolderIds`, `sidebarOrderByProject` (per child), `customOrderByProject` (per child), `showArchivedInPickers`, `databaseViewTabDisplay`, `coverPicker.*`; `chatSidebar.starredChannelIds`, `orderByProject` (per child), `customOrderByProject` (per child); `music.review.autoplay`, `music.sources.*` |
| Device | `preferences.fontFamilyId`, `fontScale`, `calendarViewMode`, `titleBarVisibility`, the four `notes*NotificationsEnabled` keys and `notesNotificationIncludeContent`; `theme.activeId`; `updates.lastAutoCheckAt`; `projects.activeProjectId`, `activeViewIds`, `sidebarCollapsed`, `sidebarCollapsedProjectIds`, `showInactiveProjects`, `iconPicker.recent`; `notes.recentPageIds`, `selectedPageId`, `sidebarCollapsedFolderIds`; `chatSidebar.showArchivedInPickers` |
| Vault config | `distractions`, `chat` |

Reasons for the judgment calls:

- `theme.activeId` is device-local because toggling dark mode on a phone at night must not repaint a desktop. The quick-toggle pair is portable because it is the person's choice of light and dark themes. A device with no stored active id uses the quick-toggle theme that matches the system color scheme.
- Notes notification toggles are notification delivery, which the storage ownership table already lists as device-local.
- Focus idle and break settings are rhythm defaults and stay portable.

### Storage

```sql
CREATE TABLE vault_preferences (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(key) BETWEEN 1 AND 128),
    value TEXT NOT NULL CHECK (json_valid(value) AND length(value) <= 65536),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
```

- Resetting a preference to its default deletes the row. The key derives from content, so the table declares a derived key with an existence register and setting the key again later works.
- The device file is `<app_config_dir>/vault-preferences/<vault_id>.json`, a JSON object of device keys, read bounded by `MAX_VAULT_CONFIG_BYTES` and written with `ganbaru-atomic-files` under the existing `CONFIG_LOCK`. The path comes from Tauri's app config directory and the vault id from `vault.json`, never a hardcoded folder. Production and dev builds therefore keep separate files, like the active-vault pointer.
- The device file was chosen over a `Local` table because device preferences need no atomicity with domain writes and must survive vault replacement, rollback restores, and refreshes unchanged, which a file outside the vault gives with no restore step. Backups leave it out by design.

### Effective view and routing

- `vault_read_config` returns one JSON object composed from the portable rows (expanded back to nested paths), the device file, and the `distractions` and `chat` branches of `config.json`. Paths are disjoint by registry, so composition is order-free. The frontend `getConfigKey` and `setConfigKey` API is unchanged.
- `vault_patch_config` groups each patch batch by scope: portable patches commit in one SQLite transaction (capture happens there), device patches in one device file write, and vault config patches through `mutate_active_vault_config` as today. Device writes are allowed on every device. Portable writes stay owner-only in step 1, while the table is still Unconverted and guarded, and gate on `can_write` or the preferences domain's replica write flag (milestone 2 step 1) from step 2. `config.json` keeps its owner-only rule.
- Native managed state `PreferencesState` holds the effective view as an `Arc<serde_json::Value>` behind a lock. It loads at vault open and refreshes after every local patch. The post-apply hook for table 16 takes only the connection and the change sink and reports the changed keys through the sink; the app's sink refreshes `PreferencesState` and runs the side effects below, so the hook needs no `AppHandle` and also runs in milestone 3's headless Android pass, after which the view reloads at the next vault open. The four native readers in [Current state](#current-state) read it synchronously instead of parsing `config.json`; `read_active_config_bounded` remains only for `distractions` and `chat`.
- Side effects move with the data: a change under `preferences.*`, local or applied, invalidates the Focus calendar and marks Focus presentation preferences stale; a change under `music.*` or `preferences.musicPause*` notifies the Music session.
- Values that fail validation read as their default. The language preference stops writing a normalized value back; normalization happens at read, so no device writes a correction that races another device.

### First-open import

At every vault open on a device that can write `config.json`:

1. In one SQLite transaction, `INSERT OR IGNORE` every registered portable key found in `config.json` into `vault_preferences`.
2. Write each registered device key found in `config.json` into the device file when the file has no entry for it.
3. Rewrite `config.json` atomically with only `distractions` and `chat`.

A file write cannot join the SQLite transaction, so the steps are ordered and each is idempotent: a crash after step 1 repeats steps 1 to 3 at the next open with no effect on rows already present. A replica never rewrites `config.json`; at open it runs step 2 from the copy it holds and otherwise ignores moved keys. Old development vaults need nothing beyond this current-format import.

### Preferences mapping

`vault_preferences` (table 16), key column `key`, derived key with existence register, no revision column.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| value | 0 | `value` | Register | No | No |
| updated | 1 | `updated_at` | Max | No | No |
| exists | 2 | None (existence register) | Register | No | No |

Preference conflicts are silent. List values (favorites, stars, orders) are whole registers, so concurrent edits of the same list on two devices keep only the later list; this is accepted for this milestone because such edits are rare and cheap to redo.

### Profile picture

`profile.imagePath` stays portable. The preferences domain declares `AssetClass { name: "profile", max_bytes }`, with `max_bytes` the current 3 MiB profile image limit (`profile_images.rs:24`). `profile_images.rs:162` already names files `{sha256}.{ext}` under `assets/profile/`, so existing files match the engine's path and need no rename. The preferences adapter's `asset_refs` reports the file named by the `profile.imagePath` value; every device fetches it eagerly, and milestone 10's sweep deletes it once no live row or stored version references it, so the domain declares no holds.

### Themes mapping

`themes` (table 17), key column `id`, no revision column.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at_ms` | Immutable | No | No |
| seed | 1 | `seed_blend_canvas`, `seed_icon_label`, `seed_calendar_default_mode`, `seed_calendar_default_custom`; owned `theme_seed_tokens`, `theme_seed_event_palette` | Immutable | No | No |
| name | 2 | `display_name` | Register | No | Yes |
| icon | 3 | `icon_label` | Register | No | Yes |
| content | 4 | `blend_canvas`, `derivation_engine_version`, `calendar_default_mode`, `calendar_default_custom`; owned `theme_tokens`, `theme_event_palette` | Register | Yes | Yes |
| dismissal | 5 | `upgrade_dismissed_engine_version` | Max | No | No |
| updated | 6 | `updated_at_ms` | Max | No | No |

- The content and seed adapter codecs are canonical: token rows sorted by `(kind, key)` with the isolated flag, palette as exactly 32 slots in order, then the columns. A full theme stays far below the 256 KiB text bound. The four owned tables declare `codec: Custom` and keep their composite primary keys, since the generic codec requires `(owner, ordinal)` keys; milestone 2 step 7 allows owned tables beside parent columns in Immutable and Register groups.
- `replace_content` rewrites every token and palette row on each save; milestone 2 step 10's unchanged-group elision keeps a rename or icon change from sealing the content group, so the themes step merges after that step.
- The content conflict uses the generic presentation (pick one version); the theme renderer shows the name, icon, and palette strip of each version. Duplicating a version first is how a person keeps both. A theme edit is a deliberate design session, so this is the one place in this milestone where a conflict is surfaced.
- Seeds are per-theme reset snapshots created by clone or import, so they replicate with the theme. The README rule that seed rows need no merge covers built-in seed rows only. `replace_content` stops writing seed columns and seed rows; only `insert` writes them.
- `theme_upgrade_dismissals` is replaced by `themes.upgrade_dismissed_engine_version INTEGER NOT NULL DEFAULT 0`, filled from `MAX(engine_version)` and the table dropped in the same migration. Dismissal is portable: dismissing an upgrade prompt on one device dismisses it everywhere.
- New ids come from `crypto.randomUUID()`, which passes `validate_theme_id`; existing `theme-` ids stay. Import always assigns a fresh id instead of rejecting an existing one, so an imported file never collides with a live or tombstoned key.
- Theme write commands gate on `can_write` or the themes domain's replica write flag.
- On an `applied` notice naming `themes`, the theme store reloads user themes. If the device's `activeId` or a quick-toggle id names a theme that no longer exists, the store falls back at read time (quick-toggle choice for the active id, built-in light or dark for the toggles) and writes nothing; only the deleting device writes the reset, as today.

### Contacts mapping

`contacts_local_identity` becomes an Engine table: never captured, carried whole by snapshots, and immutable after creation through new `BEFORE UPDATE` and `BEFORE DELETE` triggers, added by the Contacts migration, that abort, declared as `DomainTrigger { name: "contacts_local_identity_immutable_update", policy: ApplierVisible }` and `DomainTrigger { name: "contacts_local_identity_immutable_delete", policy: ApplierVisible }`. The table joins the manifest's engine list (`crates/ganbaru-sync/src/manifest/vault.rs:18`). It is the personal space identity, so it must never be a merge participant. Its card columns move out, and creation keeps the current rule (writable, no coordinator pin) and inserts the identity and card rows in one transaction.

`contact_card` (table 34), key column `key` with `CHECK (key = 'card')`, never deleted: a `BEFORE DELETE` trigger aborts, declared as `DomainTrigger { name: "contact_card_prevent_delete", policy: ApplierVisible }`.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| card | 0 | `card_nonce`, `card_revision` | Coupled, priority `card_revision`, then `(clock, writer)` | No | No |
| updated | 1 | `updated_at` | Max | No | No |

Rotation works on any device that can write replicated data. The coordinator rejects requests made with the new card until it applies the rotation, normally within the sync delay; concurrent rotations converge on one nonce and both old nonces stop working.

`contacts` (table 32) is rebuilt with `public_key` as the primary key and row key (derived key with existence register), and `id` written explicitly by the store as `'contact-' || public_key` with a matching CHECK and a unique index, so command DTOs keep their `id`. A generated column is not used: the only one in the schema (`chat_threads.title_search`) is not captured, and an explicit column keeps the capture triggers uniform.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `created_at` | Register | No | No |
| profile | 1 | `display_name`, `color` | Register | No | Yes |
| standing | 2 | `state`, `accepted_at`, `blocked_at`, `invite_trust`, `invite_trust_expires_at`, `message_trust`, `message_trust_expires_at` | Coupled, priority blocked over active, then `(clock, writer)` | No | Yes |
| updated | 3 | `updated_at` | Max | No | No |
| exists | 4 | None (existence register) | Register | No | No |

- A concurrent accept never clears a block, matching the contract. Unblock deletes the row, which writes existence 0 and never a tombstone. A later local accept of the same key seals every group as a write covering the earlier versions, so a contact blocked, unblocked, and accepted again comes back active with the new accept time in `created_at`. That is why `created_at` is a Register group: on a derived-key table an Immutable group may hold only key columns, since a re-add seals it again with a different value (milestone 2 step 4). A block concurrent with an accept that creates the row resolves by the `standing` priority, so blocked wins.
- `revision` is a `Local` column and the table's revision column; materialize writes `1` for new rows.

`contact_requests` (table 33), key column `id`, always a locally minted random token. Row keys are never chosen by a remote party, because published tombstones block reuse forever. The requester's id moves to a new column `peer_request_id` (null for sent rows), indexed with `public_key`, and status polls look it up there.

| Group | Id | Columns | Kind | Surfaced | Recoverable |
| --- | --- | --- | --- | --- | --- |
| created | 0 | `direction`, `public_key`, `peer_request_id`, `display_name`, `color`, `card`, `card_digest`, `endpoint_hint`, `coordinator_fingerprint`, `expires_at`, `created_at` | Immutable | No | No |
| state | 1 | `state`, `invite_trust`, `message_trust` | Coupled, priority accepted, declined, expired, pending, then `(clock, writer)` | No | No |
| updated | 2 | `updated_at` | Max | No | No |

- `last_attempt_at`, `last_error_code`, and `revision` are `Local` columns; materialize writes `NULL`, `NULL`, and `1` for new rows. `touch_request_attempt` stops bumping `updated_at`, so `updated_at` changes only with the request state. Delivery attempts would otherwise publish an operation every poll; replicas show a pending request without its last delivery error.
- The partial unique index becomes a plain index on `(direction, public_key, state)`. Received rows have one writer (the coordinator), and two devices sending to the same person offline produce two pending sent rows that are harmless: the recipient replaces the earlier one, and the first accepted answer closes the rest through `close_pending_from`.
- Accepted outranks declined so the request agrees with the contact row an accept creates.
- Receipt, delivery, polling, pruning, and expiry writes stay on the coordinator. Other devices show a pending request past `expires_at` as expired at read time and write nothing.
- Contacts commands for contacts, sent requests, answers, and card rotation gate on `can_write` or the Contacts domain's replica write flag.

Both self-revision triggers are recreated for the rebuilt tables with no applying gate and declared as `DomainTrigger { name: "contacts_revision_after_update", policy: SameRow }` and `DomainTrigger { name: "contact_requests_revision_after_update", policy: SameRow }`: they write only the `Local` `revision` column of the triggering row, so they also fire on applier writes, and the conformance test asserts that a revision-only update captures nothing.

### Migrations

One migration per step, named with the UTC time of writing: `<timestamp>_vault_preferences.sql` (table, capture triggers), `<timestamp>_themes_replication.sql` (dismissal column, dismissal table drop, capture triggers for the theme and owned tables), and `<timestamp>_contacts_replication.sql` (`contact_card`, identity rebuild and immutability triggers, `contacts` rebuild, `contact_requests` rebuild with `peer_request_id`, recreated revision triggers, capture triggers). Each rebuild copies rows inside the migration transaction and ends with an empty `foreign_key_check`. Existing contact ids become `'contact-' || public_key`; nothing references them. Migrations install capture triggers only; milestone 2's `seed_captures` publishes existing rows at the next vault open.

### Frontend

- `config.ts` subscribes to `listenSyncApplied`; when `vault_preferences` is named, it re-reads the effective view, skips keys with pending local edits, and fires `onConfigChangedElsewhere` for changed keys. Each window receives the notice itself, so no cross-window relay is added; device-local writes keep using the existing `vault-config-changed` channel.
- The theme store reloads on `themes`; the Contacts store reloads on `contacts`, `contact_requests`, and `contact_card`.
- The active-theme fallback becomes a pure helper in `apps/client/src/lib/themes/active-theme.ts` with tests.
- Theme conflict strings go in `apps/client/src/lib/i18n/messages/{en,es}/theme.ts`.

## Testing

- Registry: every inventory key is registered exactly once, scopes are disjoint, `PerChild` key bounds, each `ValueRule` accepts and refuses at its edges, unknown and invalid winners read as defaults.
- Routing and view: a mixed patch batch lands each part in its store; the view round-trips through `vault_read_config`; replicas write portable and device keys but not `config.json`.
- Import: fresh import, repeat import is a no-op, crash after the SQLite commit and after the device write both converge, replicas import only device keys.
- Conformance, codec round-trip, and crash-boundary tests for tables 16, 17, 32, 33, and 34 through milestone 2's data-driven suite.
- Simulation: preference delete then set again; concurrent theme saves surface one conflict and converge; seed never changes after create; dismissal takes the maximum; block beats concurrent accept including concurrent creates; card revision wins; duplicate pending sent rows converge; a request delete concurrent with an answer.
- Contacts: existing `contacts/tests.rs` pass with the card table, `peer_request_id` lookups, and a requester reusing a tombstoned id gets a fresh local row.
- Frontend: `config.test.ts` covers the applied refresh and pending-key skip; `active-theme.test.ts` covers fallbacks; `operations.test.ts` covers UUID ids and fresh import ids.

## Work breakdown

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-preference-split` | Registry, device file, `vault_preferences` table without capture, effective view, routing, import, native readers, side effects, language read-time normalization, docs for the split. | `cargo test -p ganbaru-tauri-app --lib -j 4 preferences -- --test-threads=4`; `cargo test -p ganbaru-db --lib -j 4 schema -- --test-threads=4`; `pnpm --dir apps/client exec vitest run src/lib/vault/config.test.ts --maxWorkers=1` |
| 2 | `feat/sync-preferences` | Table 16 manifest, adapter, capture triggers, post-apply hook and change sink handling, profile asset class and reference, write gates, frontend applied refresh. | `cargo test -p ganbaru-sync --lib -j 4 preferences -- --test-threads=4`; `cargo test -p ganbaru-tauri-app --lib -j 4 preferences -- --test-threads=4`; `pnpm --dir apps/client exec vitest run src/lib/vault/config.test.ts --maxWorkers=1` |
| 3 | `feat/sync-themes` | Dismissal column, seed write removal, table 17 manifest and codecs, write gates, UUID ids, fresh import ids, store reload and fallback helper, conflict renderer and strings. | `cargo test -p ganbaru-themes --lib -j 4 -- --test-threads=4`; `cargo test -p ganbaru-sync --lib -j 4 themes -- --test-threads=4`; `pnpm --dir apps/client exec vitest run src/lib/themes/active-theme.test.ts src/lib/themes/operations.test.ts --maxWorkers=1` |
| 4 | `feat/sync-contacts` | Identity as Engine table, `contact_card`, `contacts` and `contact_requests` rebuilds, tables 32 to 34 manifest, revision triggers, write gates, store reloads. | `cargo test -p ganbaru-sync --lib -j 4 contacts -- --test-threads=4`; `cargo test -p ganbaru-tauri-app --lib -j 4 contacts -- --test-threads=4`; `cargo test -p ganbaru-db --lib -j 4 schema -- --test-threads=4` |
| 5 | None | Physical acceptance on two desktops and one Android phone, recorded below. | None |

Steps 2 to 4 each increment `VAULT_MANIFEST_VERSION` by one. Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

## Documentation

- `docs/data/sync.md`: line 76 says only `distractions` and `chat` in `config.json` keep owner-only rules; the Preferences row (line 173) lists the device-local keys including the active theme; line 180 says portable preferences live in `vault_preferences` and device preferences in the app config directory, and `config.json` remains for distraction rules and Chat settings; the Contacts section (lines 215 and 217) says the identity row is immutable and arrives with the snapshot, card state replicates, any linked device can manage contacts and send requests, and receipt and delivery stay on the coordinator; the delivery table (line 243) moves preferences, themes, and Contacts to implemented.
- `docs/data/schema/sync.md`: "Currently replicated" adds preferences, themes, and Contacts; the Tables section adds `contacts_local_identity` as an Engine table, immutable after creation and carried whole by snapshots.
- `docs/data/schema/supporting-domains.md`: line 21 says the active theme is device-local and the quick-toggle pair is portable; the profile paragraph names `vault_preferences`; the Contacts paragraph (line 61) describes the card table, the identity as immutable, delivery fields as device-local, and replaces "follow it across linked devices as part of the whole-vault handoff".
- `docs/features/themes/README.md`: line 61 for the new preference homes (font family and scale device-local); line 69 says import assigns a fresh id; line 73 says font family and scale are device-local preferences.
- `docs/features/profile.md`: lines 5 and 7 replace `config.json` with portable preferences.
- `docs/features/localization.md`: line 7 for the storage and read-time normalization; line 19 for the setup handoff target.
- `docs/features/collaboration/README.md`: one sentence that contacts, trust, and card rotation sync between linked devices.
- `docs/features/README.md`: no change; the Themes (Implemented), Profile (Implemented), Localization (Partial), and Contacts (Partial) rows keep their status.
- `AGENTS.md`: the folder structure line for `config.json` says it holds distraction rules and Chat settings; the `src/vault/` map line mentions preferences.
- These edits close the README "Docs differ" rows naming milestone 4: the `config.json` row's milestone 4 part through the `docs/data/sync.md` edits above (the row stays open for milestones 7 and 9), and the `contacts_local_identity` row through the `docs/data/schema/sync.md` and `supporting-domains.md` edits.

## Manual acceptance

Two desktops (A is the coordinator and owner, B is linked) and one Android phone (P).

| Scenario | Expected |
| --- | --- |
| First open after upgrade on A | Every preference keeps its value; `config.json` holds only `distractions` and `chat`; device keys moved to the device file. |
| Change language on P | A and B switch language in every open window. |
| Change font scale and active theme on B | A and P unchanged. |
| Change profile name and picture on P | A and B show both; the contact card on A carries the new name. |
| Star a project on A while P is offline, then reconnect | Star appears on P. |
| Edit and save one theme on A and P offline, reconnect | One conflict on both; picking a version clears it everywhere. |
| Delete the theme active on B from A | B falls back to its quick-toggle theme without writing a preference. |
| Dismiss a theme upgrade prompt on B | Prompt gone on A and P. |
| Rotate the card on P, scan the new card from another vault | The request reaches A after sync; the old card is refused. |
| Block a contact on B while A accepts that person's request offline | After reconnect the contact is blocked everywhere. |
| Send a request from P | A delivers it; acceptance appears on P. |
| Import a theme file twice on two devices | Two separate themes, no rejection. |

### Acceptance results

| Scenario | Automated coverage | Physical only |
| --- | --- | --- |
| Pending | Pending | Pending |

## Performance budgets

| Measure | Budget |
| --- | --- |
| Effective view read by native readers | Under 50 µs (in-memory clone of an `Arc`) |
| Theme save operation size | Under 32 KiB |
| Vault open import when nothing moves | Under 5 ms on Android |

## Risks

| Risk | Mitigation |
| --- | --- |
| A key missed in the registry fails writes after step 1 | The inventory is enumerated in Current state, writes to unknown paths fail loudly, and the step 1 tests list every key constant. |
| Concurrent list edits lose one side | Rare and cheap to redo; set-valued rows are deferred with a clear trigger to build them. |
| A card rotated on a replica is refused until the coordinator applies it | Rotation seals immediately; status shows pending sync; acceptance covers the delay. |
| Theme content codec drifts from the editor's token set | Codec round-trip test over every built-in and a generated theme; unknown token keys are already dropped at import. |
| Profile picture asset arrives after the preference | The avatar falls back to initials until the asset exists, as with any missing managed file. |

## Deferred

- Set-valued preferences (stars, favorites) as derived-key rows per member, when a domain milestone moves them into domain tables or user reports show lost stars.
- Moving `distractions` out of `config.json` ([milestone 7](milestone_7.md)) and `chat` ([milestone 9](milestone_9.md)); whichever merges last deletes the file and its loader.
- Pruning device preference files of vaults the device no longer knows.
- Request receipt by any device ([multi-person sharing goal](../sharing/README.md)).

## Implementation checks

- Settled from source: seed values are assigned only by `cloneTheme` (`apps/client/src/lib/themes/operations.ts:96-105`) and import; the editor and `mergeThemePatch` never set them, and `replace_content` stops rewriting them in step 3, so the seed group stays Immutable.

## Progress

- [x] Step 1: preference split, registry, device file, effective view, import, native readers.
  - Date: 2026-10-22.
  - Branch: `feat/sync-preference-split` (from `feat/sync` at 60f3e1c4).
  - Commits: 5ef7579c `feat(vault): split portable and device preferences`; 33b9bc36 `docs(themes): limit the active theme fallback to startup`.
  - Deviation: the startup fallback for a missing active theme (quick-toggle theme matching the system scheme) is implemented here, although the plan files read-time theme fallbacks under step 3. Step 3 should build on it instead of re-implementing it. It has no test because the theme store has none.
  - Note: `run_preference_effects(app: &AppHandle<R>, paths: &[String])` needs an `AppHandle` because `pomodoro::invalidate_calendar` does. `effects_for` is pure. The step 2 change sink can hold a closure, built in the app crate, that captures a cloned handle.
  - Note for step 2: a portable write on a linked replica fails fast and reverts the cache. Stores that do not subscribe to `onConfigChangedElsewhere` (the translator, for example) keep the optimistic value until restart. The profile color first-run write-back logs an error on each replica boot without a saved color. In a mixed batch on a replica, the device part is lost because portable ops run first.
  - Implementation check: the plan's only implementation check concerns theme seeds in step 3, so this step does not affect it.
  - Decision: migration `20261022105141_vault_preferences.sql` adds the STRICT table `vault_preferences(key, value, updated_at)` with key and value byte bounds and no capture triggers. Unknown tables read as unconverted in the sync manifest, so replica guards already refuse writes to it.
  - Decision: the Rust registry (`vault/preferences/registry.rs`) holds 49 portable, 21 device, and 2 vault config entries (`distractions`, `chat`). Per-child maps (sidebar and custom orders per project) store one row per child. Each value is validated by a rule and capped at 64 KiB, which limits one project's custom order to roughly 1600 ids.
  - Decision: device preferences live in `{app config dir}/vault-preferences/{vault id}.json`. A device file read error is logged and reads as defaults.
  - Decision: `PreferencesState` caches the composed view per vault and generation. `resume_native_runtimes` resets it first, so every vault selection or replacement reloads.
  - Decision: import runs on the first view load when `config.json` still has moved branches. The owner inserts missing portable rows, merges missing device entries, and strips `config.json` to `distractions` and `chat`. A replica merges device entries only.
  - Decision: `vault_patch_config` routes each patch by registry scope. A mixed batch is not atomic across stores: portable ops share one transaction, then the device file is written, then `config.json`. The view is invalidated and reloaded after every attempt, including a failed one.
  - Decision: side effects are a pure `effects_for(paths)` plus `run_preference_effects`, which marks Focus presentation stale. Music and Contacts read the view on demand and need no notice. Step 2 can call both from a change sink. No effects run at load.
  - Decision: native readers (contacts card, profile image delete check, Focus presentation, Music session) await `effective_view`. Chat and distractions keep reading `config.json`.
  - Decision: frontend `config.ts` restores the saved values of a failed or refused write, unless a newer local edit is queued, and notifies subscribers without a cross-window publish. The language preference normalizes only at read. A device with no saved active theme uses the quick-toggle theme that matches the system color scheme, without writing it.
  - Decision: the profile color first-run write-back (a random slot saved once when missing) is unchanged; the plan does not name it.
  - Docs: `docs/data/architecture.md`, `docs/data/sync.md`, `docs/data/schema/supporting-domains.md`, `docs/data/security/application-boundaries.md`, `docs/features/themes/README.md`, `docs/features/profile.md`, `docs/features/localization.md`, `AGENTS.md`. The delivery table stays unchanged until replication lands.
- [x] Step 2: preferences replication and profile picture asset.
  - Date: 2026-10-22.
  - Branch: `feat/sync-preferences`, cut from local `feat/sync` at e028784c.
  - Commits: 72acdf85 `feat(sync): replicate portable preferences with the profile picture asset`; 1f36f13c `feat(preferences): let linked replicas write portable preferences and the profile picture`; 3caabdf8 `feat(preferences): refresh replicated preferences and profile pictures in every window`; ed2b7785 `docs(sync): describe preferences replication and the profile picture asset`.
  - Implementation check branch: `feat/sync-preferences` passed ganbaru-sync lib and conformance tests, ganbaru-db tests, ganbaru-tauri-app preference, profile_image, and sync tests, clippy with `-D warnings` on both crates, `cargo fmt --check`, `cargo check -p ganbaru --bin ganbaru`, focused Vitest (config and preference files), and `pnpm --dir apps/client run check`.
  - Decision: `profile.imagePath` became `profile.image` = `{path, byteSize}` because an asset reference needs the exact size; no compatibility reader since there are no external users. The JPEG extension is only `jpg`.
  - Decision: `VAULT_MANIFEST_VERSION` is 2; serde_json (existing workspace dependency) added to ganbaru-sync for the profile image value validation.
  - Decision: preference notices are consumed with one line in `apply_ready`; restored recovery offers refresh through the presentation `changed` callback. The presentation label reuses `settings.title`, so no new i18n keys.
  - Decision: received profile files pass a `with_check` on the `profile` class (capped read, image validation, content type match); avatars reload through a profile image file revision on applied `vault_preferences`.
  - Current state drift found: no app materialize hooks or notice consumers existed before this step; `Engine::vault()` callers (hub, service, vault assets, recovery, backup) all receive the hooks now.
  - Current state drift found: `apply_ready` ignores `report.seal.changed_tables` when emitting applied events; `active_replicated_asset_path` is named for assets but gates database writes; the derived key summary line has a trailing space; mixed batch ordering on replicas remains.
  - Orchestrator merge: resolved against M3 step 6 (sync host) and M5 step 3 (artwork); domain order quick notes, preferences, music; the app host's `vault_hooks` returns the engine's hooks, and `SyncHost::on_sync_applied` (no-op on the headless host) drops the cached preference view and runs effects on the Tauri host.
  - Orchestrator follow-ups: f1e12add `test(vault): derive replica writable domains from the manifest`; 888e1604 `fix(sync): report tables changed by seals and refresh cached preferences on every report` (every seal, re-seal, fork recovery, and revocation report goes through `SyncHost::report_changes`, which refreshes cached preference rows before views hear of changed tables and topics).
- [x] Step 3: themes replication.
  - Date: 2026-10-22.
  - Branch: `feat/sync-themes` (from local `feat/sync` at a112dd2d).
  - Commits: d7e63773 `feat(themes): replicate user themes between linked devices`, 7f3a8c4c `feat(themes): resolve theme conflicts and reload synced themes in the app`, 38e27269 `docs(themes): describe user theme replication and conflicts`, f78c26a2 `docs(agents): list user themes among replicated domains`.
  - Implementation check branch: seed preservation. `replace_content` now ignores seed fields (seed group is Immutable), and the frontend keeps seeds through `withSeedOf` on snapshot restore and draft replace, so no path rewrites a seed after creation.
  - Manifest: `VAULT_MANIFEST_VERSION` 3 with `fixtures/vault_manifest_v3.txt`; VAULT_DOMAINS order quick_notes, preferences, themes, music. `themes` is table 17 with groups created, seed (Immutable, owned seed tokens and palette), name and icon (Register, recoverable, not surfaced), content (Register, owned live tokens and palette, surfaced and recoverable), dismissal and updated (Max).
  - Migration `20261022191859_themes_replication.sql` moves the upgrade dismissal onto `themes.upgrade_dismissed_engine_version` (max of old rows) and drops `theme_upgrade_dismissals`.
  - No engine files edited. No-op elision is done in the domain (writes compare current content and skip unchanged rows) because owned table updates capture unconditionally until engine elision (M2 step 10) lands; once it lands this domain code is redundant but harmless.
  - Theme IDs are random lowercase UUIDs; import always assigns a fresh id and a unique display name instead of rejecting a taken id.
  - Conflicts: pick one version (`themes_conflict`, `themes_resolve_conflict`); keep both by duplicating first. List row badge and editor banner. Writes are gated by `can_write_domain(THEMES_DOMAIN)`.
  - Recovery: themes root recovery offers (name, icon, content are recoverable), so the app registers a themes restorer that creates a new theme with a fresh UUID whose seed is the recovered content.
  - Frontend: store reloads on `sync-applied` for theme tables, keeps unsaved fresh themes and the open editor buffer, defers the reload until the editor session ends; active theme falls back via `resolveActiveThemeId` when a linked device deletes it. `THEMES_SYNC_PRESENTATION` registered after preferences.
  - Deviation: the two-replica conflict simulation lives in `ganbaru-sync-replica` exchange tests (`concurrent_theme_saves_surface_one_conflict_that_closes_on_both_replicas`), which needed `ganbaru-themes` as a dev dependency.
  - Known gap: `reloadOnce` resolves from the in-memory `activeId`, which a fallback has already overwritten, not from the saved `theme.activeId` key, so after a fallback the saved choice is only reconsidered at startup (resolving from the saved key on reload risks a stale per-window config cache). `ganbaru-themes` Cargo description does not mention recovery yet. Quick-toggle preference changes from sync are validated at read time but the store's cached pair refreshes through the preferences path from step 2.
- [x] Step 4: Contacts replication.
  - Date: 2026-10-22.
  - Branch: `feat/sync-contacts` (from local `feat/sync` at 8172c9e2; not rebased onto fb3d1d07).
  - Commits: 06048bd4 `feat(contacts): replicate contacts between linked devices`, 6b725569 `docs(sync): describe contacts replication`.
  - Implementation check: the contacts, contact_requests, and contact_card tables replicate through the generic adapter (manifest version 4, tables 32 to 34); the device identity table `contacts_local_identity` stays an engine-local table. Focused tests pass for ganbaru-sync (166), ganbaru-db (109), ganbaru-sync-replica (40, including 4 new two-device contact merges), and tauri-app (full lib before the last fix, contacts 23 after it).
  - Decisions: all contacts groups are `recoverable: false` (no visible conflicts or recovery entries, a deviation from the plan); card nonce and revision moved to the replicated `contact_card` row, with card priority as `card_revision` clamped to u8; the identity creation gate stays app-side; request expiry is applied at read time; received rows get a random local id plus `peer_request_id`, so a second person reusing a peer request id gets their own row; duplicate pending rows are allowed (partial unique index dropped) and readers collapse them; an identity without a card reads as none; `touch_request_attempt` does not bump `updated_at`; handled-request pruning breaks ties by `created_at DESC, rowid DESC`; the frontend contacts store refreshes on applied sync tables.
  - Bundle contracts: fail identically on the base without this change (each Android closure one module over, build 1294 vs 1291), so no ceiling was raised here.

- [ ] Step 5: physical acceptance.

## Review fixes (done)

- Step 4 (Contacts):
  - 076e15fa fix(contacts): merge removals as a standing and harden request replication (findings 1 to 7)
  - 84b7bd59 fix(contacts): refresh the card revision when sync applies the card (finding 8)
  - 6c6217df docs(sync): describe removed contacts and trust precedence (findings 5, 9)
  - Tests: ganbaru-db contacts 5, ganbaru-sync 176, ganbaru-sync-replica 42, tauri-app contacts 26; clippy clean; client check clean; bundle contracts pass under the 1295 ceiling.
- Steps 1 to 3 (merged as 67b2d316 from `fix/sync-milestone-4-review`):
  - c65690f7 themes rename keeps `max(updated_at_ms)`, restored themes get unique names; b20e5b7f portable `config.json` import runs only when the device can write; 5666014f profile picture asset class documented; 1340f91b editor saves merge with remote edits (base recorded at session open, only changed groups written, conflict dialog with save as new, overwrite, keep editing), and edits of a theme deleted meanwhile are saved as a new theme.
  - Orchestrator follow-up: ae12a096 and 610ce979 make the frontend unique name skip suffixed names over the length limit, like the native restore.

## Milestone close

- 2026-10-23. Review fixes merged (67b2d316) and the review findings closed. `pnpm -w run validate` on 610ce979 (log `validate-m4-close.log` in the session scratchpad): check, test (33 Rust result lines, Vitest shards of 348 and 347 files) and editor-check passed; bundle contracts failed only on Android ceilings grown by `projects/order.ts` and `themes/editor-save.ts`, raised in 0a07da9f, after which the Android bundle check alone passed (the baseline file is read only by that stage).
- Multi-replica coverage for themes and contacts lives in per-domain scenario tests (`ganbaru-sync-replica` exchange tests and `ganbaru-sync` domain tests) rather than `sim.rs`, which models only the fixture domain; this is the accepted form for domain milestones until milestone 10 step 7 extends the harness.
- Open known gap carried to the goal report: after an active theme fallback, `reloadOnce` resolves from the in-memory `activeId`, so the saved choice is reconsidered only at startup.
