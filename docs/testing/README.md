# Testing

Ganbaru uses layered validation across the Svelte frontend, Rust workspace, SQLite persistence, browser extension, Android project, production bundles, and dependency graph.

This document is the command and gate-selection reference. Supporting documents cover:

- [Validation infrastructure](validation-infrastructure.md): ordering, resource limits, caching, bundle contracts, and topology changes.
- [Test authoring](test-authoring.md): useful frontend, Rust, SQLite, authorization, and UI tests.
- [Android acceptance](android.md): physical-device, emulator, permission, lifecycle, and release cases.
- [Local vault handoff](vault-handoff.md): physical desktop and Android round-trip acceptance and result record.
- [Chat provider trust](chat-provider-trust.md): project provider configuration in trusted and untrusted folders, with marker hooks.
- [Calendar recurrence](calendar-recurrence.md): recurrence edit, protection, deletion, and Undo matrices.
- [Notes editor](notes-editor.md): editor, database, transfer, clipboard, and recovery acceptance.
- [Project dependency cascades](project-dependency-cascades.md): dependency date review, apply, retry, and Gantt acceptance.
- [Distraction blocker runtime](distractions.md): desktop and Android runtime acceptance without the WebView.
- [Music desktop](music.md): real audio, lifecycle, and automation acceptance.
- [Desktop windows](desktop-windows.md): detached tab windows, Focus overlays, and Chat previews under per-window permissions.
- [Performance harness](../performance/harness.md): benchmark-only contracts outside normal validation.

## Goals

Validation should protect durable user data and security boundaries, catch behavior and build regressions, preserve platform portability, and remain predictable on resource-constrained machines.

Test count and coverage percentage are not quality targets. Coverage follows product risk, state complexity, interoperability, and the cost of failure.

## Validation layers

- **Frontend tests:** Vitest tests next to source, using Node by default and jsdom only for real DOM behavior. Release and validation scripts under `apps/client/scripts/` keep their `.test.mjs` files beside them in the same suite.
- **Rust tests:** domain, persistence, provider, platform, migration, transaction, recovery, and command-boundary tests in the owning crate.
- **Static checks:** Svelte Check, TypeScript, Rust formatting, Clippy, provider protocol snapshots, and Tailwind diagnostics.
- **Production bundle contracts:** real Vite builds that verify route closures, source-module ceilings, platform composition, and forbidden eager imports.
- **Android project and build contracts:** generated-project policy, isolated Android frontend graph, and an independent ARM64 pull request build.
- **Dependency audits:** pnpm advisories and RustSec, with reviewed Rust exceptions documented under [dependency audits](../data/security/dependency-audits.md).
- **Manual platform acceptance:** real Tauri and Android behavior that automated tests cannot represent faithfully.

## Root commands

Always use the workspace flag for root scripts.

| Command | Purpose |
| --- | --- |
| `pnpm -w run check` | Rust formatting and Clippy, then the workflow signing-isolation and unsafe inventory checks, provider protocol snapshots, Svelte Check, and TypeScript |
| `pnpm -w run test` | Rust tests, then normal frontend tests in sequential shards |
| `pnpm -w run test:benchmark-contracts` | Explicit benchmark fixture and harness contracts, excluded from normal validation |
| `pnpm -w run editor-check` | Tailwind editor-style diagnostics |
| `pnpm -w run bundle-contracts` | Desktop and Android production-build contracts |
| `pnpm -w run audit` | pnpm and Rust dependency audits |
| `pnpm -w run validate` | Complete normal code gate |
| `pnpm -w run validate:rust`, `validate:frontend` | The two halves of `validate`, run as separate CI jobs |
| `pnpm -w run validate:full` | Dependency audits followed by the normal code gate |

`validate` is the normal comprehensive gate. `validate:full` is required for dependency or lockfile changes, security-sensitive dependency work, releases, and explicit full-security requests.

For pull requests and the merge queue, `.github/workflows/check.yml` runs `validate:rust` and `validate:frontend` as parallel Linux jobs with the standard validation profile, and the required `linux validation` check passes only when both pass. Together they are exactly local `validate`. The Rust job restores a dependency cache that only pushes to `dev` save. The independent Android job builds an ARM64 debug APK, while the Windows job runs `cargo check -p ganbaru` without producing an installer.

## Choosing a gate

Start with the narrowest command that can catch a plausible regression.

### Focused frontend behavior

```sh
pnpm --dir apps/client exec vitest run src/path/to/file.test.ts --maxWorkers=1
```

Confirm that Vitest reports only the requested files. Stop and correct the command if the full suite starts.

### Focused Rust behavior

```sh
cargo test -p ganbaru-chat --lib -j 4 test_name -- --test-threads=4
cargo test -p ganbaru-notes --lib -j 4 test_name -- --test-threads=4
cargo test -p ganbaru-db --lib -j 4 test_name -- --test-threads=4
cargo test -p ganbaru-tauri-app --lib -j 4 test_name -- --test-threads=4
```

Substitute `ganbaru-chat-contracts`, `ganbaru-chat-providers`, `ganbaru-pomodoro`, `ganbaru-working-folders`, `ganbaru-contacts`, `ganbaru-sync-contracts`, `ganbaru-sync`, or `ganbaru-sync-replica` when that crate owns the behavior.

### Focused sync behavior

```sh
cargo test -p ganbaru-sync-contracts --lib -j 4 -- --test-threads=4
cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4
cargo test -p ganbaru-sync --lib -j 4 sim -- --test-threads=4
cargo test -p ganbaru-sync-replica --lib -j 4 -- --test-threads=4
cargo test -p ganbaru-tauri-app --lib -j 4 sync -- --test-threads=4
```

The contracts crate owns golden operation vectors, mutation tests, and order key fuzzing. A golden vector mismatch means the operation encoding changed by accident, because every stored log depends on it; regenerate vectors with `GANBARU_SYNC_BLESS_VECTORS=1` only when deliberately introducing a new format version. The engine's conformance tests compare every table, column, and capture trigger of a freshly migrated database with the sync manifest.

The engine simulation replays seeded runs of several installations that edit Quick notes, partly offline, through a hub over a network that drops, duplicates, and reorders messages, with crash failpoints, clock skew, and revocations. It checks database invariants after every step and identical replicated state at quiescence. A second, peer topology simulation has the anchor, two admin desktops, and phones exchange with random peers under partitions, crashes, and anchor outages, while admins remove members that keep editing, remove each other, race handovers, restore or clone the anchor, quarantine members, and resolve late changes; every installation still enrolled must converge, including authorization state and late changes. Each simulation runs 48 seeds of 60 steps in four shards during normal validation. `GANBARU_SYNC_SOAK_SEEDS` sets a larger seed count for a soak run, and `GANBARU_SYNC_SEED` replays the single seed that a failing run prints. The simulation keeps one replica set in memory at a time, so a soak run stays within the single-thread memory budget.

The engine's desktop performance budgets (capture overhead per write, seal of 100 changed notes, apply of a 256-operation batch, a removal excluding 1,024 applied operations, a fork exclusion of 1,024 applied operations on a member and on the anchor, and the apply cost of retaining displaced versions) are ignored tests that assert only in the release profile, on the median of several runs: `cargo test -p ganbaru-sync --lib --release -j 1 budgets -- --ignored --nocapture --test-threads=1`.

Use the native messaging binary only for binary-local tests:

```sh
cargo test -p ganbaru-linking-firewall --bin ganbaru-linking-firewall -j 4 test_name -- --test-threads=4
cargo test -p ganbaru-native-messaging --bin ganbaru-native-messaging -j 4 test_name -- --test-threads=4
```

Check desktop composition after changing a core crate or Tauri adapter:

```sh
cargo check -p ganbaru --bin ganbaru -j 4
```

### Focused static feedback

```sh
pnpm --dir apps/client run check
cargo fmt --check
cargo clippy --workspace -j 4 -- -D warnings
pnpm -w run editor-check
```

### Small UI and documentation changes

Do not run the complete gate solely because a small UI or documentation edit is finished. Use static checks when Svelte, TypeScript, Tailwind, or shared UI structure can be affected. Add focused tests when behavior changes.

Mechanically obvious prose, copy, or link changes need no code validation unless a workflow requires it.

### Data-sensitive changes

Backend persistence, SQLite, import, export, migrations, membership, note saving, and other data-loss-sensitive changes require focused tests during implementation and a broader completion gate proportional to risk.

Tests must cover current-format state, stale rows, unknown values, supported external imports, rollback, partial failure, and idempotency. Unsupported pre-user internal formats should be rejected explicitly, not maintained through compatibility fixtures. Preserve regression fixtures that exercise current production behavior.

### Pull requests and releases

Run `pnpm -w run validate` before a normal pull request and before other risk-sensitive integration work. Run `pnpm -w run validate:full` when dependencies, lockfiles, audits, release security, or explicit instructions require it.

If a batch already passed the required gate, do not repeat it unless later changes materially affect covered behavior.

## Execution constraints

Broad root scripts run Rust and frontend work one after the other. Do not start Cargo, Vitest, Svelte Check, Turbo, or another broad validation command while `check`, `test`, `validate`, or `validate:full` is active.

Focused Cargo commands use four build jobs and test threads, or one of each under the low-memory validation profile. Focused Vitest uses one worker. Do not increase broad concurrency, combine Rust and frontend stages, or remove sharding without measuring peak memory and confirming unchanged coverage.

See [Validation infrastructure](validation-infrastructure.md) for ordering, heap limits, caches, bundle contracts, and topology changes.

## Writing tests

Test names describe observable behavior. Prefer pure decisions and narrow external boundaries. Cover invalid input, boundaries, ordering, recovery, permissions, transactions, interoperability, and forbidden transitions, not shallow existence.

See [Test authoring](test-authoring.md) for frontend, Rust, SQLite, security, UI, and coverage guidance.
