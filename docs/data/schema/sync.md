# Sync engine schema

**Status: Implemented** in source. The sync engine stores its metadata in `sync_*` tables beside the domain tables in the vault database. Exact columns, constraints, and indexes are authoritative in `crates/ganbaru-db/migrations/`; the engine manifest in `crates/ganbaru-sync/` classifies every other table. Replication behavior is owned by [Device linking and synchronization](../sync.md) and merge semantics by [Sync merge rules](../../algorithms/sync/README.md).

## Tables

| Table | Holds |
| --- | --- |
| `sync_spaces` | The vault's replication spaces. One personal space per vault, whose id derives from the vault id. |
| `sync_apply_state` | A singleton `applying` flag that suppresses capture while the engine materializes remote changes. |
| `sync_capture` | Local changes not yet sealed: one row per changed domain row with its changed group mask, groups forced by a conflict resolution, and created and deleted flags. |
| `sync_writers` | Every known writer of a space: public key, device, certificate, state (`active`, `retired`, `revoked`, `forked`), revocation cutoff, stored and applied sequences, and the head hash and clock of its chain. |
| `sync_ops` | Every stored operation as its exact signed envelope, with its full causal context and state (`waiting`, `applied`, `held` with a reason). |
| `sync_rows` | Merge state for every published row of a replicated table: live or tombstoned, a recovery flag, and a conflict mask. |
| `sync_register_versions` | Concurrent versions of each group of each published row, at most one per writer, with the decoded value, its hash, and an indexed reference key for reference groups. |
| `sync_tombstones` | Tombstones of published rows, keyed by the deleting operation, with an optional redirect. |

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
- **Published keys are permanent.** A key present in `sync_rows` cannot be inserted again locally when its domain row is gone, which prevents resurrection and key reuse.
- **Device-local state stays outside.** Writer keys, the writer record with its sequence reservation, pause state, and carry-forward bundles live in the app config directory, never in the vault database, because the vault database is copied whole to other devices.

## Replicated domain tables

A replicated table carries capture triggers, row key and creation time immutability guards, a re-create guard, and value guards that enforce operation bounds on every local write. All of them are rendered from the manifest and stored as schema objects; a conformance test fails when they differ from the current rendering, when a table or column is unclassified, or when a UNIQUE index or multi-column CHECK on a replicated table has no declared resolution.

Every column of a replicated table is a key, a member of exactly one field group, local, or derived. The optimistic `revision` column is local: each replica bumps it when it materializes a content change, never for order or `updated_at`.

Connection-scoped guard triggers on unconverted tables are created per connection on guarded replicas only and are never part of the vault schema. See [mixed mode](../sync.md#mixed-mode).

Currently replicated: Quick notes and their tags. Their schema rules are in [Supporting domains](supporting-domains.md#quick-notes).
