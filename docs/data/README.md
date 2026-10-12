# Data documentation

This directory defines how Ganbaru AI owns, protects, and evolves durable data. It explains decisions that are not obvious from the schema or source code. SQL migrations and schema tests remain authoritative for exact columns, indexes, triggers, and foreign keys.

These documents describe the intended contract. When current code does not meet it, the document names the gap instead of redefining the contract around an accidental implementation detail.

## Start here

- [Architecture](architecture.md) defines sources of truth, portability, and the boundary between files, SQLite, and device-local state.
- [Schema](schema/README.md) indexes the durable SQLite domains and migration policy.
- [Access control](access-control.md) is the normative authorization specification for organizational Chat.
- [Invariants](invariants.md) lists conditions that must remain true across writes, recovery, imports, and refactors.
- [Hazards](hazards.md) records cross-cutting failure scenarios that are easy to miss in ordinary feature work.
- [Security](security/README.md) defines the threat model and security boundaries.
- [Sync](sync.md) defines the implemented local whole-vault handoff and the separate planned concurrent synchronization contract.

## Authority map

| Question | Authoritative source |
| --- | --- |
| Which storage mechanism owns a kind of data? | [Architecture](architecture.md) |
| What tables and relationships exist now? | SQL migrations and schema tests, summarized under [Schema](schema/README.md) |
| Who may see or operate on Chat resources? | [Access control](access-control.md) |
| What must never become false? | [Invariants](invariants.md) |
| What failure sequences require explicit handling? | [Hazards](hazards.md) |
| What is trusted, untrusted, or future security work? | [Security](security/README.md) |
| How does local handoff work, and how will concurrent sync work later? | [Sync](sync.md) |

Feature documents own user-visible behavior. Algorithm documents own pure decision rules. Data documents own durable identity, authority, persistence, migration, and recovery contracts. Avoid copying the same rule into all three layers.

## Change discipline

The app has no external users yet, so old development vaults, internal exports, and device state do not justify compatibility readers or migration shims. See [Migration policy](schema/README.md#migration-policy).

Do not document routine fields merely to mirror DDL. Preserve the rationale for non-obvious identity, ownership, deletion, ordering, history, portability, and security decisions.
