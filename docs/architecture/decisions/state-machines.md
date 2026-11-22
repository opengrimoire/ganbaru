# Explicit state machines

Lifecycle state is modeled explicitly. When a value moves through phases (a turn, an outbox entry, a save, a linking flow), the phases are one type and every move goes through one transition owner.

## Rules

- One state type per lifecycle. Rust uses an enum whose variants carry the data that only exists in that phase, instead of a status field next to `Option`s or booleans that must agree. TypeScript uses a discriminated union keyed by `phase` (or `kind` when the surrounding code already uses it) with the same rule.
- One transition owner. Rust exposes a pure `transition` (or `can_transition` plus `transition`) that returns an error for illegal moves, following `crates/ganbaru-chat/src/state.rs`. TypeScript keeps the transition logic in a pure module next to its store or component (`*-flow.ts`, `*-state.ts`) with a `.test.ts`; components and stores only dispatch events and render the current variant.
- Persisted states are guarded where they are written. SQL writers update with `WHERE state IN (<legal sources>)` and treat `rows_affected = 0` as a rejected transition, not as success. Invariants that SQLite can check (a status that requires a timestamp, mutually exclusive columns) are CHECK constraints added in a new migration.
- Stale asynchronous results are part of the state. A request token or the target identity lives inside the in-flight variant, so a late result for a previous target cannot be applied.
- Derived flags stay derived. Booleans such as `busy`, `turnActive`, or `isLoading` are computed from the state, never stored beside it.
- Persisted and wire names change only deliberately, together with their owning documentation and every reader (for example the Rust, native messaging, and Android readers of a shared state file).

## Reason

Scattered flags allowed contradictory combinations (an unlink confirmation without a target, a held outbox entry that was also rejected, a session marked ready while still stopping) that no code path intended. A single state type makes those combinations unrepresentable, a single transition owner makes every legal move reviewable in one place, and pure transition functions are cheap to test exhaustively, which matters because agents cannot inspect the running UI.
