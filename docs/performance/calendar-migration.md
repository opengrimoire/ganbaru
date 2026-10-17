# Calendar migration CPU measurements

**Reference:** isolated engine CPU diagnostics recorded when recurrence expansion and edit preview moved from TypeScript to Rust. They do not measure installed-app latency, WebView work, Tauri transport, painting, or retained memory, and do not show a general Rust speedup. Release interaction benchmarks are in [performance records](results.md).

## Workload

Both records use the same fixture: 64 UTC daily templates (`baseline-{index}`), each starting `2026-05-01 09:00`, one hour long, with COUNT 365 and no exclusions, additional dates, overrides, imported data, or Focus evidence.

- **Navigation:** 12 consecutive seven-day windows starting May 11; one sample covers all 12 expansions and emits 5,376 occurrences.
- **Preview:** a whole-series title edit selected at `baseline-0::2026-05-15`, with a May 8 scope clock and no active occurrence, over 448 cached visible occurrences.
- **Sampling:** one initial invocation, five warmups, then 30 measured samples; sorted indices 15 and 28 give P50 and P95.

Machine: Linux `7.0.0-31-generic`, AMD Ryzen 3 7320U, 5,926,703,104 bytes of RAM. No other validation ran concurrently.

## TypeScript record, 2026-10-03

Recorded at `2026-10-03T18:11:10.110Z` with Node `v24.15.0`, one Vitest worker, `TZ=UTC`, repository base `6b7c01c7cb0758ea732ba14a008154f7e033fbab`. Preview used the former `computeEditDisplay` path and returned 448 visible events. The measuring test was temporary and has been removed along with the measured sources.

| Former TypeScript CPU path | Initial invocation, ms | Warm P50, ms | Warm P95, ms |
| --- | ---: | ---: | ---: |
| Twelve navigation-window expansions | 178.461 | 144.522 | 154.670 |
| Scoped title-edit preview over cached events | 3.198 | 0.631 | 1.208 |

| Source under `apps/client/src/lib/components/calendar/` (since removed) | SHA-256 |
| --- | --- |
| `recurrence.ts` | `ee0b206fcb06bad0e408357cd8c2dcfeeff6197d060e1cb04422e3318e831dd9` |
| `display-events.ts` | `1c376794deeaa3be80b5b4d6fcebb7a2b48cdcf8d323acd70041496bd6d43304` |
| `recurrence-edit-plan.ts` | `774d613cf0c152fad5fa091ce57587febffda2fa490cb7c808dbfe9cf7897732` |

## Native record, 2026-10-04 UTC

Recorded at `2026-10-04T03:20:35.134751598Z` with Rust `1.98.0 (88d9e12ae 2026-08-18)`, unoptimized Cargo test profile, one job and one test thread. Preview returns seven native source-family occurrences plus 441 unchanged cached ones (448 cards). Snapshot read and preview CPU are timed separately; serialization (a 6,613-byte preview body) is outside the timer.

| Native diagnostic | Initial invocation, ms | Warm P50, ms | Warm P95, ms |
| --- | ---: | ---: | ---: |
| Twelve canonical geometry-window expansions | 71.802 | 70.040 | 75.971 |
| Complete bounded native preview CPU | 4.633 | 3.363 | 4.167 |
| Preview SQLite snapshot read | 9.277 | 2.952 | 3.672 |

The native preview costs more CPU than the former TypeScript preview because it performs full protection and metadata preparation that the cached frontend calculation skipped. Compiler profiles and result shapes also differ.

The hashes pin the measured sources at record time, so they are intentionally not refreshed. The surviving modules now live under `crates/ganbaru-calendar/src/` (`recurrence/` and `events/`, with the diagnostic in `events/tests/migration_measurements.rs`) and have changed since; a rerun produces a new record rather than updating this one.

| Source at record time under `apps/client/src-tauri/app/src/` | SHA-256 |
| --- | --- |
| `recurrence/canonical.rs` | `21733d637d5dcf0b386eef14176c87c759736664b11c9f6c8edd4cf0699f0968` |
| `recurrence/engine.rs` | `c1b7179e329910d552de3f42c552e68018c9c4550f2e483af7317be179a7bea6` |
| `recurrence/rule.rs` | `7b66360e7f975d79e0f3d79df0b4ce54081e4008de4d390e3aacd990eafc2d2f` |
| `recurrence/time.rs` | `30614445c376e5c4efd1add77c906dc2463e492c9639d4c10a9a92d5d9196946` |
| `calendar_events/preview.rs` | `b808f66873cb1916ac7e52864b5497f62bee503104ae9c116153a4550654770f` |
| `calendar_events/tests/migration_measurements.rs` | `5f51b056fb0e17696cae2f0a7eaaeba21ab9e0063f95c93624af9b1ab4bd4f16` |

The native diagnostic is an ignored test and adds no timing gate to normal validation. Rerun it while other validation is idle:

```sh
cargo test -p ganbaru-tauri-app --lib -j 1 calendar_migration_cpu_measurements -- --ignored --nocapture --test-threads=1
```

Installed desktop and Android held-navigation and editing measurements belong in the [benchmark harness](harness.md) and [performance records](results.md), following the [performance methodology](README.md).
