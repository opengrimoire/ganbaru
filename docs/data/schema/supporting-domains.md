# Supporting schema domains

This document covers persisted domains that do not need a dedicated schema page. Exact columns, constraints, and indexes remain authoritative in migrations and schema tests.

## Quick notes

Quick notes are structured SQLite data, not loose Markdown files. A note has stable identity, rich text stored as normalized text runs, color, pin, archive and trash state, an optional tag, an order key, and timestamps. Rendered HTML is never canonical. Quick notes and their tags are replicated tables; their field groups and merge kinds are described in [Sync merge rules](../../algorithms/sync/README.md) and the engine tables in [Sync engine schema](sync.md).

- **Order.** Notes and tags sort by `(order_key, id)`. Order keys are variable-length strings compared by bytes, so placing a note writes only that note's key and never rebalances its group. Content updates never move a note; create, pin, unarchive, and restore place it first in its group.
- **Tags.** Tags have stable identity independent of their name. Names are unique case-insensitively and trimmed to 1 to 40 characters. The nine-tag cap is a local create rule only; replicated merges can leave more tags, which stay visible so rename and delete can fix them. Renaming changes only the name. Deleting a tag clears it from its notes in the same transaction, and naming a tag that no longer exists leaves the note untagged instead of failing.
- **Trash.** Reads hide notes trashed more than seven days ago. The `quick_notes_purge_expired_trash` command deletes them and returns the next expiry; a frontend lifecycle job runs it, and read paths never write. The purge is an ordinary deletion, so an edit made elsewhere to a purged note becomes a recovery entry.
- **Revision.** The optimistic `revision` is local to each replica and changes only for content and lifecycle changes, never for reordering.

Windows sync through canonical writes and invalidation, and replicated changes from other devices use the same invalidation; a window is never the only copy of a note.

## Themes

Themes and token values live in SQLite so custom themes can be validated, selected, repaired, imported, and exported as structured data. Built-in themes have stable identities, and a custom theme never mutates a built-in row. Import validates format, token keys, value types, size, and identity before writing. Token rules are owned by [Themes](../../features/themes/README.md).

The active theme ID is portable configuration. Applying it to a webview is runtime projection. A missing or invalid selected theme falls back safely without deleting the user's row, so it can still be repaired or exported.

## Music

Music separates canonical library identity from physical source location and playback state.

- Library items carry playable identity and metadata. Local roots and locations describe user-selected folders or document trees without making an absolute path canonical.
- Source collections (local collections, YouTube playlists, and other discovery sources) keep refresh generation and failure state. Refresh and relink use staged generations so a failed scan leaves the previous library usable.
- User playlists and ordered memberships are independent of source collections; removing or refreshing a source never silently destroys a playlist.
- Related families cover skip ranges, item signals, snoozes, statistics, recent selections, refresh issues, context assignments, and soundscapes.

Music bytes stay in the user's library and are never copied into the vault because an item was indexed. Relative identities and platform document handles are revalidated before playback.

Import receipts store the action identity, request hash, and result in the same transaction as the imported rows, so a lost response cannot duplicate a playlist, and a reused identity with different input is rejected. Export reads all row families from one bounded snapshot.

Native session checkpoints are keyed by device ID and hold portable item and membership IDs, playback settings, bounded navigation history, and queue state, never resolved paths, content URIs, or decoder handles. Copying a vault copies these records, not permission to resume another device's playback. Recovery rebuilds the local queue against current rows and bindings and starts paused. Listening counters, checkpoint changes, and action receipts share one transaction.

Background sound state records whether a selection was manual or automatic. An automatic selection restored after restart is displayed but never starts output by itself; a fresh native Focus phase decision is required.

## Distraction blocker

Portable rules live in the vault `config.json`, with separate browser and desktop settings because the surfaces can carry different intent. SQLite stores usage samples and block events for daily limits, reports, and recovery. Local-date grouping keeps the date basis, so a later device timezone change never reassigns historical usage. Browser rules, desktop application rules, and Android app rules have separate source identities and validation.

Device-local state supports enforcement without replacing portable policy:

- Runtime and budget snapshots are native derivatives of the persisted rules and accepted usage. Their configuration fingerprint, local day, week, and totals are admission checks, not policy.
- The desktop native owner spools captured usage batches with retry receipts before publishing totals, and sent sample IDs are immutable until acknowledged.
- Android's Guardian journal records usage, totals, and its observation checkpoint in one transaction. A full journal rejects new evidence without advancing the checkpoint, and rows from another vault stay pending until that vault is active again.

Linked-device accounting follows [Sync](../sync.md#distraction-usage-exception). Accepted totals and acknowledgement identities commit together, so a failed acknowledgement is retried before new evidence is sent and never double-counts usage. A capture with inconsistent identities, vault attribution, dates, or oversized evidence fails explicitly instead of publishing a truncated total. Vault and configuration changes revoke the old native publication before the new one becomes visible.

## Profile and managed icons

Profile metadata is portable configuration. Profile and project-icon bytes use bounded feature-owned asset paths, and rows store managed relative identity, never an absolute or hardcoded vault path. Replacing an image stages validated bytes, updates the relationship atomically, and removes the old asset only after rechecking references.

## Contacts

Source: `crates/ganbaru-db/migrations/`, `crates/ganbaru-contacts/`, and `apps/client/src-tauri/app/src/contacts/`.

The vault holds one local identity row (public key, current card nonce, card revision), contact rows keyed by the contact's public key with a state of active or blocked and two independent trust scopes with optional expiry, and contact request rows in either direction with the peer's signed card, delivery hint, expiry, and last delivery error. The private key is never stored in the vault; see [Contacts and contact requests](../sync.md#contacts-and-contact-requests). Contacts and requests carry a revision, and every mutation names the revision it expects. Regenerating the card changes only the nonce and revision, so existing contacts keep working while earlier cards stop admitting requests. Removing a contact deletes its row; blocking keeps a row in the blocked state so later requests from that key can be dropped silently. Rows for the same vault follow it across linked devices as part of the whole-vault handoff.

## Domain evolution

Split a domain into its own page when it gains a distinct set of identity, retention, recovery, or interoperability rules that maintainers need to understand together, not merely to list more columns.
