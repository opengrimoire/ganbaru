# Benchmark harness

The benchmark harness is a dev-visible mechanism inside the desktop app for taking deterministic, comparable performance measurements. It exists because manual timing and one-off RAM snapshots are too noisy for cross-build decisions.

Harness v1 (`HARNESS_VERSION = "1"`, dense dataset `v1`) is the current recorded baseline since the 2026-05-12 reset. It runs scenarios against an isolated benchmark database, records only the measurement each scenario needs, shows readable summary tables, and copies normalized Markdown for [performance records](results.md). Startup and idle memory run both dense datasets, `dense-v1-r1y-s1-d1` and `dense-v1-r10y-s1-d1`. Practical user-window scenarios run only `dense-v1-r1y-s1-d1`.

Harness and dataset versions are tied to recorded rows, not local iteration. Do not bump `HARNESS_VERSION`, `DENSE_DATASET_VERSION`, or dense detail profile names until the current version has at least one recorded run. While tuning an unrecorded benchmark shape, edit the current version in place.

## User flow

1. Build and install a release package, then open the installed app.
2. Open diagnostics with `Ctrl + Shift + D` or the optional title-bar button.
3. Click `Run core benchmarks`, `Run backend benchmarks`, or `Run all benchmarks`. Each suite row can also expand to run individual scenarios.
4. Leave the app alone while the overlay runs. In-app close affordances are blocked during a run.
5. Review the summary tables, then click `Copy markdown` and place the rows in the performance record.

The harness fires a desktop notification when it reaches the summary or error state.

## Data safety

The harness uses device-local `app_config_dir/benchmark.sqlite` for the whole run. The user's real database and Ganbaru AI folder are never opened during benchmark passes.

- The benchmark database and its sidecars are deleted before a run starts and when the summary closes, the run is cancelled, or stale state is detected.
- `vaultMode: "benchmark"` in `benchmark-state.json` tells the database URL resolver to open the benchmark database.
- The app restarts after teardown so the Rust database layer returns to the real user database.
- Stale state is discarded when the harness or dataset version changed, the total run TTL expired, or a pending restart was not claimed within its short TTL.

## Measurement kinds and dataset ids

Each scenario declares one primary measurement kind so it does not spend time on unrelated waits:

| Kind | Captures | Memory observation |
|---|---|---|
| `startup` | Repeated process launches to usable calendar paint after a closed-process cooldown | No |
| `idle-memory` | Min, max, and end memory during the idle observation window | Yes |
| `stress-memory` | Min, max, and end memory after the fixed user action | Yes |
| `interaction-latency` | Repeated UI action timings | No |
| `operation-latency` | Repeated or single operation timings | No |

Memory scenarios capture one sample per second for 30 seconds. A failed sample is a harness failure, not a reportable `n/a` row.

Result tables use compact dataset ids, never prose labels:

| Pattern | Meaning |
|---|---|
| `base-N` | The benchmark database after scenario setup and before dense seeding, with `N` calendar events at measurement start. |
| `dense-vX-rYy-sZ-dP` | Dense dataset version `vX`, `Y` years before and after the run anchor date, `Z` overlapping timed events at each hourly start, and detail profile `dP`. |

## Boot path and anchor date

A benchmark boot must load exactly one calendar window, or memory rows would include the user's current week as well as the scenario window. On boot, desktop startup reads the persisted benchmark state. When a pending pass exists, the app resolves the benchmark database, loads the overlay and runner, and skips the normal current-week hydration; the scenario setup loads the anchor window itself. Running, stale, invalid, or missing state falls back to the normal boot on the user's real database.

The anchor is the user's local date when they confirm the run. It is persisted as `anchorDate`, carried through every restart, and copied into run metadata, so a suite crossing midnight still uses one anchor. Using the real date keeps the temporal context realistic: anchor-day events are today, earlier rows are past history. Event counts vary slightly with leap years, which is why the anchor is recorded.

## Suite state machine

Single-scenario runs and suite runs use the same state file. Suite runs add a queue and accumulated results.

```text
idle
  user clicks Run on a scenario or suite
  user confirms
  resolve and persist the local anchor date
  prepare benchmark DB
  write phase-a-pending
  restart app

phase-a-pending
  open isolated empty DB
  skip normal current-week preload
  write phase-a-running
  run baseline pass at the persisted anchor, unless the scenario is dense-only
  for startup only, repeat phase-a-pending until enough launch samples exist
  seed dense-v1-r1y-s1-d1 around the persisted anchor
  write phase-b-pending
  restart app

phase-b-pending
  open isolated seeded DB
  skip normal current-week preload
  write phase-b-running
  run dense pass for the current dataset at the persisted anchor
  for startup only, repeat phase-b-pending until enough launch samples exist
  if the scenario has more dense datasets:
    seed the next dense dataset around the same anchor
    write phase-b-pending with completed dense results
    restart app
  if suite has more scenarios:
    prepare benchmark DB
    write next phase-a-pending with accumulated results and the same anchor
    restart app
  otherwise:
    teardown benchmark DB
    clear benchmark state
    show summary

summary
  user clicks Return to your data
  restart app

phase-a-running or phase-b-running on boot
  previous process was killed mid-pass
  teardown benchmark DB
  clear benchmark state
  continue on the user's real DB
```

The startup scenario records five process launches per dataset; each relaunch exits the app, waits 10 seconds in a helper process, then opens Ganbaru AI again. A pass interrupted by a killed process is not comparable, so the next boot discards it rather than resuming. Pending state carries `startedAt`, which caps the whole run, and `updatedAt`, which caps the restart gap, so a failed relaunch returns to the user's database quickly.

## Lazy loading

Normal startup must not import benchmark implementation code. The title bar loads only the benchmark status store, and the performance popover loads only scenario metadata. The runner, dense generator, scenario modules, and operation helpers load when a user starts a benchmark or when a pending state resumes. `registry.ts` is the boundary: `BENCHMARK_SCENARIOS` holds metadata only, and `loadScenarioById()` owns the dynamic imports.

## Scenario contract

Each scenario has dependency-light metadata in `registry.ts` and one executable module implementing `BenchmarkScenario` from `types.ts` (`setup`, `runWorkload`, `seed`, `cleanup`). Rules:

- Metadata must not import scenario modules, stores, dense generators, or operation helpers.
- `setup()` places the app in the same deterministic starting state for every pass.
- Calendar scenarios use `context.anchorDate`, never a fixed date or a fresh `new Date()`.
- `runWorkload()` performs the measured action and honors the abort signal. Fixed-window workloads stay alive for `workload.durationMs`.
- Metrics are scalar and non-sensitive. Never record titles, notes, URLs, or other user content.
- `seed()` writes only to the isolated benchmark database. `cleanup()` is usually a no-op because the database is deleted after the run.

## Registered scenarios

| Suite | Scenarios | Purpose |
|---|---|---|
| Core benchmarks | `startup-boot`, `idle-memory`, `calendar-nav`, `calendar-panel-latency` | User-perceived startup, memory, and interaction latency |
| Backend benchmarks | `calendar-import-ops` | Rust-backed calendar import latency |
| All benchmarks | Core plus backend | Complete release baseline or broad refactor validation |

| Scenario | Datasets | Primary measurement |
|---|---|---|
| `startup-boot` | Baseline plus both dense datasets | Repeated launch to usable calendar paint |
| `idle-memory` | Baseline plus both dense datasets | Anchored week idle RAM |
| `calendar-nav` | Practical dense only | Memory after a real 3-second held right-arrow navigation in week view |
| `calendar-panel-latency` | Practical dense only | Panel open latency for existing-event and create actions |
| `calendar-import-ops` | Practical dense only | Rust-backed 1000-event import add and update latency |

`calendar-nav` dispatches real `ArrowRight` key events so the benchmark follows the same held-navigation controller and readiness gate as a physical key hold. `calendar-panel-latency` runs 50 samples per action and opens many different visible events, so a warmed detail cache is not measured as the normal case.

## Dense calendar dataset

`dense-v1` is deterministic for a given anchor date. It covers the half-open range from `anchor - yearRadius` to `anchor + yearRadius` and intentionally avoids recurrence, so it isolates dense visible windows and long non-recurring history.

- Every day has `stackCount` one-hour timed events at each hour from `00:00` through `23:00`, plus three all-day events.
- Timed events use the adaptive Pomodoro preset (40-minute focus, 5-minute short break, 10-minute long break).
- Detail profile `d1` gives timed and all-day events realistic metadata, including descriptions, alarms, locations, categories, organizers, attendees, and extended properties. Colors cycle through every palette slot.
- Timed events before the anchor day receive completed Pomodoro runs and segments, seeded natively by `benchmark/seed.rs`.
- Source UIDs do not include the year radius, so seeding the 10-year dataset after the 1-year dataset adds only the outer years.

Week-view scenarios load the visible Monday to Sunday week plus one day on each side, matching the calendar store's real render window. With `s1-d1` that is 216 timed and 27 all-day rows. After dense rows are recorded, any generator change requires a new dense dataset version and a fresh row series.

## Output format

The overlay renders a readable HTML summary, and `Copy markdown` copies one suite-level canonical block:

- A header with the unresolved `YYYY-MM-DD-ID` run placeholder. Replace `-ID` with the next zero-padded suffix for that date when recording.
- One `Run metadata` table with run id, harness, anchor date, build ref, platform, and notes. It is the only generated table with a `Notes` column.
- One section per recorded scenario, containing only its canonical table: launch statistics for startup, `Min`, `Max`, and `End` rows for memory, and repeated latency rows for interaction and operation scenarios.

Diagnostics such as move counts, skipped ticks, raw averages, and smaller import runs are shown on screen but not copied. A future comparable detail should become a typed column rather than a generic note.

`Launch median ms` measures Rust process start through `boot.usable-paint`, when calendar data has loaded and rendered a frame. The 10-second closed-process cooldown reduces instant-relaunch cache bias without pretending to be a first launch after reboot. `buildRef` combines the app version and short git commit, with `-dirty` for a dirty worktree. The platform label comes from the native memory report.

## Critical files

- `apps/client/src/lib/benchmark/types.ts`: scenario contract, versions, sampling constants, and state types.
- `apps/client/src/lib/benchmark/dense.ts` and `pomodoro-history.ts`: deterministic dense calendar and Pomodoro history generation.
- `apps/client/src/lib/benchmark/registry.ts`: scenario metadata, suites, and dynamic loaders.
- `apps/client/src/lib/benchmark/runner.ts`: pass orchestration, persisted state, and restart wiring.
- `apps/client/src/lib/benchmark/sampler.ts`: memory sampling and boot timing capture.
- `apps/client/src/lib/benchmark/output.ts`: canonical Markdown formatter.
- `apps/client/src/lib/stores/benchmark-runner.svelte.ts`: UI-facing runner state and suite continuation.
- `apps/client/src/lib/components/diagnostics/BenchmarkOverlay.svelte`: confirmation, running, summary, and error overlays.
- `apps/client/src/lib/components/diagnostics/PerformancePopover.svelte`: benchmark buttons.
- `apps/client/src/lib/calendar/nav-handle.svelte.ts`: headless calendar driver for scenarios.
- `apps/client/src/lib/api/db.ts`: benchmark database selection through `vaultMode`.
- `apps/client/src-tauri/app/src/benchmark.rs` and `benchmark/memory.rs`: benchmark state commands, database prepare and teardown, and memory report. Restart and startup timing stay in `runtime/desktop.rs`.
- `apps/client/src-tauri/app/src/benchmark/seed.rs`: native Pomodoro history seeding and a dense Music library fixture command that no registered scenario uses yet.

## Constraints

- Benchmarks are not CI. They require a real graphical environment and are affected by WebKit or WebView2 GC heuristics.
- Compare runs on the same host, OS, WebView engine version, window state, and power mode.
- Do not interact with the app during a run.
- Benchmark rows are cross-build comparison signals, not claims about one user's real database.
