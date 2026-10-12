# SQLite schema

Ganbaru AI stores structured application data and document graphs in the active vault's `ganbaru-ai.sqlite` database. This directory explains durable domain relationships and non-obvious constraints. It is not a handwritten copy of every column or index.

The authoritative schema is the ordered migration set in `crates/ganbaru-db/migrations/`. The `ganbaru-db` crate embeds it with SQLx and owns pool and migration services. Schema invariant tests verify the resulting database.

## Domain map

- [Calendar](calendar.md) covers calendars, events, recurrence, import preservation, notifications, and project scheduling links.
- [Pomodoro](pomodoro.md) covers configuration snapshots, runs, segments, pauses, recovery evidence, and adaptive decisions.
- [Notes and projects](notes-and-projects.md) covers the Notes graph, databases, assets, history, project planning, tasks, templates, and managed working folders.
- [Chat](chat.md) covers organizational channels, provider execution, assignments, working-folder identity, canonical events, attachments, checkpoints, and cleanup.
- [Supporting domains](supporting-domains.md) covers Quick notes, themes, Music, the distraction blocker, and other smaller persisted surfaces.
- [Sync engine](sync.md) covers replication spaces, local capture, writers, the operation log, register versions, tombstones, and the guards on replicated tables.
- [Query plans](query-plans.md) records latency-sensitive reads whose index use is protected by tests.

## Migration policy

The app has no external users yet. `20261004220000_baseline_schema.sql` is the fresh-start schema, and later changes are additive migrations named `YYYYMMDDHHMMSS_description.sql` with a UTC timestamp. SQLx discovers them at compile time, so they are never registered manually. Older development databases, internal exports, and device state are unsupported when their contracts change and must be recreated explicitly; do not keep compatibility readers or upgrade shims for them, and never delete local vaults automatically. Once a user-capable release can have applied a migration, it is immutable. A schema squash requires explicit maintainer approval.

Before changing persistent state, check:

- Partially populated rows in the current format.
- Supported external imports and current internal exports.
- Config and JSON validation.
- Seed and repair paths.
- Stale rows and derived indexes.
- Cleanup after interrupted filesystem work.

## Encoding rules

Identity and time encodings are domain-specific contracts, not global assumptions.

Many user-authored entities use client-generated UUID text, but stable semantic IDs, hashes, compound source identities, singleton IDs, and built-in values such as the local calendar are also valid. New IDs must be globally stable where synchronization or import requires it and must not expose authority through guessability.

Calendar, Pomodoro, Notes, Projects, and Chat primarily use normalized text timestamps named `*_at`. Integer Unix epoch milliseconds always carry an `_ms` suffix (`created_at_ms`, `review_deferred_until_ms`), both in columns and in the Rust and TypeScript contracts that carry them (`createdAtMs`), so the encoding is visible from the name. Music, themes, and distraction usage samples use integer milliseconds. Notes pages and blocks keep `created_time` and `last_edited_time` because they mirror the Notion object model. A new field follows its domain convention and documents timezone or precision semantics. Do not claim that every timestamp has a Z suffix.

SQLite foreign-key enforcement is required on every connection. Constraints and triggers protect invariants that must hold regardless of caller. Application services still validate first so users receive domain-specific errors rather than raw constraint failures.

Foreign keys that must block deleting a referenced row use `ON DELETE NO ACTION`, not `RESTRICT`. SQLite checks `RESTRICT` immediately, so a cascade that removes both rows in one statement could fail depending on table creation order; `NO ACTION` is checked when the statement ends, which keeps deletes independent of table order.

## Naming and layout

Table names are plural snake case and start with their owner. Domains that hold many independent aggregates use the domain name (`notes_`, `music_`, `chat_`, `pomodoro_`, `distractions_`, `calendar_`); child tables of a single aggregate root use the root in singular form (`project_tasks`, `quick_note_tags`, `theme_tokens`). Preserved iCalendar data uses `icalendar_` because it mirrors RFC 5545 objects rather than app concepts. Booleans are `INTEGER` 0 or 1 guarded by a `CHECK`, and index names follow `idx_<table>_<purpose>`.

The baseline groups tables by domain with parents before children, follows each table with its indexes, ends each section with its triggers, and seeds data last. New migrations keep new objects in the same naming scheme.

## Canonical, derived, and device-local state

Canonical tables contain user-authored state or durable execution evidence. Search indexes, projections, and summaries may be persisted but remain rebuildable from canonical rows. Derived tables need an invalidation and rebuild path.

External absolute paths, provider homes, executable paths, process state, and native credential material are device-local and do not belong in the vault database. Portable rows refer to logical identities whose current device binding is revalidated before use.

See [Data architecture](../architecture.md) for the complete source-of-truth model and [Data invariants](../invariants.md) for cross-domain rules.

## Documentation rule

Add a schema note when a relationship, identity, deletion effect, migration decision, or recovery rule would be difficult to infer safely from SQL alone. Do not add routine field inventories. When a migration changes one of these contracts, update the smallest domain document in the same change.
