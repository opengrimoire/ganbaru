# Supporting schema domains

This document covers persisted domains that do not need a dedicated schema page. Exact columns, constraints, and indexes remain authoritative in migrations and schema tests.

## Quick notes

Quick notes are structured SQLite data, not loose Markdown files. A note has stable identity, rich text stored as normalized text runs, color, pin, archive and trash state, an optional tag, an order key, and timestamps. Rendered HTML is never canonical. Quick notes and their tags are replicated tables; their field groups and merge kinds are described in [Sync merge rules](../../algorithms/sync/README.md) and the engine tables in [Sync engine schema](sync.md).

- **Order.** Notes and tags sort by `(order_key, id)`. Order keys are variable-length strings compared by bytes, so placing a note writes only that note's key and never rebalances its group. Content updates never move a note; create, pin, unarchive, and restore place it first in its group.
- **Tags.** Tags have stable identity independent of their name. Names are unique case-insensitively and trimmed to 1 to 40 characters. The nine-tag cap is a local create rule only; replicated merges can leave more tags, which stay visible so rename and delete can fix them. Tags carry a name and an event-palette color that edit independently. Deleting a tag clears it from its notes in the same transaction, and naming a tag that no longer exists leaves the note untagged instead of failing.
- **Trash.** Reads hide notes trashed more than seven days ago. The `quick_notes_purge_expired_trash` command deletes them and returns the next expiry; a frontend lifecycle job runs it, and read paths never write. The purge is an ordinary deletion, so an edit made elsewhere to a purged note becomes a recovery entry.
- **Images.** A note holds up to 24 ordered image references in `quick_note_images`: digest, type, byte size, and pixel dimensions. Bytes live in content-addressed files under `assets/quick-notes/images/`, shared by every note that references the same image. A note saves its whole list at once, which syncs as one register, so concurrent edits to a note's images become a conflict rather than a silent mix.
- **Revision.** The optimistic `revision` is local to each replica and changes only for content and lifecycle changes, never for reordering.

Windows sync through canonical writes and invalidation, and replicated changes from other devices use the same invalidation; a window is never the only copy of a note.

## Themes

Themes and token values live in SQLite so custom themes can be validated, selected, repaired, imported, and exported as structured data. Built-in themes have stable identities, and a custom theme never mutates a built-in row. Import validates format, token keys, value types, size, and identity before writing. Token rules are owned by [Themes](../../features/themes/README.md).

The active theme ID is a device preference, and the quick-toggle light and dark pair is a portable preference. A device without a saved active theme uses the quick-toggle theme matching the system color scheme. Applying it to a webview is runtime projection. A missing or invalid selected theme falls back safely without deleting the user's row, so it can still be repaired or exported.

The `themes` table replicates, and its live and seed token and palette tables are owned rows that travel with it. Seed columns and seed rows are immutable after insert. The highest dismissed engine upgrade lives on the theme row as `upgrade_dismissed_engine_version` and merges as a maximum, so it is deleted with the theme. Theme writes skip rows that already hold the requested values, so a save that changes nothing records no operation.

## Music

Music separates canonical library identity from physical source location and playback state.

- Library items carry playable identity and metadata. Local roots and locations describe user-selected folders or document trees without making an absolute path canonical. Original title, artist, and album text is capped at 500 characters in the schema; writers truncate longer source text.
- Rows whose identity follows from content (local and imported items, locations, YouTube items, source collections, and memberships) take keys derived in Rust from that content, never from client-supplied ids; writes return the keys in their receipts. A location whose path changes through normalization or relink is removed and added under its new key rather than renamed.
- Sync never lets a published key be created again. Memberships and pair-keyed rows re-add under their derived key through an existence register; items, locations, and source collections instead move to the first successor key (`{key}-2`, `{key}-3`, and so on) that no published deletion retired, so deleting a file, video, or followed playlist and adding it back works, and devices that saw the same deletions re-add it under the same key.
- Writers diff child rows instead of deleting and reinserting them, and skip writes whose values are unchanged. Device observations (location size, modification time, availability, and seen generation; item availability; collection refresh state) update without touching a row's `updated_at_ms`.
- Built-in playlists exist from the baseline migration; reads never recreate them.
- Source collections (local collections, YouTube playlists, and other discovery sources) keep refresh generation and failure state. Refresh and relink use staged generations so a failed scan leaves the previous library usable.
- User playlists and ordered memberships are independent of source collections; removing or refreshing a source never silently destroys a playlist.
- Playlists, memberships, and preset sounds are ordered by fractional order keys and then id, compared byte-wise, so a move rewrites only the moved rows. A membership's break item and its range are columns of the membership, and its skip ranges are owned rows keyed by membership and ordinal.
- Volumes, rates, and soundscape levels are stored as integer thousandths so equal values compare exactly across devices; the frontend converts them to multipliers at the command boundary.
- Listening statistics are kept per device and item, and reads sum counts and take the latest timestamps across devices.
- Music replicates concurrently as tables 48 to 62. Tables keyed by a pair of ids (signals, statistics, collection items, intended uses, preset sounds, and context assignments) carry a `sync_key` derived from that pair, and memberships use the derived key of their playlist and item as their `id`, so a membership removed on one device and added again on another settles on one row. Device observations (location size, modification time, availability, and seen generation; item availability; collection refresh state) are Local columns and never replicate. A vault opened after the capture migration converts placeholder keys and older membership ids to their derived values before sync starts; moving a membership drops session checkpoints, which name memberships. A YouTube item's fetched metadata and its resolution state merge as separate coupled groups: concurrent metadata carrying the resolved flag beats a placeholder, and a resolved or terminal state beats `resolving`. An intended use added while its playlist is deleted offers the playlist for recovery.
- Related families cover skip ranges, item signals, snoozes, statistics, recent selections, refresh issues, context assignments, soundscapes, and soundscape presets.
- A context assignment that plays a soundscape names exactly one preset; other soundscape behaviors name none. Preset references are not foreign keys, so a removed preset stays an explicit unavailable reference. Preset layer membership cascades when a layer is removed, and an emptied preset is kept. A local layer's file path is device-local app state keyed by vault and layer, never a vault row.

Music bytes stay in the user's library and are never copied into the vault because an item was indexed. Relative identities and platform document handles are revalidated before playback.

Import receipts store the action identity, request hash, and result in the same transaction as the imported rows, so a lost response cannot duplicate a playlist, and a reused identity with different input is rejected. Export reads all row families from one bounded snapshot.

Native session checkpoints are keyed by device ID and hold portable item and membership IDs, playback settings, bounded navigation history, and queue state, never resolved paths, content URIs, or decoder handles. Copying a vault copies these records, not permission to resume another device's playback. Recovery rebuilds the local queue against current rows and bindings and starts paused. Listening counters, checkpoint changes, and action receipts share one transaction.

Soundscape state records whether a selection was manual or automatic. An automatic selection restored after restart is displayed but never starts output by itself; a fresh native Focus phase decision is required.

## Distraction blocker

Source: `crates/ganbaru-db/migrations/`, `crates/ganbaru-distractions/src/config/`, and `apps/client/src-tauri/app/src/distractions/`.

Portable rules are SQLite rows in replicated tables, with separate browser, desktop, and Android settings because the surfaces can carry different intent:

- `distractions_settings` is one row keyed `vault`, seeded by its migration on every replica, with the mode and each switch in its own field group, so concurrent toggles of different switches all survive.
- `distractions_category_rules` holds a row only for a built-in category toggled away from its default; a missing row means the default from the shared category list.
- `distractions_custom_categories` holds named custom categories; their hosts are `distractions_host_rules` rows scoped by the category id.
- `distractions_host_rules` and `distractions_app_rules` are keyed by content (list, scope, and host; the lowercase desktop app name or the Android package), so a rule removed on one device can be added again on another.
- `distractions_usage_limits` owns its entries in `distractions_usage_limit_entries`, ordered by ordinal, and the limit and its entries merge as one value.
- `distractions_usage_totals` holds one row per device, source, and local date, keyed by a hash of those four values. Only its device writes it, its seconds merge by maximum and never shrink, and rows are never deleted; reads keep the usage retention window.

Native commands make each edit in one transaction and return the assembled rules or a refusal reason; every reader (desktop blocker, browser host, Android projection, Settings) reads that one assembled snapshot. Assembly skips rows it cannot use instead of failing. `distractions_usage_samples` is a local table: each device keeps its own usage journal there and derives its totals from it. `distractions_block_events` replicates with its Focus run: `run_id` cascades with the run, `segment_id` clears, and each event owns at most one immutable rule snapshot keyed by `(block_event_id, ordinal)`. Local-date grouping keeps the date basis, so a later device timezone change never reassigns historical usage. Browser rules, desktop application rules, and Android app rules have separate source identities and validation.

Device-local state supports enforcement without replacing portable policy:

- Runtime and budget snapshots are native derivatives of the persisted rules and accepted usage. Their configuration fingerprint, local day, week, and totals are admission checks, not policy.
- The desktop native owner spools captured usage batches with retry receipts, and the spool drains into the usage journal whenever this device may write usage. A batch leaves the spool only after the journal committed it, and the journal ignores sample IDs it already holds.
- The browser host spools block events with the redacted host only. The app validates each spooled event, records it once, without its run when the vault no longer holds that run, and removes it from the spool only after the vault committed it. Each event records in its own savepoint, so an event the validation or the vault constraints refuse is dropped and logged while the rest of the batch imports; any other vault error keeps the whole batch for a later attempt. Spooled events never name a segment.
- Android's Guardian journal records usage, totals, and its observation checkpoint in one transaction. A full journal rejects new evidence without advancing the checkpoint, and rows from another vault stay pending until that vault is active again.

Combined accounting across devices follows [Sync](../sync.md#mixed-mode). Limits add other devices' totals to this device's journal and its pending spool or Guardian events, each counted once. A capture with inconsistent identities, vault attribution, dates, or oversized evidence fails explicitly instead of publishing a truncated total. Vault and configuration changes revoke the old native publication before the new one becomes visible.

## Profile and managed icons

Profile metadata is a set of portable preferences in `vault_preferences`, which stores one canonical JSON value per registered key, or per child of a keyed map such as a sidebar order per project, with bounded key and value sizes. Profile and project-icon bytes use bounded feature-owned asset paths, and rows store managed relative identity, never an absolute or hardcoded vault path. The `profile.image` preference is an object of exactly `path` (`profile/{sha256}.{png|jpg|webp}`) and `byteSize`, so sync can fetch and verify the file it names. Replacing an image stages validated bytes, updates the relationship atomically, and removes the old asset only after rechecking references.

## Contacts

Source: `crates/ganbaru-db/migrations/`, `crates/ganbaru-contacts/`, and `apps/client/src-tauri/app/src/contacts/`.

The vault holds one local identity row with the person public key, written once and never changed or deleted; one card row (`contact_card`) with the current card nonce and card revision; contact rows keyed by the contact's public key, with an id derived from it, a state of active, removed, or blocked, and two independent trust scopes with optional expiry; and contact request rows in either direction with the peer's signed card, delivery hint, expiry, and last delivery error. A received request keeps its own random id and records the id the requester chose in `peer_request_id`, because a requester's id is unique only for that person. The private key is never stored in the vault; see [Contacts and contact requests](../sync.md#contacts-and-contact-requests). Contacts and requests carry a revision, and every mutation names the revision it expects. Regenerating the card changes only the nonce and revision, so existing contacts keep working while earlier cards stop admitting requests. Removing or unblocking a contact keeps its row in the removed state with no trust, which readers treat as no contact, so the removal merges with concurrent edits on other devices; blocking keeps a row in the blocked state so later requests from that key can be dropped silently. A pending request whose expiry has passed reads as not pending everywhere, without a write. Two devices may each hold a pending row for the same person and direction, so the schema has no uniqueness rule for pending rows and readers take the newest. Contacts, requests, and the card replicate concurrently between linked devices; the identity row travels only with snapshots and handoff.

## Domain evolution

Split a domain into its own page when it gains a distinct set of identity, retention, recovery, or interoperability rules that maintainers need to understand together, not merely to list more columns.
