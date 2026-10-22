# Sync engine schema

**Status: Implemented** in source. The sync engine stores its metadata in `sync_*` tables beside the domain tables in the vault database. Exact columns, constraints, and indexes are authoritative in `crates/ganbaru-db/migrations/`; the engine manifest in `crates/ganbaru-sync/` classifies every other table. Replication behavior is owned by [Device linking and synchronization](../sync.md) and merge semantics by [Sync merge rules](../../algorithms/sync/README.md).

## Tables

| Table | Holds |
| --- | --- |
| `sync_spaces` | The vault's replication spaces. One personal space per vault, whose id derives from the vault id. |
| `sync_apply_state` | A singleton `applying` flag that suppresses capture while the engine materializes remote changes. |
| `sync_capture` | Local changes not yet sealed: one row per changed domain row with its changed group mask, groups forced by a conflict resolution, created and deleted flags, and `held_until_ms`, the end of a [deletion hold](../../algorithms/sync/README.md#deletion-holds) that keeps a delete, and the deletes it cascaded to, unsealed. |
| `sync_seeded_tables` | Replicated tables whose existing rows capture seeding has captured. The baseline records the tables that replicated from the start. |
| `sync_writers` | Every known writer of a space: public key, device, certificate, state (`active`, `retired`, `revoked`, `forked`), revocation cutoff, stored and applied sequences, and the head hash and clock of its chain. |
| `sync_ops` | Every stored operation as its exact signed envelope, with its full causal context and state (`waiting`, `applied`, `held` with a reason). |
| `sync_rows` | Merge state for every published row of a replicated table: live or tombstoned, a recovery flag, and a conflict mask. |
| `sync_register_versions` | Concurrent versions of each group of each published row, at most one per writer, with the decoded value and its hash. |
| `sync_version_refs` | The non-null reference values of each retained version, one row per reference column, indexed by referenced key so redirects and deletions find their referrers. |
| `sync_tombstones` | Tombstones of published rows, keyed by the deleting operation, with an optional redirect or an optional cause (`cause_table`, `cause_row`) naming the deleted root a cascade repair followed. |

## Invariants

- **Engine metadata is not a parallel store.** The UI reads domain tables. The engine materializes winners into them; its own tables are never queried for display except conflict, recovery, and status views.
- **Single connection.** Capture suppression through `applying` and the seal-then-apply order are correct only because every vault pool has exactly one connection, so nothing interleaves inside an engine transaction. `applying` is reset to 0 at service start and in snapshot copies.
- **Captures commit with the change.** Capture triggers write `sync_capture` in the same transaction as the domain write. Deleting a row supersedes its group marks. A row created and deleted before it was ever published is dropped at seal time.
- **Contiguous chains.** A writer's stored operations are contiguous from 1 and linked by hash. `applied_seq` never exceeds `stored_seq`. An operation is stored at most once; storing the same operation again is a no-op, and a different operation at a stored sequence is a fork.
- **Envelopes are kept whole.** Peers are served the exact signed bytes, so stored envelopes are never re-encoded.
- **Held means stored.** A held operation keeps its reason and blocks its writer's later operations and everything that depends on them, but it is never dropped and is still forwarded.
- **Versions are mutually concurrent.** After every apply, the versions of one group are exactly those no newer applied version's context covers. A local seal leaves its own version as the only one.
- **Retained state.** Versions of a tombstoned row stay as retained state for recovery. Only a tombstoned row can carry the recovery flag, and only a live row can carry a conflict mask.
- **Redirects point down.** A tombstone redirect is always a lower row key than the tombstoned row, so redirect chains terminate.
- **Causes exclude redirects.** A tombstone carries a redirect or a cause, never both. A cause names the root of a cascade, so recovery groups the tombstoned row under that root and checks it against the root's coverage.
- **Published keys are permanent.** A key present in `sync_rows` cannot be inserted again locally when its domain row is gone, which prevents resurrection and key reuse. A [derived-key table](../../algorithms/sync/README.md#derived-keys) is the exception: its rows are removed and added again through an existence register, never tombstoned.
- **Device-local state stays outside.** Writer keys, the writer record with its sequence reservation, pause state, and carry-forward bundles live in the app config directory, never in the vault database, because the vault database is copied whole to other devices. The exception is device state that must change atomically with vault data: it lives in local tables and columns, which snapshot copies empty or reset and a replacement restores from the receiver's live database.

## Replicated domain tables

A replicated table carries capture triggers, row key and creation time immutability guards, a re-create guard, and value guards that enforce operation bounds on every local write. A table with a [publication predicate](../../algorithms/sync/README.md#publication) also carries a guard that refuses turning a published row back into a draft, and its capture triggers and those of its `published_with` children skip draft rows. All of them are rendered from the manifest and stored as schema objects; a conformance test fails when they differ from the current rendering, when a table or column is unclassified, when a table id falls outside its domain's range, when a UNIQUE index or multi-column CHECK on a replicated table has no declared resolution, when an owned table's unique indexes differ from its declared unique columns or it has a multi-column CHECK, when a replicated column is a real number, when a hook column is neither nullable nor defaulted, when an owned codec does not round trip through the schema (sample rows are encoded, decoded, written to the owned tables, read back, and encoded to identical bytes), when a foreign key into a replicated table neither cascades nor clears on delete, when a replicated table's foreign keys differ from its declared references and their delete actions, or when tables are not listed in [dependency order](../../algorithms/sync/README.md#references).

A replicated table with a composite primary key carries a `sync_key TEXT NOT NULL UNIQUE` key column, derived from the primary key columns by `ganbaru_sync_contracts::derived_key` with a table prefix, and its primary key columns form a required immutable identity group. A row inserted before its parts are known may hold a `pending:` placeholder key: the key guard lets one update replace a placeholder with a valid key, a placeholder row is never sealed, and the engine refuses to start while any replicated row still holds one.

Every column of a replicated table is a key, a member of exactly one field group, local, derived, a hook column maintained by a [materialize hook](../../algorithms/sync/README.md#materialize-hooks), or key-derived. A key-derived column holds a fixed prefix followed by the row key: materialize writes it on insert, a `CHECK` against the key pins it, its unique index and that `CHECK` need no resolution, and it stays out of codecs and summaries. Every column of an [owned table](../../algorithms/sync/README.md#owned-tables) is its owner column, an ordinal, or a declared data column. A local column declares the value it takes when the engine materializes a row and when a snapshot copy resets it. The optimistic `revision` column is local: each replica bumps it when it materializes a content change, never for order or `updated_at`.

A domain whose values reference managed files declares their asset classes in its manifest. A class name is a relative path of lowercase segments that fixes the directory of its files under `assets/`, the bound is between 1 byte and 64 MiB, and class names are unique across domains; files are named `{sha256}.{ext}` from the referenced digest and content type, so no stored value chooses a path. Manifest validation fails on a class that breaks these rules and on asset groups in a domain that declares no class. Quick note images are the `quick-notes/images` class.

Every other trigger on a converted, local, or engine table declares a policy in its domain manifest: derived index maintenance, same-row bookkeeping, gated on the engine not applying, or visible to the applier. Conformance fails on an undeclared trigger, a trigger that breaks its policy, and an ungated unconverted trigger that writes a converted table. Quick notes search index triggers are derived index maintenance.

Connection-scoped guard triggers are created per connection on guarded replicas only and are never part of the vault schema. See [mixed mode](../sync.md#mixed-mode).

Currently replicated: Quick notes and their tags, portable preferences with the profile picture in the `profile` class, user themes with their owned token and palette tables, and contacts with contact requests and the contact card. Their schema rules are in [Supporting domains](supporting-domains.md#quick-notes), [Themes](supporting-domains.md#themes), [Profile and managed icons](supporting-domains.md#profile-and-managed-icons), and [Contacts](supporting-domains.md#contacts). The person identity in `contacts_local_identity` is an engine table: it anchors the sync space and never travels as an operation.
