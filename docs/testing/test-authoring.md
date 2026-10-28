# Test authoring

Useful tests protect behavior, invariants, failure modes, security boundaries, and user-visible contracts. They do not exist only to raise a count or percentage.

## General cases

Consider:

- Boundary values and realistic maximum sizes.
- Invalid, missing, stale, truncated, duplicated, and unknown input.
- Transaction rollback and partial failure.
- Restart, recovery, retry, cancellation, and idempotency.
- Ordering, pagination, deduplication, and stable identity.
- Permission, path, URL, protocol, redaction, and disclosure boundaries.
- Import and export round trips and interoperability fixtures.
- State-machine transitions and forbidden transitions.
- Cross-platform path, timing, and process behavior.

Inspect nearby tests before adding a new one and match their fixture style and depth.

## Frontend

Frontend tests use Vitest and live next to source with a `.test.ts` suffix. Keep the Node environment unless DOM behavior is part of the contract. jsdom adds startup and memory cost.

Pure domain decisions are the preferred boundary. Extracting a pure function is valuable when it clarifies behavior, not only to make a test possible. Use component tests for rendering, events, accessibility, loading, focus, or Svelte state integration.

Mock the narrowest external boundary. Keep representative integration tests for important first-use and component flows. Do not globally disable Vitest isolation without auditing Svelte stores, module caches, timers, DOM globals, and mocks for order dependence.

Production wiring and chunk placement belong to type checks and bundle contracts, not broad component-registry imports in unit tests.

## Rust and SQLite

Use library tests unless behavior belongs specifically to a binary. Add `--lib` to focused Cargo commands so unrelated targets are not built.

Persistence tests use isolated temporary databases with the current schema. The full migration chain costs over ten seconds in a debug build, so tests that only need a migrated database copy a template migrated once per test process through the `test-support` feature of `ganbaru-db`: in memory, or written to a new vault file with `write_migrated_database`. Upgrade tests start from a template migrated through an earlier version and then run the real later migrations. Tests of database creation, migration on open, and the schema invariants run the real chain, so each test process still runs it at least once. Migration tests exercise actual migration files and SQLx checksums.

`crates/ganbaru-db/schema-catalog.txt` holds the schema catalog of the complete migration chain, which schema validation uses instead of rebuilding it. After adding a migration, regenerate it with `GANBARU_WRITE_SCHEMA_CATALOG=1 cargo test -p ganbaru-db --lib committed_snapshot_matches_the_migration_chain` and commit the result; without it, that test fails and validation of current vaults falls back to running the chain.

Fixture optimizations must preserve isolation, foreign keys, migration order, transaction semantics, and cleanup on supported platforms. Do not rewrite an applied migration to simplify a test.

Standard Cargo commands remain portable. An optional linker optimization must retain the standard linker fallback and be measured before repository-wide use.

Provider probe tests must configure their own temporary provider homes and explicit executable paths instead of depending on installed CLIs, `PATH`, or a developer's home directory. Cover invalid-home and missing-executable results separately because configuration validation can fail before executable discovery.

## Authorization and security

Authorization tests exercise intersections, not isolated positive flags. For Chat this includes participant capability, history boundary, destination audience, folder tier, execution target, scratch generation, immutable authorization revision, provider enforcement, host-tool scope, continuation, revocation, and result publication.

Cross-channel tests must prove that references, summaries, schedules, and retained context do not widen the destination audience. Filesystem tests cover traversal, symbolic links, absolute paths, worktrees, secondary folders, and cleanup.

The normative model is [Chat access control](../data/access-control.md).

Do not shrink realistic security, data-volume, or interoperability bounds only to shorten a suite. Isolate an expensive workload behind an explicit command when it should not run in normal validation.

## UI and platform acceptance

Automated tests do not prove that the real Tauri or Android application looks and behaves correctly across native windows, WebViews, system services, input methods, display configurations, and operating-system settings.

Use focused checks while iterating. User or maintainer acceptance should cover real target platforms and the feature-specific matrices under `docs/testing/`.

Do not launch a development server, Tauri app, or HTTP smoke check as a substitute for requested visual inspection. Pure responsive helper tests are useful when layout depends on measured space, anchors, or collision decisions.

## Coverage

Generate frontend coverage with:

```sh
pnpm --dir apps/client run test:coverage
```

Coverage is a diagnostic. Use it to locate untested decisions and branches, not to justify shallow assertions or pursue a percentage without regard to risk. The repository does not define a root Rust coverage command.
