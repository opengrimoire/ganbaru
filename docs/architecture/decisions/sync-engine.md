# Sync engine decisions

**Status: Reference.** Recorded 2026-10-09, when Quick notes became the first replicated domain. This record explains why concurrent sync is built the way it is. The contract is owned by [Device linking and synchronization](../../data/sync.md), merge semantics by [Sync merge rules](../../algorithms/sync/README.md), and the engine tables by [Sync schema](../../data/schema/sync.md).

## Constraints

- SQLite stays the source of truth. Domain tables hold the materialized current state, and the engine adds metadata beside them; it is never a parallel store the UI reads.
- Saves are local and immediate, and every write path must replicate, including imports, restores, scheduled jobs, and native background writes.
- Applying the same set of valid operations in any causal order must give byte-identical state, conflicts, and recovery entries. Nothing is silently lost.
- No project-operated infrastructure. LAN linking needs no server, and a later relay must store only opaque encrypted records.
- Operations carry a space, a person-certified writer, an authorization revision, and a key epoch from the start, so multi-person spaces later are a configuration of the same engine rather than a redesign.

## Decisions

| Decision | Reason |
| --- | --- |
| Trigger-based capture plus sealing, not explicit per-command operations | Capture triggers are attached to the schema and commit atomically with the change, so coverage is complete by construction. A conformance test fails when the triggers differ from the manifest. |
| Seal from current row values with field-group masks | Rapid edits coalesce, operation content always matches the causal context it is signed with, and no pre-image logging is needed. Local captures are sealed before any remote operation applies in the same database. |
| Explicit classification of every table and column in a Rust manifest | Unknown data fails closed. Table and field ids are stable and never reused, and validation of an operation is a pure function of its bytes and the current manifest; manifest versions only add or loosen rules, so a valid operation stays valid after an update. |
| Multi-value registers with version vectors | A write supersedes exactly the versions its author had seen. Concurrent versions stay as alternatives, which gives order independence and loss-free conflicts without a global order or a server. |
| Hybrid logical clock only as a display tie breaker | Clock skew can change which concurrent value is shown, never what is superseded or kept. |
| Installation-scoped writer keys certified by the person key | Reinstalls and restores get fresh writers, a durable sequence reservation detects restored or copied databases, the hub detects disk clones as forks, and future people's writers use the same trust path. |
| Hand-written, bounded, big-endian binary codec in the contracts crate | Signatures need exact bytes, and serde formats do not guarantee a canonical encoding across versions. It matches the contact card codec and adds no dependency. Golden vectors and mutation tests pin the format. |
| Value hashes in the signed header, values in an unsigned body | Later compaction can drop superseded values without breaking signature verification. |
| Variable-length string order keys with a row id tie breaker | Concurrent moves never require rebalancing other rows, and byte comparison matches SQLite `BINARY` ordering. |
| A registry of domain manifests with disjoint table id ranges, and an engine built from the manifest plus one adapter per domain | Each domain converts in one place, conformance checks every domain the same way, and tests can register a fixture domain without touching the vault manifest. The engine refuses adapters that do not match the manifest domains. |
| App-registered materialize hooks that receive only the connection and a change sink | Aggregates and derived tables stay current inside the engine transaction without the engine knowing domain rules, and the app learns what to refresh from the reports. Hook output is never captured, so it cannot travel or diverge replicas. |
| A generic typed codec for owned tables keyed by owner and ordinal | New domains declare owned columns instead of writing codecs, and a conformance round trip through the real schema pins each codec. |
| Local classification for device state, and declared policies for every trigger on synced tables | Device state must neither travel as operations nor leak through snapshot copies, and a trigger that writes during apply would diverge replicas unless it only maintains derived or same-row state. |
| Carried operations applied and local state restored in the staged database before it replaces the vault, with local state read from the receiver's live database | The vault is never in use without its own operations or device state, and a failure at any step leaves the current vault whole. Rollback copies are pruned and may fail to be kept, so they cannot be the source of device state. |
| Mixed mode with connection-scoped guard triggers | Each domain converts when ready while unconverted data stays single-writer and safe. Guards live only on the connection, so they never travel in a snapshot. |
| Star topology through the coordinator desktop | Revocation cutoffs and fork detection are deterministic without consensus because every operation passes one point. Delivery through other devices or a relay requires the anchored authority in [Sync topology and authority](sync-topology-and-authority.md) first. |
| Single connection per vault pool | Capture suppression during apply and the seal-then-apply order rely on nothing interleaving inside an engine transaction. Raising the pool size requires redesigning capture suppression first. |

## Rejected alternatives

- **cr-sqlite:** a native loadable extension (unsafe loading and native code in every process) with column-level last-writer-wins semantics and no support for surfaced conflicts, coupled values, order keys, or recovery.
- **Automerge or Loro as the canonical store:** conflicts with SQLite as the source of truth and would duplicate every domain model. Loro remains a reference for tree and list algorithms.
- **Server-authoritative engines (Replicache, PowerSync, Electric, Zero):** require project-operated or always-on servers and centralize authority.
- **Hocuspocus as relay:** persists plaintext Yjs documents server-side, breaking the encrypted record boundary.
- **Serde formats (postcard, CBOR, JSON) for signed operations:** no canonical byte guarantee across versions.
- **Explicit per-domain operation code without capture:** coverage depends on every write path remembering to emit, including imports, restores, scheduled jobs, and native background writes, and a missed path silently diverges replicas.
- **Whole-row last-writer-wins:** loses concurrent edits to different fields and hides conflicts.

## Accepted limitations

- Operations are plaintext at rest inside the vault, like the rest of the vault, until per-space encryption exists.
- Every linked device holds the person key, so a revoked device could still certify a new writer; the hub refuses revoked devices at TLS, and a separate admin key is planned with anchored authority.
- Android writer keys use app-private files until Android Keystore wrapping exists.
- The operation log and retained state grow until causal stability and compaction exist.
- Linked devices must run the same protocol version, which is acceptable before external users.
