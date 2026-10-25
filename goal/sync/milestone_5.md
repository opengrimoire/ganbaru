# Milestone 5: Music library

**Status: Partial.** Steps 1 to 6 are implemented in source on `feat/sync`, with review fixes merged; physical acceptance (step 7) is pending. Part of the [concurrent sync goal](README.md). This file is the implementation plan for replicating the Music library: items, local roots and locations, sources, playlists and memberships, snoozes, soundscapes and presets, Focus music assignments, and listening statistics. [Device linking and synchronization](../../docs/data/sync.md) stays the owning contract, with [Sync merge rules](../../docs/algorithms/sync/README.md), [Sync schema](../../docs/data/schema/sync.md), the Music section of [supporting domains](../../docs/data/schema/supporting-domains.md), and [Music](../../docs/features/music/README.md). Each step moves the parts it implements into `docs/`. The progress checklist is at the end.

## Outcome

When this milestone is done:

- Playlists, memberships and their advanced settings, intended uses, review state, metadata overrides, item signals, snoozes, sources, soundscapes, presets, and Focus music assignments are editable on every linked device, online or offline, and converge to identical state.
- A device that has a local root bound to a folder discovers files there and those items appear on every device; a device without that root bound shows the source as "Not on this device" with Choose folder, and never marks its items missing for everyone.
- Two devices that discover the same file, add the same YouTube video or playlist, or add the same item to the same playlist produce one row, not duplicates.
- Concurrent playlist reorders converge to one order; deleting a playlist on one device while another adds songs to it produces a recovery entry that restores the playlist with those songs.
- Listening statistics count plays on every device without lost increments.
- An unchanged library rescan writes nothing to the operation log.

## Scope

In scope:

- Manifest and adapter for 15 Music tables (TableIds 48 to 62), with conformance, codec round trips, and simulation coverage.
- One shape migration: order keys for playlists, memberships, and preset sounds; break items folded into memberships; skip ranges as owned children; fixed-point integers for volume, rate, and level; per-device listening statistics.
- A single identity module for every content-derived Music key, used by desktop refresh, Android refresh, YouTube, import, and playlist writers.
- Writers that diff instead of deleting and reinserting, and that skip unchanged values.
- Removal of the read-path built-in playlist repair and of cross-domain rewrites in playlist deletion.
- Artwork overrides as managed vault assets under `assets/music/artwork/`.
- Frontend: unbound root status, YouTube eligibility from resolution state, aggregated statistics, playlist recovery strings.

Out of scope:

- Media bytes, root folder bindings, local soundscape file paths, playback sessions, receipts, refresh jobs, and relink plans; they stay device-local.
- Live playback handoff between devices or remote control.
- `projects.focus_playlist_id`, `projects.break_playlist_id`, and `calendar_events.playlist_id`, which belong to milestones 6 and 7.
- Replication of custom emojis and project icon assets referenced by playlist icons (milestone 6).
- A user interface for artwork overrides; only the storage contract and the picker command change.
- Surfaced conflicts for any Music value.

## Prerequisites

- Milestone 2 is merged: `Local` table and column class with a declared materialization value per Local column and the per-domain replica write flag (step 1), cascade hiding through `CASCADE` foreign keys including those inside Immutable groups, dependency-ordered apply, derived keys with existence registers and the composite row key convention (`sync_key` filled by `ganbaru_sync_contracts::derived_key`, generic `pending:` startup refusal, step 4), grouped recovery for cascaded children (step 5), capture seeding through `sync_seeded_tables` and `seed_captures` and batched sealing (step 6), generic owned children keyed `(owner, ordinal)` and post-apply hooks registered by the app composition root (step 7), the asset class registry (step 8), guards skipping while `sync_apply_state.applying = 1`, recovery presentation and generalized domain change notices (step 9), and winner elision and unchanged-group elision at seal (step 10).
- Milestone 1's Quick notes order-key migration is the pattern for filling order keys in SQL.
- No new external dependencies. `sha2` is already a `ganbaru-music-library` dependency, and random UUID keys come from the frontend (`crypto.randomUUID`), so Rust needs no `uuid`; it and `ganbaru-music` gain a path dependency on the Tauri-free `ganbaru-sync-contracts` for `derived_key`.

## Current state

Facts this design builds on, verified in source:

- **Item identity.** Local items are created as `stable_id("item", [root_id, relative_path])` (`local_refresh/helpers.rs:69`), producing `music-item-{sha256}`, with `identity_key = local:{id}`. YouTube items are `music-youtube-{video_id}` (`youtube.rs:545`). `stable_id` is copied four times (`local_refresh/helpers.rs:72`, `youtube.rs:578`, `relink.rs:736`, and the Android `apps/client/src-tauri/app/src/music/library/mobile_refresh.rs:426`). Locations are `stable_id("location", [root, path])` (`local_refresh/persistence.rs:795`).
- **Import.** `interchange.rs:356` creates `music-import:{operation}:item:{n}` when no row has the identity key, `interchange.rs:270` creates `music-import:{operation}:playlist:{n}`, `interchange.rs:423` creates `music-import:{operation}:location:{n}`, and `interchange.rs:452` creates `music-import:{operation}:membership:{p}:{m}`. Imported playlists stay per-import user decisions; imported items, locations, and memberships take the identity table keys below. Two devices importing the same file create different keys for the same identity.
- **Client ids.** Roots and local-root collections get `crypto.randomUUID()` ids with `identity_key = local-root:{rootId}` (`lib/music/sources/controller.svelte.ts:362-368`). Membership ids come from requests; `duplicate_playlist` composes `'duplicate-membership:' || ? || ':' || id` in SQL (`writes.rs:192`), so repeated duplication grows ids. `validation.rs:9` allows ids up to 200 bytes; the engine row key bound is 128.
- **Delete and reinsert writers.** Intended uses (`writes.rs:118`), signals (`writes.rs:527`), skip ranges (`writes.rs:679`), preset sounds (`soundscape_presets.rs:127`), assignments (`crates/ganbaru-music/src/assignments.rs:190`), and import replace (`interchange.rs:295`, `:300`, `:398`).
- **Scan churn.** `upsert_item` (`local_refresh/persistence.rs:743`) rewrites every column and `updated_at_ms` on every scan; `upsert_location` rewrites unchanged files; `resolve_item_identity` (`persistence.rs:692-741`) normalizes backslash paths with an in-place `relative_path` update. Android refresh is a separate SQL path (`app/src/music/library/mobile_refresh.rs:117-330`).
- **Deletes.** Items are deleted only by orphan pruning (`source_lifecycle.rs:177`); locations and roots only by item repair (`item_repair.rs:182`, `:196`). Collections are soft-removed through `removed_at_ms`.
- **Playlist delete** (`writes.rs:346-420`) takes a replacement and an expected impact and rewrites `projects.focus_playlist_id` and `break_playlist_id`, `calendar_events.playlist_id`, and `music_context_assignments.playlist_id` before deleting.
- **Built-in repair.** `ensure_built_in_music_playlists` (`defaults.rs:87-130`) runs `INSERT OR IGNORE` from the list read path (`queries.rs:432`). Built-in deletion is already refused (`writes.rs:247`).
- **Schema.** Playlists order by `sort_order` (`idx_music_playlists_sort`), memberships by `position` (three indexes), skip ranges by `UNIQUE (membership_id, sort_order)`, preset sounds by `PRIMARY KEY (preset_id, position)` with `UNIQUE (preset_id, soundscape_id)`. `music_membership_break_items` is 1:1 with memberships and its `item_id` is `ON DELETE NO ACTION`. `music_listening_statistics` has `item_id` as primary key and is incremented in place (`playback.rs:43`). `music_context_assignments` has no foreign keys, and dangling preset and playlist ids are reported as unavailable by design (`20261019180000_music_soundscape_preset_mixes.sql:41`).
- **Parents of cascades.** Items, roots, collections, playlists, memberships, soundscapes, and presets all have `ON DELETE CASCADE` children, so none of them can be rebuilt by rename without deleting children.
- **Device state already outside the vault.** Root folder bindings and local soundscape paths live in app state (`app/src/vault.rs:66` and `:73`); the device id comes from `ensure_device_id` (`vault.rs:124`).
- **Device observations inside replicated rows.** `availability`, `original_artwork_identity` (hashes the absolute path with `DefaultHasher`, `artwork.rs:200`), location size, mtime, generation, and collection refresh state.
- **Eligibility.** Playlist playback requires `availability = 'available'` for every source kind (`lib/music/playlists/playback.ts:97`, `:100`). Root bindings report `available`, `missing`, or `needs-relink` (`lib/music/library/contracts.ts:24`).
- **Artwork.** `artwork_override` is free text; `music_pick_artwork_file` (`app/src/music.rs:327-350`) returns an absolute path on desktop and a content URI on Android. `pickArtworkFile` (`lib/api/music.ts:49`) and `saveMetadataOverrides` (`inspector.svelte.ts:117`) have no callers. `library-and-sources.md` already says managed overrides are vault assets.
- **Description limits** are abort triggers above 500 characters (`20261020150000_music_item_description_limits.sql`); writers truncate first.

### Re-verification on 2026-10-22

Checked against `feat/sync` after milestone 2 steps 1 to 4 and milestone 3 steps 1 to 5.

- Holds: item identity copies (`helpers.rs:69/72`, `youtube.rs:545/578`, `relink.rs:736`, `mobile_refresh.rs:426`, `persistence.rs:795`; the `relink.rs` and `youtube.rs` copies also build Local relink, job, and issue ids), delete-and-reinsert writers, scan churn, deletes, playlist delete (`writes.rs:346-420`, assignments update :396), built-in repair, schema, device state, observations, eligibility, artwork picker, description limits, checkpoint payloads (`models.rs:126`). Android refresh is `start` at `mobile_refresh.rs:63` plus `persist_track` at :238-348; `relink.rs:498` also rewrites `relative_path` in place. `saveMetadataOverrides` is at `lib/music/builder/inspector.svelte.ts:118`.
- Differs: import location ids are `music-import:{op}:location:{n}:{location_index}` (`interchange.rs:423`), and import also writes assignments (`interchange.rs:540`).
- Client ids the plan did not list: YouTube playlist collections get `randomUUID` (`controller.svelte.ts:471`); item repair creates a root and a location with UUIDs (`:577-578`, `item_repair.rs:136`); bulk add composes membership ids as `{action_id}:{i}:{playlist_id}` (`playlist_edits.rs:24`), which can exceed 128 bytes; also `writes.rs:208` and `:223`.
- Derived key API: `ganbaru_sync_contracts::derived_key(prefix: &str, parts: &[&str]) -> Result<RowKey, DerivedKeyError>` (`ids.rs:302`), output `{prefix}:` plus 32 hex digits of SHA-256 over the parts joined by NUL; prefixes are 1 to 32 bytes of `[a-z0-9_-]` and not `pending`. The six plan prefixes are valid. Engine side: `manifest::SYNC_KEY_COLUMN`, `DerivedKey { exists_group, key_inputs }`. Neither music crate depends on the contracts crate yet.
- Engine constraints the replication step must meet: duplicate repair is refused on derived-key tables (`manifest.rs:784`), `sync_key` uniqueness is implied (`conformance.rs:748`), Immutable groups must be non-recoverable (`conformance.rs:837`), the partial unique index `idx_music_soundscapes_generated_kind` is refused by conformance (`conformance.rs:664`), rekeying a `pending:` placeholder captures the row as created (`triggers.rs:386`), and the `pending:` check runs in `Engine::init_space` at sync start (`app/src/sync/service.rs:861`).
- `ganbaru-db` has no schema dump; music schema tests are in `src/tests/music.rs`, and `tests/core.rs:80` lists `music_membership_break_items` as a required object. `validate_migration_prefix` (`lib.rs:403`) requires applied history to be an ordered prefix, so a migration landing after a later-timestamped one is re-timestamped at merge.

### Decisions taken at start (2026-10-22)

- Steps 1 and 2 start before milestone 2 finishes. They touch no engine file and no `sync_*` table, and the working agreement takes the first unfinished step whose needs are done; the README's coarser "after 2" ordering applies to the replication step.
- Step 1 covers every unlisted client id above: YouTube playlist collections use the collection key from `youtube-playlist:{playlist_id}`, item repair keeps a random root id and computes the location key, bulk add and import compute membership and location keys in Rust, and import location ids use the location formula.
- Memberships (table 57) keep `stable_id("membership", [playlist_id, item_id])` as a plain key in step 1. The replication step resolves how `UNIQUE(playlist_id, item_id)` is declared: the engine track adds, in milestone 2 step 7, that a unique constraint whose columns equal a derived key's `key_inputs` is implied by the row key and needs no repair. Identity groups are non-recoverable, and `DerivedKey` lists `key_inputs`.
- The replication step drops the duplicate repair for `idx_music_soundscapes_generated_kind` (refused by conformance) and resolves it there; the conversion pass that rekeys `pending:` placeholders runs with capture as the engine defines it, and the "seeds no captures" wording is revised at that step.
- `sync_key` is added with a placeholder default or a table rebuild, since SQLite refuses `ADD COLUMN ... NOT NULL` without a default.
- Vault data check (longest key per Music table): the maintainer's vault is on another machine, so the plan takes a data-independent branch. The shape migration rekeys nothing; the replication step's Rust conversion pass rekeys any Music row whose key exceeds 128 bytes with the identity formula (memberships together with their skip ranges, deleting `music_session_checkpoints` rows that name a rekeyed membership), and readers accept legacy keys of any length until then.
- Milestone 2 step 7 contract: a unique constraint equal to a derived key's `key_inputs` is implied by the row key (memberships). Domain code that deletes rows cascading into a generic owned table (skip ranges, owned break items) renumbers the owner's remaining ordinals from 0 in the same transaction, as materialize writes them.

## Design

### Classification

| Class | Tables |
| --- | --- |
| Replicated | `music_local_roots`, `music_library_items`, `music_item_signals`, `music_listening_statistics`, `music_local_locations`, `music_source_collections`, `music_source_collection_items`, `music_playlists`, `music_playlist_intended_uses`, `music_playlist_memberships`, `music_snoozes`, `music_soundscapes`, `music_soundscape_presets`, `music_soundscape_preset_sounds`, `music_context_assignments` |
| OwnedChildren | `music_membership_skip_ranges` (generic owned table of memberships, keyed `(membership_id, ordinal)`) |
| Local | `music_session_checkpoints`, `music_session_receipts`, `music_transfer_receipts`, `music_recent_selections`, `music_soundscape_state`, `music_relink_plans`, `music_relink_plan_entries`, `music_refresh_jobs`, `music_refresh_job_entries`, `music_refresh_job_issues` |
| Derived | `music_search_fts`, `music_search_index_state` |

Local columns on replicated tables are never captured. The manifest declares a materialization value for each (milestone 2 step 1), which the applier inserts for a new remote row: schema defaults for items, locations, collections, and collection items, and `missing` for `music_soundscapes.availability`, which is `NOT NULL` without a default and cannot be altered because soundscapes are a cascade parent. Snapshot preparation writes the same values on every row (milestone 2 step 1), so Music needs no bootstrap reset. `version` is local on every table, and the applier bumps it when a group materializes a change.

Replicated columns never use `REAL` (milestone 2 conformance). Membership `volume` and `rate`, their break copies, and preset and preset sound `level` become integer thousandths in `_milli` columns (volume and preset sound level 0 to 1000, preset level 0 to 2000, rate 250 to 2000); writers, readers, and the frontend contract use the integer unit.

TableIds are assigned so every reference points to a lower id. 63 to 95 stay reserved.

### Identity module

`crates/ganbaru-music-library/src/identity.rs` owns every content-derived key and replaces the four `stable_id` copies with the same formula (`music-{kind}-{sha256 hex}`), so existing scanned ids stay valid:

| Row | Key |
| --- | --- |
| Local item | `stable_id("item", [root_id, relative_path])` at first discovery |
| Imported non-YouTube item | `stable_id("import-item", [identity_key])` |
| YouTube item | `music-youtube-{video_id}` |
| Location | `stable_id("location", [root_id, relative_path])` |
| Collection | `stable_id("collection", [identity_key])`, with `identity_key` `local-root:{root_id}` or `youtube-playlist:{playlist_id}` |
| Membership | `stable_id("membership", [playlist_id, item_id])` |
| Signals, intended uses, collection items, preset sounds, statistics, assignments | `sync_key` = `derived_key(prefix, primary key columns)` |

Requests stop carrying ids for these rows; Rust computes them and returns them in receipts. Roots, playlists, presets, user soundscapes, and snoozes keep random UUID v4 keys, because two devices creating "the same" playlist or root are two user decisions. `MAX_ID_BYTES` becomes 128. Rows created before this milestone keep their keys: every replica descends from one database, so legacy keys exist identically everywhere, and imports already match by identity key before creating. The derived form is enforced on new rows by the writers, not by the adapter.

The six composite-key tables (50, 51, 54, 56, 61, 62) follow the milestone 2 composite row key convention: each gains `sync_key TEXT NOT NULL UNIQUE`, the manifest row key, computed by the identity module through `ganbaru_sync_contracts::derived_key` with a per-table prefix (`music-signal`, `music-statistics`, `music-collection-item`, `music-intended-use`, `music-preset-sound`, `music-assignment`) over the primary key columns in declaration order. Each declares a derived key with an existence group (a `Register` with no domain column, listed with its own group id), so remove then add again works, and an Immutable `identity` group holding its primary key columns, so a remote create materializes them. The composite primary key stays; it is implied by `sync_key`, so its duplicate repair keeping the lowest key is unreachable. Existing rows get a placeholder in SQL and their derived value from the Music conversion pass (see [Migrations](#migrations)).

Relative paths are stored with forward slashes. Path normalization and relink never update `relative_path` in place; they tombstone the old location and create the new one, because the path is part of the key.

### Writer rules

- No Music writer deletes all children and reinserts them. Signals, intended uses, preset sounds, assignment phases, and import replace compute the difference and insert or delete only what changed.
- Every update skips rows whose values are unchanged (`WHERE (...) IS NOT (...)`), so a no-op save captures nothing.
- Desktop and Android refresh write portable item and location values only when the row is new or this device's local size or mtime changed. The fingerprints written are content-based, so a device applying another device's fingerprints does not rewrite them; this removes scan ping-pong between devices with different copies.
- `ensure_built_in_music_playlists` and its call in `queries.rs:432` are removed. Seeds come from migrations and exist identically everywhere; a test asserts they exist after migrations. On a replica, the read-path `INSERT OR IGNORE` would also fire guard triggers.
- Playlist and preset sound reorder becomes a key-between insert of the moved row, like Quick notes reorder.
- `duplicate_playlist` computes membership keys in Rust.
- Playlist delete no longer writes `projects` or `calendar_events`. The replacement applies to `music_context_assignments` only. The impact still counts projects and events, and the dialog says they fall back to no playlist. Readers of those legacy columns already treat an unknown playlist id as none. Milestones 6 and 7 own those columns.

### Unbound roots and availability

- A root without a binding on this device reports a new binding status `unbound`. The Sources view shows "Not on this device" with Choose folder; its items stay listed but are not playable here. Binding uses the existing app-state binding and runs the existing refresh.
- `availability` on items and locations is a Local column observed by this device. A remote item arrives as `unknown`.
- After apply, the Music post-apply hook reports the roots that received new or changed locations through its change sink, and the Music runtime schedules one debounced refresh (2 s) of each of those roots that is bound on this device. The refresh is incremental through the size and mtime skip above, so it only stats files.
- YouTube eligibility reads the replicated `youtube_resolution_state` instead of `availability`: `embedding-blocked` and `unavailable` block, `ready` plays, and `resolving`, `timed-out`, or null attempt playback, replacing the availability filter in `lib/music/sources/playback.ts:15` (available or unknown) and the timed-out block in `lib/music/playlists/playback.ts:97`, which both move to this rule. A transient state written by one device therefore never blocks another, and a video resolved on one device plays on every online device.

### Listening statistics

`music_listening_statistics` is rebuilt with `PRIMARY KEY (device_id, item_id)`, keyed by the device id sync membership pairs with this installation, never the writer id (the milestone 7 rule). In source that is the app-state `ensure_device_id` value, which pairing and session checkpoints already use. Each device increments only its own row; every count and `last_played_at_ms` merges with `Max`. Reads sum counts and take the maximum `last_played_at_ms` per item. Existing rows move to the fixed bucket `vault`, which no device writes again. A cloned installation that shares a device id can only undercount under `Max`; it never corrupts.

### Artwork

- `AssetClass { name: "music/artwork", max_bytes: 10 MiB }` in the Music `DomainManifest`; the engine's path function names the files. Accepted types are PNG, JPEG, WebP, and GIF, checked by `ganbaru-image-metadata` through `parse_display_image_metadata` (the only parser that accepts GIF) with its dimension limits; `music_pick_artwork_file` (`app/src/music.rs:333-336`) drops BMP and AVIF from its filter. The adapter's `asset_refs` returns the reference in the `artwork` group, so every device fetches it eagerly, and milestone 10's sweep deletes bytes no row or stored version references.
- `music_pick_artwork_file` copies the picked image into the asset folder and returns the managed reference `music/artwork/{sha256}.{ext}`. `artwork_override` accepts only that form; validation rejects anything else.
- The artwork migration sets every existing `artwork_override` to null; no UI writes it today.
- Readers (`list-artwork.ts:14`, `review.ts:42`, `review.ts:56`, `sources/playback.ts:33`, `playlists/playback.ts:176`) resolve managed references through the asset protocol.

### Playlist icons

Playlist and preset icons stay opaque strings validated by `validate_project_icon`. A custom emoji or project icon asset referenced from another device renders the existing fallback until milestone 6 replicates those assets.

### Group tables

`music_local_roots` (table 48).

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| created | 0 | `created_at_ms` | Immutable | No |
| name | 1 | `name` | Register | No |
| updated | 2 | `updated_at_ms` | Max | No |

`music_library_items` (table 49). Local columns: `availability`, `original_artwork_identity`, `version`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `source_kind`, `identity_key`, `youtube_video_id` | Immutable | No |
| discovered | 1 | `discovered_at_ms` | Register | No |
| source | 2 | `media_kind`, `original_title`, `original_artist`, `original_album`, `original_track_number`, `duration_ms`, `youtube_resolution_state` | Register | No |
| title | 3 | `title_override` | Register | No |
| artist | 4 | `artist_override` | Register | No |
| album | 5 | `album_override` | Register | No |
| artwork | 6 | `artwork_override` (managed asset reference) | Register | No |
| review | 7 | `review_state`, `review_changed_at_ms`, `review_deferred_until_ms` | Register | No |
| updated | 8 | `updated_at_ms` | Max | No |

- The source-kind CHECK lives inside the identity group. Resolutions: `UNIQUE (identity_key)` and `UNIQUE (youtube_video_id)` use duplicate repair keeping the lowest key; with the identity module they are reachable only through legacy keys.
- Description limit triggers stay; writers truncate before writing, and a conformance assertion checks that every encoded source value is within 500 characters, so apply never hits the trigger.

`music_item_signals` (table 50): row key `sync_key` over `(item_id, signal)`; `item_id`, `signal` Immutable (identity, group 0), `created_at_ms` Register (group 1, because a re-add seals every group again and Immutable groups on derived-key tables hold only key columns), and the existence group (group 2).

`music_listening_statistics` (table 51): row key `sync_key` over `(device_id, item_id)`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `device_id`, `item_id` | Immutable | No |
| played | 1 | `last_played_at_ms` | Max | No |
| plays | 2 | `play_count` | Max | No |
| completions | 3 | `completion_count` | Max | No |
| skips | 4 | `skip_count` | Max | No |
| updated | 5 | `updated_at_ms` | Max | No |
| exists | 6 | None (existence) | Register | No |

`music_local_locations` (table 52). Local columns: `file_size_bytes`, `modified_at_ms`, `availability`, `last_seen_generation`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `root_id`, `relative_path` | Immutable | No |
| item | 1 | `item_id` | Reference to table 49, not null | No |
| discovered | 2 | `first_seen_at_ms` | Register | No |
| fingerprint | 3 | `lightweight_fingerprint`, `strong_fingerprint` | Register | No |
| updated | 4 | `updated_at_ms` | Max | No |

Resolutions: `UNIQUE (root_id, relative_path)` and `UNIQUE (root_id, item_id, relative_path)` use duplicate repair keeping the lowest key; the first is implied by the derived key for new rows. A concurrent relink and scan of the same new path converge through the `item` register; the losing item shows missing with Relink.

`music_source_collections` (table 53). Local columns: `refresh_state`, `last_successful_refresh_at_ms`, `previous_successful_refresh_at_ms`, `last_refresh_error_code`, `snapshot_generation`, `version`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `kind`, `identity_key`, `local_root_id`, `youtube_playlist_id` | Immutable | No |
| created | 1 | `created_at_ms` | Register | No |
| name | 2 | `name` | Register | No |
| discovery | 3 | `discovery_enabled` | Register | No |
| removal | 4 | `removed_at_ms` | Register | No |
| updated | 5 | `updated_at_ms` | Max | No |

The kind CHECK is inside the identity group. `UNIQUE (identity_key)`, `UNIQUE (youtube_playlist_id)`, and `UNIQUE (local_root_id)` use duplicate repair keeping the lowest key; all three follow from the derived key for new rows.

`music_source_collection_items` (table 54): row key `sync_key` over `(collection_id, item_id)`. Local columns: `last_seen_generation`, `missing_from_latest_snapshot`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `collection_id`, `item_id` | Immutable | No |
| discovered | 1 | `first_discovered_at_ms` | Register | No |
| position | 2 | `source_position` | Register | No |
| exists | 3 | None (existence) | Register | No |

`music_playlists` (table 55).

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| created | 0 | `created_at_ms` | Immutable | No |
| name | 1 | `name` | Register | Yes |
| icon | 2 | `icon` | Register | Yes |
| playback | 3 | `shuffle_enabled`, `repeat_mode`, `mix_enabled` | Register | Yes |
| order | 4 | `order_key` | Position | No |
| updated | 5 | `updated_at_ms` | Max | No |

A playlist deleted while another device edits it or its memberships becomes one recovery entry. Restore creates a playlist with a new key and copies of the concurrently edited memberships.

`music_playlist_intended_uses` (table 56): row key `sync_key` over `(playlist_id, intended_use)`, `playlist_id`, `intended_use` Immutable (identity, group 0) and the existence group (group 1), recoverable with its playlist.

`music_playlist_memberships` (table 57): row key `id` (the membership `stable_id`), declared `derived_key` with the existence group, so remove then add again works.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `playlist_id`, `item_id` | Immutable | Yes |
| created | 1 | `created_at_ms` | Register | No |
| order | 2 | `order_key` | Position | No |
| weight | 3 | `weight` | Register | Yes |
| enabled | 4 | `enabled` | Register | Yes |
| range | 5 | `start_ms`, `end_ms`, `volume_milli`, `rate_milli` | Register | Yes |
| break item | 6 | `break_item_id` | Reference to table 49, null allowed | Yes |
| break range | 7 | `break_start_ms`, `break_end_ms`, `break_volume_milli`, `break_rate_milli` | Register | Yes |
| skips | 8 | Owned `music_membership_skip_ranges` (`start_ms`, `end_ms` Integer, `max_rows` 64, `codec: Generic`) | Register | Yes |
| updated | 9 | `updated_at_ms` | Max | No |
| exists | 10 | None (existence) | Register | No |

- Both range CHECKs live inside their groups. Writers clear the break range together with the break item; a break range without a break item is ignored by playback.
- The skip list is canonical: writers store it sorted by `(start_ms, end_ms)`, non-overlapping, with ordinals from 0, and reject a 65th entry, so the generic codec round-trips it byte-exactly and materialize reinserts the same rows.
- `UNIQUE (playlist_id, item_id)` uses duplicate repair keeping the lowest key; reachable only when a legacy key and a derived key meet after a restore.

`music_snoozes` (table 58).

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| target | 0 | `item_id`, `scope`, `playlist_id` | Immutable | No |
| created | 1 | `created_at_ms` | Register | No |
| window | 2 | `starts_at_ms`, `ends_at_ms` | Register | No |
| reason | 3 | `reason` | Register | No |

The scope CHECK lives inside the target group; both target references cascade.

`music_soundscapes` (table 59). Local columns: `availability`, `version`.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `source_kind`, `generated_kind`, `bundled_identity` | Immutable | No |
| created | 1 | `created_at_ms` | Register | No |
| name | 2 | `name` | Register | No |
| icon | 3 | `icon` | Register | No |
| updated | 4 | `updated_at_ms` | Max | No |

The partial `UNIQUE (generated_kind)` uses duplicate repair keeping the lowest key; generated soundscapes are seeds, so it is unreachable. A local loop from another device shows "Not on this device" with Choose file, using the existing app-state path map.

`music_soundscape_presets` (table 60).

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| created | 0 | `created_at_ms` | Immutable | No |
| name | 1 | `name` | Register | No |
| icon | 2 | `icon` | Register | No |
| level | 3 | `level_milli` | Register | No |
| updated | 4 | `updated_at_ms` | Max | No |

`music_soundscape_preset_sounds` (table 61): row key `sync_key` over `(preset_id, soundscape_id)`, both cascading.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `preset_id`, `soundscape_id` | Immutable | No |
| order | 1 | `order_key` | Position | No |
| level | 2 | `level_milli` | Register | No |
| exists | 3 | None (existence) | Register | No |

The 16-layer cap is enforced on local insert only; concurrent additions can exceed it, and playback mixes the first 16 by order key.

`music_context_assignments` (table 62): row key `sync_key` over `(owner_kind, owner_id, phase)`. Owners stay unchecked: owner domains delete their rows, and orphans are never read because every read starts from an owner.

| Group | Id | Columns | Kind | Recoverable |
| --- | --- | --- | --- | --- |
| identity | 0 | `owner_kind`, `owner_id`, `phase` | Immutable | No |
| behavior | 1 | `behavior` | Register | No |
| playlist | 2 | `playlist_id` | Register | No |
| soundscape behavior | 3 | `soundscape_behavior` | Register | No |
| preset | 4 | `soundscape_preset_id` | Register | No |
| provenance | 5 | `provenance_kind`, `provenance_id` | Register | No |
| updated | 6 | `updated_at_ms` | Max | No |
| exists | 7 | None (existence) | Register | No |

`playlist_id` and `soundscape_preset_id` stay opaque registers, not references: a dangling id is reported as unavailable, which is the current, intended behavior, and a reference would silently turn it into "none".

### Migrations

`{ts}_music_sync_shape.sql`, one transaction, foreign keys on:

1. Playlists, additively: drop `idx_music_playlists_sort`, add `order_key TEXT NOT NULL DEFAULT ''` with a length CHECK, fill rank keys by `(sort_order, name COLLATE NOCASE, id)`, drop `sort_order`, create the index on `(order_key, id)`, and add an empty-key trigger.
2. Memberships, additively: drop the three position indexes, add and fill `order_key` per playlist by `(position, id)`, drop `position`; add `volume_milli` and `rate_milli` with their CHECKs, fill them with `CAST(round(x * 1000) AS INTEGER)`, drop `volume` and `rate`; add `break_item_id TEXT REFERENCES music_library_items(id) ON DELETE SET NULL` and the four break columns (integer thousandths for volume and rate) with their CHECKs, copy from `music_membership_break_items`, drop that table and its index; recreate the indexes on `order_key`, and add `idx_music_playlist_memberships_break_item`.
3. Skip ranges, rebuilt (no children): `PRIMARY KEY (membership_id, ordinal)`, no `id` or `sort_order`, ordinals from `row_number() - 1` by `(start_ms, end_ms)`, and `CHECK (ordinal BETWEEN 0 AND 63)`.
4. Presets, additively: add and fill `level_milli`, drop `level`. Preset sounds, rebuilt (no children): `PRIMARY KEY (preset_id, soundscape_id)`, `order_key` filled by `position`, `level_milli` from `level`.
5. Listening statistics, rebuilt (no children): `device_id TEXT NOT NULL CHECK (trim(device_id) <> '')`, `PRIMARY KEY (device_id, item_id)`, existing rows under `vault`, and an index on `item_id`.

`{ts}_music_artwork_assets.sql` sets `artwork_override` to null.

`{ts}_music_sync_capture.sql` adds `sync_key TEXT NOT NULL` to the six composite-key tables, fills existing rows with the placeholder `'pending:' || rowid`, creates a unique index on it, and installs the capture triggers rendered from the manifest. It seeds no captures. The Music conversion pass (`ganbaru-music-library` `identity.rs`, called by the app at vault open) rewrites every placeholder to its `derived_key` value with plain updates, which the key immutability guard allows for `pending:` keys; the value is deterministic on every device because all replicas descend from one database, and milestone 2's generic startup check refuses any remaining placeholder. Milestone 2's `seed_captures` then seeds tables 48 to 62 at the same vault open, after every conversion pass, so no capture names a placeholder. Migrations run on the owner only; guarded replicas receive the new shape through refresh.

### Adapter and hooks

- `crates/ganbaru-sync/src/manifest/vault/music.rs` declares the tables, groups, resolutions, and Local columns; `src/domains/music.rs` holds the value codecs and `asset_refs` for the artwork group; the skip list uses the generic owned codec. The `DomainManifest` declares the artwork asset class. Music adds its line to `VAULT_DOMAINS` and `vault_adapters`, and `VAULT_MANIFEST_VERSION` goes up by one.
- `apps/client/src-tauri/app/src/music/sync.rs` registers two post-apply hooks: search index refresh for changed item keys (a new `ganbaru-music-library` function that rebuilds FTS rows by id), and the changed-root report that feeds the refresh scheduler. Hooks receive only the connection and a change sink, never an `AppHandle`, so they also run in milestone 3's headless Android pass; the scheduler subscribes to the sink in the Music runtime.
- Domain triggers: the description limit triggers and the empty order key trigger are declared `ApplierVisible` (abort checks with no writes); Music has no other non-engine triggers.
- Music sets its milestone 2 per-domain replica write flag, so its commands run on replicas once the domain is converted. Session commit stays legal on replicas because it writes Local tables plus the device's own statistics row.
- Music recovery strings go in `apps/client/src/lib/i18n/messages/{en,es}/music.ts`.

### Frontend

- Binding status `unbound`, with "Not on this device" and Choose folder in Sources and the item inspector.
- YouTube eligibility from resolution state in `lib/music/playlists/playback.ts` and `lib/music/sources/playback.ts`.
- Source creation and membership requests stop sending ids; volume, rate, and level travel as integer thousandths.
- Library change notices from milestone 2 refresh the Music projections, matching the existing `notifyMusicLibraryChanged` path.

## Testing

- Conformance for tables 48 to 62: every column classified, every UNIQUE and multi-column CHECK resolved, Local columns defaulted, `sync_key` on every composite-key table, references pointing to lower TableIds, codec round trips byte-exact.
- Conversion pass tests: placeholders on a migrated dense fixture become the values the writers compute for the same rows, a second run changes nothing, and engine startup refuses a vault with a remaining placeholder.
- Migration tests on the dense fixture: playlist and membership order preserved, break items folded, skip range ordinals, preset order, volume, rate, and level converted to thousandths, statistics totals unchanged, seeds intact, `PRAGMA foreign_key_check` empty, and every FTS match surviving.
- Identity tests: the module reproduces existing scanned ids exactly; desktop and Android refresh, YouTube, and import produce identical keys for the same content.
- Writer tests: diffs instead of delete-and-reinsert (counted through `sync_capture`), no-op saves capture nothing, and an unchanged rescan of the dense root captures zero rows on desktop and Android paths.
- Simulation seeds: concurrent discovery of the same file, relink against scan, concurrent add of the same membership and same YouTube playlist, remove then add again, concurrent reorder, playlist delete against membership add with recovery and restore, soundscape delete against preset edit, statistics from three devices, assignment edits against playlist delete with replacement.
- Frontend: unbound status mapping, YouTube eligibility, aggregated statistics parsing.

## Work breakdown

Steps land on `feat/sync` under the [working agreement](README.md#working-agreement).

| Step | Worktree branch | Contents | Focused checks |
| --- | --- | --- | --- |
| 1 | `feat/sync-music-writers` | Identity module; ids computed in Rust; `MAX_ID_BYTES` 128; diff writers; unchanged-value skips; incremental desktop and Android refresh; location remove and add on normalization and relink; built-in repair removed; playlist delete limited to assignments. | `cargo test -p ganbaru-music-library --lib -j 4 identity -- --test-threads=4`, `cargo test -p ganbaru-music-library --lib -j 4 writes -- --test-threads=4`, `cargo test -p ganbaru-music --lib -j 4 assignments -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 music -- --test-threads=4` |
| 2 | `feat/sync-music-shape` | Shape migration; order-key reorder; folded break items; owned skip ranges; integer thousandths in writers and readers; per-device statistics with aggregated reads; schema tests. | `cargo test -p ganbaru-db --lib -j 4 music -- --test-threads=4`, `cargo test -p ganbaru-music-library --lib -j 4 -- --test-threads=4` |
| 3 | `feat/sync-music-artwork` | Asset class, picker copy, managed reference validation, artwork migration, readers on the asset protocol. | `cargo test -p ganbaru-music-library --lib -j 4 artwork -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 music -- --test-threads=4`, `pnpm --dir apps/client exec vitest run src/lib/music/review.test.ts --maxWorkers=1` |
| 4 | `feat/sync-music-replication` | `sync_key` columns, writers filling them, conversion pass, manifest, adapter, artwork `asset_refs`, capture migration (seeding is milestone 2's `seed_captures`), post-apply hooks, replica write flag, conformance, simulation seeds, budgets test. | `cargo test -p ganbaru-music-library --lib -j 4 identity -- --test-threads=4`, `cargo test -p ganbaru-sync -j 4 music -- --test-threads=4`, `cargo test -p ganbaru-sync -j 4 conformance -- --test-threads=4`, `cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4` |
| 5 | `feat/sync-music-ui` | Unbound status, YouTube eligibility, request contract changes, recovery strings, i18n. | `pnpm --dir apps/client exec vitest run src/lib/music/playlists/playback.test.ts --maxWorkers=1`, focused Vitest for `lib/music/sources` and the Music API, `pnpm --dir apps/client run check` |
| 6 | `docs/sync-milestone-5` | Final docs, acceptance mapping to automated coverage, goal README update. | `pnpm -w run validate` |
| 7 | None | Physical acceptance on A, B, and P, recorded here. | None |

Steps 1 to 3 are useful without sync and land in order; step 3's asset class needs milestone 2 step 8, and step 4 needs milestone 2.

## Documentation

- `docs/data/sync.md`: the Music row in storage ownership (portable library, playlists, assignments, statistics per device; bindings, availability, paths, sessions device-local) and the delivery status row once Music replicates.
- `docs/data/schema/supporting-domains.md` Music section: order keys, folded break items, owned skip ranges, integer thousandths for volume, rate, and level, per-device statistics, derived identities and `sync_key` on composite-key tables, Local columns.
- `docs/data/schema/sync.md`: Music TableIds 48 to 62 in the domain table list.
- `docs/features/music/library-and-sources.md`: unbound roots on linked devices, availability per device, artwork as managed assets.
- `docs/features/music/playlists-and-review.md`: concurrent edits, playlist recovery, statistics across devices.
- `docs/features/music/automation.md`: playlist delete replacement limited to Music assignments; projects and events fall back to no playlist.
- `docs/features/music/playback.md` and `soundscapes.md`: YouTube eligibility, local loops on other devices, layer cap under concurrency.
- `docs/features/README.md` Music row, and the `AGENTS.md` vault map assets list (`music/artwork/`).
- `docs/testing/README.md` only if new focused commands or simulation seed variables are added.

## Manual acceptance

Two desktops (A is the coordinator, B is linked) and one Android phone (P). A and B both bind the same root to a copy of the same folder; P has the root unbound unless stated.

| Scenario | Expected |
| --- | --- |
| A rescans an unchanged root | No sync activity; status shows nothing pending. |
| A adds a new file to its folder and refreshes | The item appears on B and P; B marks it available after its automatic refresh if the file exists there; P shows the source as Not on this device. |
| A and B both add the same new file and refresh offline | After reconnect, one item everywhere. |
| P adds a YouTube video, resolves it, and adds it to a playlist | It plays on A and B when online, with no refresh on those devices. |
| A and P add the same YouTube playlist offline | One source everywhere. |
| A and P reorder the same playlist offline | Same order on every device. |
| A deletes a playlist while P adds two songs to it offline | P's change becomes a recovery entry; Restore brings the playlist back with those songs on every device. |
| B and P set different weights on the same membership offline | Same weight everywhere, no conflict shown. |
| A removes an item from a playlist and adds it again while B edits its range | One membership everywhere; the result follows the merge rules without duplicates. |
| Play songs on A, B, and P | Play counts on every device equal the sum of plays. |
| P binds the root to a folder | Items become playable on P after its refresh; nothing changes on A or B. |
| B sets a Focus phase playlist; A deletes that playlist choosing a replacement | The phase uses the replacement everywhere, or reports the playlist as unavailable if B's change arrived later. |
| A deletes a soundscape used by a preset edited on P | The layer is gone from the preset everywhere. |

## Performance budgets

Measured on the dense Music fixture (20,000 items, 20,000 locations, collection items, playlists with memberships) in the release profile, and on the acceptance devices.

| Measure | Budget |
| --- | --- |
| Captures from an unchanged rescan | 0 |
| Incremental desktop refresh of an unchanged 20,000-file root | Under 3 s |
| First publication seal of the dense fixture | Under 15 s desktop, no single seal transaction over 500 ms |
| Operation log growth from the first publication | Under 25 MiB |
| Apply of the first publication on P | Under 60 s in the background, no apply transaction over 500 ms |
| Library list with aggregated statistics | No more than 10% slower than before the milestone |
| Post-apply search index refresh of 1,000 changed items | Under 200 ms desktop |

## Risks

| Risk | Mitigation |
| --- | --- |
| A large library floods the operation log on first publication | Unchanged-value skips, per-device observations as Local columns, milestone 2's batched sealing, and a measured budget before step 4 merges. |
| Scans on two devices with different file copies fight over values | Writers only write after a local size or mtime change; device observations are Local. |
| Legacy and derived keys for the same identity meet | Imports and scans match by identity first; duplicate repair covers the remaining restore case. |
| Removing the cross-domain playlist rewrite leaves projects and events on a deleted playlist | Readers already treat it as none; the dialog says so; milestones 6 and 7 own those columns with the same rule. |
| Desktop and Android refresh drift apart | Both call the identity module and share the incremental rule; one test feeds the same media list to both. |
| Concurrent layer additions exceed 16 | Playback mixes the first 16 by order; the editor shows the excess and lets the user remove it. |

## Deferred

- Artwork override UI.
- Pruning of session and transfer receipts, which are Local and unaffected by sync.
- Surfaced conflicts for metadata overrides, if silent registers prove confusing.
- Statistics compaction across retired device ids.

## Implementation checks

Each check is already decided: the plan takes the primary branch of its rule, and only the named fallback may replace it. The tag says what confirms it: **Runtime** (OS, device, or library behavior, checked on the acceptance matrix), **Vault data** (the maintainer's vault, checked before the step merges), or **Earlier step** (code this or an earlier milestone writes, checked by that step's tests). The implementer records which branch held.

- **Runtime:** Whether a device that did not resolve a YouTube video retries resolution when it sees `resolving` or `timed-out` from another device. If it does not, start resolution on play for those states.
- **Runtime:** `ADD COLUMN` with `REFERENCES ... ON DELETE SET NULL` and column CHECKs that reference sibling columns on the bundled SQLite. If a sibling reference is refused, enforce the break range order with a trigger instead.
- **Vault data:** The longest existing key per Music table in the maintainer's vault. If any table other than memberships has keys over 128 bytes, add a rekey for that table only to the shape migration; if memberships do, rekey memberships and their skip ranges there.
- Settled from source: checkpoint payloads store `membership_id` (`crates/ganbaru-music-library/src/session/models.rs:126`) inside opaque `checkpoint_json` (B:2092). A shape migration that rekeys memberships deletes every `music_session_checkpoints` row in the same migration; they are Local, and the only cost is one lost resume position.
- **Runtime:** Whether playback refuses `unknown` local items that exist on disk. If it does and the scheduled refresh takes over 3 s, add an on-demand stat at play time.
- **Runtime:** Case-insensitive path collisions on Windows (two paths differing only by case on Linux). If a Windows refresh would remove and add such locations each scan, compare paths case-sensitively and mark the collision as ambiguous locally.

## Progress

- [x] Step 1: identity module and writers.
  - 2026-10-22, branch `feat/sync-music-writers` (from `feat/sync` at 2ac14c10), commits 210aea82 (identity module), 628e12f6 (derived keys, diff writers, incremental refresh, delete scope, docs), and 93a60eaa (delete test covering untouched project and event rows with impact counts).
  - Checks: full `ganbaru-music-library` lib suite (143 passed, includes identity, writes, local_scan, local_refresh, relink), `ganbaru-music` assignments (5), `ganbaru-tauri-app` music (141), clippy `-D warnings` on the three crates with all targets, `cargo fmt --check`, `cargo check -p ganbaru --bin ganbaru`, `pnpm --dir apps/client run check`, focused Vitest (review and sources controllers, inspector, contracts, youtube-adapter, i18n catalog shape) all pass.
  - Implementation checks: id derivation and receipt return held as planned; vault-data key length and runtime capture checks are not verifiable before step 4 (no `sync_key` or capture yet). Android `mobile_refresh.rs` is not compiled on this host (no Android target); its logic moved into `local_scan::persist_scanned_file`, which is tested on the host.
  - No `ganbaru-sync-contracts` dependency (no `derived_key` call); `MAX_ID_BYTES` 128 lives in music-library validation only.
  - Skip range ids derive from `(membership_id, sort_order)`; a no-op membership save returns the current version; `MusicCollectionWrite` drops `identity_key`; YouTube collections match by `youtube_playlist_id`, then identity key; import keeps playlist copy ids from the operation; the review controller maps optimistic ids to receipt ids; preset sounds diff through parked positions.
  - Incremental refresh: unknown size or mtime never matches, so such files are always inspected; identity repair only runs on inspected files; device observations never bump `updated_at_ms`; item version is not bumped for availability; collection refresh state still bumps `version` but not `updated_at_ms`.
  - Relink with no fingerprint keeps the stored one; a relinked location gets a new derived key and a NULL mtime so the next refresh inspects it.
  - Android search rebuild is skipped when nothing was written. Android `saf:` fingerprints and embedded artwork identity are unchanged (artwork is step 3).
  - Playlist delete leaves `projects` and `calendar_events` untouched; the planned dialog fallback copy was pulled forward into `music.builder.deletePlaylistWarning` (en, es), so step 5 does not need to redo it.
- [x] Step 2: shape migration.
  - 2026-10-22, branch `feat/sync-music-shape` (from feat/sync at 2bc72780), commit 876437f7.
  - Implementation checks: the `ADD COLUMN ... REFERENCES ... ON DELETE SET NULL` plus sibling-column CHECK branch held on the bundled SQLite (primary branch, no trigger fallback). No rekey was needed, so `music_session_checkpoints` rows are kept. Vault-data key length and the runtime checks remain for the maintainer's vault and the acceptance matrix.
  - Migration `20261022091235_music_sync_shape.sql`: order keys on playlists, memberships, and preset sounds (empty-key insert and update triggers on playlists and memberships, so raw inserts must supply `order_key`); break item folded onto the membership; skip ranges keyed by `(membership_id, ordinal)` with ordinal 0..63 and no id; volume, rate, and level as integer thousandths; statistics keyed by `(device_id, item_id)` with legacy rows in a `vault` bucket and reads aggregated by SUM and MAX.
  - Order keys reuse the sync-contracts `OrderKey` with a new `order_placement` helper (LIS reorder returning only changed keys); quick-notes order uses it too. `ganbaru-music-library` now depends on `ganbaru-sync-contracts`.
  - Playlist reorder returns `MusicOrderReceipt { id, orderKey, version }`; the builder optimistically reuses the existing sorted keys and applies receipt keys on success. Versions bump only for moved playlists.
  - Frontend: membership wire types use `volumeMilli` and `rateMilli`, converted by the playback reader; soundscape preset domain types keep multipliers and convert at the command boundary (`presetWriteWire`, `levelMilli`). `compareCodeUnits` moved to `utils/order-key.ts`; ordering is by order key then id, with no name tiebreak.
  - `from_thousandths` is public in music-library. Interchange version is 2. The transfer allowance test uses snoozes. The benchmark dataset version was not bumped.
- [x] Step 3: artwork assets.
  - 2026-10-22, branch `feat/sync-music-artwork` (from `feat/sync` ae364e50), commits 12f7d546 `feat(music): store artwork overrides as managed vault assets` and ec8104f0 `docs(music): describe managed artwork assets`.
  - Implementation checks: the sibling-column CHECK on `ADD COLUMN` held again on the bundled SQLite (`artwork_override_byte_size` checks `artwork_override`). The migration nulls overrides before the `ALTER`, because SQLite tests the new CHECK against existing rows.
  - Decision: `AssetRef` needs a byte size, so `music_library_items.artwork_override_byte_size` sits beside `artwork_override` with a pair CHECK (both null, or a `music/artwork/` reference with 1 to 10 MiB). `set_metadata_overrides` fills it from the verified vault file, so the frontend contract is unchanged. Step 4's artwork group should carry both columns.
  - Decision: the Music `DomainManifest` (TableIds 48 to 95) declares only the artwork class and no tables, and no Music adapter is registered (`Engine::with` refuses adapters without tables). `VAULT_MANIFEST_VERSION` stays as is; step 4 bumps it when tables and `asset_refs` arrive.
  - Decision: "asset protocol" means the app's command-returned data URLs through `lib/api/asset-url-cache.ts` (kind `music-artwork`, command `music_artwork_asset_data_url`) plus `music_register_artwork_asset` for hosted playback artwork on desktop. The Tauri `asset:` protocol, capabilities, and scope are unchanged; only the two commands were added to `permissions/app-commands.toml`. Managed references skip `confirmLocalMusicSources` and record no path grant.
  - Android picker path (`artwork_data_url` then import) is not compiled on this host.
  - Orchestrator follow-up: 2dd9776c `fix(music): read stored artwork off the async worker`; moves the artwork read and hash in `set_metadata_overrides` into `spawn_blocking` (tokio moves from dev to normal dependency of `ganbaru-music-library`).
- [x] Step 4: replication.
  - Date: 2026-10-22. Branch `feat/sync-music-replication` (worktree agent-a7d9aa985f7a52767), merged current `feat/sync` with a signed merge (no rebase).
  - Commits: 91494d4f `feat(sync): add optional integer column type` (authorized `ColumnType::OptionalInteger { min, max }`), b2bf53a7 `feat(sync): replicate music library rows`, d2ce857c merge of `feat/sync`, de415b6b `feat(sync): refresh music search and roots after apply`.
  - Manifest: Music tables 48 to 62 under vault manifest version 5 (Contacts took 4), recorded in `vault_manifest_v5.txt`; artwork overrides replicate as `music/artwork` asset refs; capture triggers come from the manifest and seeding from milestone 2's `seed_captures`.
  - Implementation check branch: the checkpoint rule held (moved memberships drop Music session checkpoints, settled from source); runtime and vault data checks were covered by focused tests only, not by physical devices.
  - Decision: membership identity is non-recoverable. Membership ids are `derived_key(MEMBERSHIP_PREFIX, [playlist_id, item_id])`; the conversion pass rekeys legacy ids by delete and reinsert under the applying flag (triggers stay untouched).
  - Decision: recovery restores a deleted playlist with concurrent edits as a new playlist with those memberships; library item recovery offers discard only.
  - Decision: post-apply hooks refresh search rows for changed items and mark the index stale on child deletions; desktop schedules a debounced refresh of bound roots whose locations changed. Replica writes in the Music session go through `can_write_domain` and `acquire_replicated_write`.
  - Deviation: simulation seeds are deterministic multi-replica scenario tests in `crates/ganbaru-sync/src/tests/music.rs` instead of extending `sim.rs`, which models only the fixture domain.
  - Gaps: Android has no post-apply root refresh (availability catches up at the next refresh) and its paths were not compiled on this host; frontend unbound-root status, YouTube eligibility, and recovery i18n strings are left for step 5.
  - Budgets: unchanged rescan emits zero captures (tested); post-apply search refresh of 1,000 items under 200 ms is an ignored release-only test, not run here.
  - Checks: ganbaru-sync all (190 passed, conformance included), ganbaru-music-library lib (164), ganbaru-music, ganbaru-db, ganbaru-tauri-app `sync`, `music`, `vault`, `recovery` filters, clippy `--all-targets -D warnings` on touched crates, `cargo fmt --check`, and `cargo check -p ganbaru --bin ganbaru` all pass after the merge.
- [x] Step 5: frontend.
  - 2026-10-22, branch feat/sync-music-ui, commit 06865317 "feat(music): show unbound roots, gate YouTube on resolution state, add recovery copy".
  - Root binding status renamed needs-relink to unbound (Rust and TypeScript); Sources and the review inspector show "Not on this device" with Choose folder.
  - YouTube eligibility is derived from resolution state (embedding-blocked and unavailable block). Rust projection now carries youtubeResolutionState and the native queue treats resolved YouTube rows as available (row_availability in session/queue.rs).
  - Request contract (no ids on source creation and memberships, integer thousandths) was already done by steps 1 and 2; verified against contracts.
  - Recovery presentation for Music (playlist restores as a new playlist; library items discard only) via registry note and restorable options, with en and es strings.
  - Applied-sync notices for Music tables refresh sources and call notifyMusicLibraryChanged (first-use-preload).
  - Checks held: cargo fmt, ganbaru-music-library lib tests (166), ganbaru-tauri-app root_binding tests, clippy -D warnings for both crates, focused vitest (music, sync, i18n, components/music), client svelte-check (0 errors), editor-check.
- [x] Step 6: docs and validation.
  - 2026-10-22, same branch feat/sync-music-ui (one branch for steps 5 and 6), commit 35b705cd "docs(music): describe unbound roots, YouTube eligibility, and replicated soundscapes".
  - Steps 1 to 4 already wrote: sync.md storage row and assets class, schema/sync.md TableIds 48 to 62, supporting-domains Music section, playlists-and-review recovery, library-and-sources unbound text, AGENTS.md music/artwork/ entry. Gaps filled: sync.md delivery row now Implemented, unbound label in library-and-sources, YouTube eligibility in playback.md, replacement across devices in automation.md, linked-device layers and the 16-layer cap in soundscapes.md. features/README.md Music row stays Partial (no change needed). docs/testing/README.md unchanged (no new commands or seed variables).
  - Validation (full validate) left to the orchestrator.
  - Acceptance mapping (rewritten by the review fixes on fix/sync-music-review, commit 6fb54fb3 for the new scenario tests). Every row has a deterministic test; "physical" names what still needs a manual check on real devices.
    - Unchanged rescan, no sync activity: crates/ganbaru-music-library/src/local_refresh/tests.rs unchanged_rescan_writes_only_device_observations; crates/ganbaru-sync/src/tests/capture.rs local_and_derived_columns_and_unchanged_values_are_not_captured. Physical: a real rescan on a bound root.
    - New file added on A appears on B and P with location availability unknown: crates/ganbaru-sync/src/tests/music.rs music_new_file_reaches_other_devices_unobserved_and_their_observations_stay_local; apps/client binding-status.test.ts (unbound label). Physical: the Not on this device label in the UI.
    - P binds the root and refreshes (availability and generation), no captures, B unchanged: tests/music.rs music_new_file_reaches_other_devices_unobserved_and_their_observations_stay_local (second half); apps/client binding-status.test.ts and sources controller.test.ts. Physical: binding and refresh on the device.
    - Same new file added on A and B offline: tests/music.rs music_concurrent_discovery_of_one_file_settles_on_one_item. Physical: none beyond discovery itself.
    - P adds and resolves a YouTube video, plays on A and B: tests/music.rs music_resolved_youtube_metadata_and_terminal_states_beat_concurrent_placeholders; crates/ganbaru-music-library/src/session/queue.rs youtube_availability_follows_the_replicated_resolution_state; crates/ganbaru-music-library/src/tests/query.rs youtube_counts_and_issues_follow_the_replicated_resolution_state_not_device_availability; apps/client youtube-eligibility.test.ts and playlists/playback.test.ts. Physical: playback.
    - Same YouTube playlist added offline on two devices, one row per table: tests/music.rs music_same_youtube_playlist_added_offline_on_two_devices_keeps_one_row_per_table. Physical: the follow flow in the UI.
    - Reorder same playlist offline: tests/music.rs music_concurrent_reorders_converge. Physical: drag reorder UI.
    - Playlist deleted while songs added, offered and restored: tests/music.rs music_playlist_deleted_while_another_device_adds_to_it_is_offered_and_restored and music_playlist_deleted_while_another_device_adds_an_intended_use_is_offered_with_it; crates/ganbaru-sync-replica/src/tests/exchange.rs a_restored_music_playlist_keeps_concurrent_membership_skip_range_and_intended_use_edits; apps/client sync-presentation.test.ts. Physical: the recovery offer and restore in Settings.
    - Different weights on one membership, one value and no recovery entry: tests/music.rs music_different_weights_on_one_membership_settle_on_one_value_without_an_offer. Physical: none.
    - Remove and re-add membership while range edit: tests/music.rs music_removed_membership_can_be_added_again_on_another_device, music_concurrent_add_of_one_membership_settles_on_its_derived_key, and music_rows_deleted_on_one_device_are_added_again_under_one_successor_key. Physical: none.
    - Play counts summed: tests/music.rs music_statistics_from_three_devices_keep_one_row_per_device. Physical: the counts shown after playback.
    - Focus phase playlist deleted with a replacement while another device repoints: tests/music.rs music_focus_playlist_deleted_with_a_replacement_against_a_repoint_never_keeps_the_deleted_one (both clock orders; the later edit wins and the deleted playlist is never kept). Physical: Focus playback of the phase.
    - Soundscape deleted while a preset sound level is edited: tests/music.rs music_soundscape_deleted_while_a_preset_sound_level_is_edited_cascades_everywhere (the cascade removes the deleted sound, the other sound keeps the edited level, converged). Physical: soundscape playback.
  - 2026-10-23: review fixes merged as a0025a3b; `pnpm -w run validate` passed on feat/sync after dc3be726 raised the Android Music closure and total module ceilings (203 and 1301) for the review fix modules.
- [ ] Step 7: physical acceptance.


### Review fixes (done)

Branch fix/sync-music-review (worktree agent-a84e84c41f4635664). Signed merge 216d2aa6 brings in feat/sync at 10671f43 (milestone 10 step 4, which includes the step 3 compaction merge 713d74f0); the only conflict was the ganbaru-sync-replica dev-dependencies, resolved by keeping both. `git merge-base --is-ancestor feat/sync HEAD` holds. Migration 20261023060004_music_item_resolution_group.sql sorts after the latest feat/sync migration, 20261023045510_projects_folder_triggers.sql. All checks below ran after that merge.

- Finding 1 (deleted items, locations, collections could not be recreated): fixed in 7f57cf68 and the defaults part in 22fcafbe. Writers (local scan, relink, normalization, interchange, YouTube add) move a key retired by a published delete to the first successor key `{key}-2`, `{key}-3`, and so on, derived the same way on every device, so re-adding works and devices that saw the same deletes agree. Tested in music-library local_scan tests and ganbaru-sync tests/music.rs music_rows_deleted_on_one_device_are_added_again_under_one_successor_key.
- Finding 2 (more than 16 preset layers broke the presets list and the native player): fixed in 9d0ebf2e. The client contract no longer rejects long layer lists, the native player plays the first 16 available layers by order key, the editor keeps every layer editable, and no edit adds a layer past 16. soundscapes.md updated to that rule.
- Finding 3 (sync docs omitted Music and contradicted the delivery table): fixed in a4ae3fa6 (docs/data/sync.md and docs/features/README.md).
- Finding 4 (intended use lost when its playlist is deleted concurrently): fixed in 0235c3a0. The intended uses identity is recoverable, so the playlist is offered for recovery and returns with the intended use. Test music_playlist_deleted_while_another_device_adds_an_intended_use_is_offered_with_it.
- Finding 5 (placeholder metadata or `resolving` overwrote resolved YouTube data; refresh overwrote terminal states): fixed in 0235c3a0. Resolution state moved to its own Coupled group, and the metadata group became Coupled with the `metadata_resolved` flag, so the engine's existing coupled rule lets resolved metadata and resolved or terminal states beat placeholders regardless of clock. Manifest bumped to v6 with a new fixture, migration 20261023060004 rebuilds the capture triggers for the new group. A playlist refresh keeps terminal states; only playback asks to resolve again. Test music_resolved_youtube_metadata_and_terminal_states_beat_concurrent_placeholders.
- Finding 6 (restore failed on a missing icon or playback value; no real recovery test): fixed in 22fcafbe (defaults) and a24576db (ganbaru-sync-replica tests/exchange.rs a_restored_music_playlist_keeps_concurrent_membership_skip_range_and_intended_use_edits, which feeds real engine recovery output from a converge scenario through the restorer).
- Finding 7 (YouTube counts and review issues used device availability): fixed in c1f8c1b0. Shared SQL fragments in queries.rs apply the resolution-state rule to playlist eligible and unavailable counts, source summaries, and review issues; the review controller uses `musicItemCountsAsPlayable` from youtube-eligibility.ts. playback.md updated.
- Finding 8 (rekey_memberships deletes without a tombstone): fixed in 36573159 with a rustdoc invariant and a debug assertion that no moved key is published in sync_rows. Writer audit (writes.rs, recovery.rs, interchange.rs, playlist_edits.rs, fixtures) found every writer derives membership ids, so no behavior change.
- Finding 9 (skip range doc wording; unused restore strings): the supporting-domains.md wording was corrected in 7f57cf68 (skip ranges are owned rows keyed by ordinal; only content-identified rows take derived keys). bd96d0fc removes `itemRestored` and `itemRestoreFailed` from sync-presentation.ts and the en and es catalogs, and makes restore copy optional in the presentation registry with neutral fallbacks.
- Acceptance scenarios: 6fb54fb3 adds deterministic tests in crates/ganbaru-sync/src/tests/music.rs for the six manual rows; the acceptance mapping in milestone-5-step-6-progress.md is rewritten.

#### Decisions

- Manifest v6 changes item group layout, so development vaults with Music sync state need a reset (no shims, per AGENTS.md).
- Interchange imports without a `metadata_resolved` value default it to 1, since imported metadata is user or source authored, not a placeholder.
- A timed-out YouTube video counts as a review issue but still attempts playback, matching youtube-eligibility.ts and session/queue.rs; only embedding-blocked and unavailable are not playable. Source summaries follow the same rule as playlists.
- Layer cap: all merged layers are kept and editable, the first 16 available by order key play, and edits cannot add past 16.
- Focus phase scenario: the plan row and the brief differ. The test has A delete the assigned playlist with a replacement (rewriting the assignment, then deleting) while B repoints to a third existing playlist; in both clock orders every device settles on the later edit and never on the deleted playlist. If B instead chose the deleted playlist itself, the assignment keeps that opaque id, which reads as unavailable, as the plan says; assignments are not references, so this is by design.
- Soundscape scenario: deleting a soundscape cascades to its preset sounds through the identity cascade; a concurrent level edit on that sound does not resurrect it, and the other sounds keep their edited levels.
- Membership rekey: kept as an invariant with a debug assertion rather than a tombstone, since no writer produces non-derived membership ids.

#### Checks

- dc25c958 fixes clippy auto-deref findings on the successor key call sites and formats the rekey assertion.
- 9e8c9706 adds the no-offer assertion to the soundscape cascade scenario (passes: a cascaded layer leaves no conflict and no recovery offer).
- `cargo test -p ganbaru-sync`: 222 passed, 1 ignored. `ganbaru-sync-replica`: 59 passed. `ganbaru-music-library --lib`: 173 passed, 2 ignored. `ganbaru-db --lib music`: 11 passed. `ganbaru-tauri-app --lib music`: 144 passed.
- Clippy `--all-targets -D warnings` on ganbaru-sync, ganbaru-music-library, ganbaru-sync-replica, ganbaru-db, ganbaru-tauri-app: clean. `cargo fmt --check`: clean.
- Vitest (youtube-eligibility, presentation-registry, sync-presentation, soundscape contracts, review controller, one worker): 28 passed. `pnpm --dir apps/client run check`: 0 errors, 0 warnings.

#### Known limitations after review

- Successor keys depend on the deletions a device has seen. A device that missed a published delete keeps the old key live; if it edits that row while another device re-adds under `{key}-2`, the delete and the edit meet as usual, and restoring the old row from a recovery offer can leave two rows for one file or playlist. Devices that re-add after seeing different deletion sets can also pick different successor keys. No test covers these cases yet; they are part of the physical acceptance rows for re-adding removed sources.
- Close-out validation (2026-10-23): `pnpm -w run validate` ran every stage on a0025a3b (Rust, both Vitest shards, editor check); only the Android bundle contract failed, fixed by dc3be726 and rerun clean.
