# Performance

Performance documentation separates repeatable methodology from historical evidence.

- [Benchmark harness](harness.md): dataset versions, scenarios, state machine, and copied output contract.
- [Performance records](results.md): immutable benchmark rows and package-size history.
- [Calendar migration CPU measurements](calendar-migration.md): dated before and after engine CPU records from moving recurrence expansion and edit preview to Rust, separate from release interaction benchmarks.

Recorded rows are historical evidence. Do not present them as the performance of the current repository head without a new comparable run.

## Principles

- Measure installed release builds, not development mode.
- Keep benchmark data isolated from the user's active vault.
- Compare runs only when machine, operating system, WebView, power mode, window state, harness version, and dataset are compatible.
- Treat startup, idle memory, post-interaction memory, visible latency, and operation throughput as different questions.
- Prefer fixing measured cost over hiding warnings or raising limits.
- Bound hot paths by the visible window or requested page, not total stored history.
- Keep uncommon diagnostics, editors, transfer flows, grammars, and large catalogs lazy unless a measured common interaction justifies residency.
- Record exact methodology changes before comparing results across versions.

## Recording a run

1. Build and install a release package, then open diagnostics as described in the [benchmark harness](harness.md).
2. Use the isolated benchmark vault and one registered dataset identifier.
3. Run the complete scenario contract for the measurement being recorded.
4. Copy the normalized Markdown produced by the harness.
5. Replace the `YYYY-MM-DD-ID` placeholder with the next zero-padded run suffix for that date.
6. Add rows to [performance records](results.md) without rewriting earlier runs.
7. Record the platform metric and environment notes needed for comparison.

Do not bump `HARNESS_VERSION`, `DENSE_DATASET_VERSION`, or a recorded dataset profile while tuning an unrecorded shape. Bump only when a later methodology or workload change makes new rows incomparable with existing records.

## First-use contracts

First-use contracts are deterministic structural budgets, not elapsed-time benchmarks. They cap what opening the empty Projects, Notes, and Chat surfaces may cost: critical IPC calls, SQL reads and writes, serialized response bytes, and the number of source modules in each route's bundle closure. Chat has a module ceiling but no backend baseline yet.

The budgets live in code, not in this document:

- `apps/client/scripts/bundle-contracts/baselines/first-use.json` owns route module ceilings and the no-vault startup boundary.
- `apps/client/src-tauri/app/src/first_use_contracts.rs` owns backend call, statement, and payload limits.
- Bundle contract scripts own route closures, required resident modules, and forbidden platform imports.

A ceiling is not a target to fill. A change that raises one must explain the user-visible benefit and preserve intentional lazy boundaries.

## Memory metrics

Use the most meaningful implemented platform metric:

| Platform | Metric | Interpretation |
| --- | --- | --- |
| Linux | Proportional set size, with RSS fallback | Preferred estimate of total physical cost including a fair share of shared pages |
| Windows | Working set | Resident physical pages, with shared pages potentially counted per process |
| macOS | Physical footprint | Required future metric; do not record misleading substitutes as canonical rows |

Do not compare Linux PSS and Windows Working Set as if they were identical measures.

Process buckets are:

- Backend: Rust, Tauri, SQLite, and native services.
- Frontend: WebView renderer, Svelte, DOM, CSS, and JavaScript heap.
- Network: WebView network process where the platform exposes it.

The frontend bucket includes the browser engine and cannot be cleanly divided into an engine baseline and Ganbaru code at runtime. On Linux, the live Diagnostics chart also includes direct child processes, so a provider executable started by Chat appears as a separate column.

## Output rules

Use normalized tables with one value per column. Keep median and P95 for repeated latency. Use min, max, and end for sampled memory windows. Keep raw averages and internal counters as diagnostics unless the benchmark question specifically concerns them.

Fixed scenario details belong in the harness specification. Historical rows contain only the identifiers and context required to interpret the measurement.
