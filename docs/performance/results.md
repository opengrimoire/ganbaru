# Performance records

This is the canonical performance record for Ganbaru AI. Rows are historical measurements, not claims about the current repository head. Preserve recorded rows and benchmark identifiers so later measurements remain comparable.

Recording procedure, identifiers, dataset definitions, output shape, and versioning rules live in the [performance entry point](README.md) and [benchmark harness](harness.md).

## Benchmark records

Latest canonical baseline: `2026-06-02-01`.

Canonical rows keep the statistics and memory buckets that can support long-run comparisons. Raw harness diagnostics such as per-action counters, fixed guard timings, and redundant averages are not preserved here unless they answer a specific performance question. Interaction rows use the realistic dense current-window dataset unless the benchmark is explicitly asking about empty-state or total-history behavior.

### Run metadata

| Run | Harness | Anchor date | Build ref | Platform | Notes |
|---|---|---|---|---|---|
| 2026-05-12-01 | v1 | 2026-05-12 | 0.1.0+b75c37a | Linux Ubuntu 24.04.4 LTS |  |
| 2026-05-16-01 | v1 | 2026-05-16 | 0.1.0+043fa1c-dirty | Linux Ubuntu 24.04.4 LTS |  |
| 2026-05-21-01 | v1 | 2026-05-21 | 0.1.0+89e2765 | Linux Ubuntu 24.04.4 LTS |  |
| 2026-06-02-01 | v1 | 2026-06-02 | 0.1.0+d946cea | Linux Ubuntu 24.04.4 LTS |  |

### Startup boot

Use `Launch median ms` as the headline app-open comparison value. `Usable paint median ms` marks when the app is ready for interaction.

| Run | Dataset | Runs | Usable paint median ms | Launch median ms | Launch P95 ms |
|---|---|---:|---:|---:|---:|
| 2026-05-12-01 | base-0 | 5 | 293 | 887 | 931 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | 5 | 667 | 1253 | 1664 |
| 2026-05-12-01 | dense-v1-r10y-s1-d1 | 5 | 797 | 1337 | 1753 |
| 2026-05-16-01 | base-0 | 5 | 319 | 912 | 1417 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | 5 | 887 | 1502 | 1915 |
| 2026-05-16-01 | dense-v1-r10y-s1-d1 | 5 | 1237 | 1877 | 2237 |
| 2026-05-21-01 | base-0 | 5 | 344 | 932 | 1309 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | 5 | 884 | 1442 | 1812 |
| 2026-05-21-01 | dense-v1-r10y-s1-d1 | 5 | 1047 | 2073 | 2087 |
| 2026-06-02-01 | base-0 | 5 | 363 | 956 | 1040 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | 5 | 995 | 1568 | 1700 |
| 2026-06-02-01 | dense-v1-r10y-s1-d1 | 5 | 1845 | 2444 | 2544 |

### Idle memory

Memory uses the best implemented platform metric: PSS on Linux and Working Set on Windows. macOS memory rows must not be recorded until physical footprint sampling is implemented. Record the metric in run notes whenever it is not obvious from the platform. The harness samples idle memory once per second for 30 seconds after the anchored calendar window is ready. `Min` and `Max` are the lowest and highest values observed during that window. `End` is the final sample.

| Run | Dataset | Statistic | Backend MB | Frontend MB | Network MB | Total MB |
|---|---|---|---:|---:|---:|---:|
| 2026-05-12-01 | base-0 | Min | 111.9 | 193.6 | 19.3 | 325 |
| 2026-05-12-01 | base-0 | Max | 112.5 | 196.0 | 19.4 | 328 |
| 2026-05-12-01 | base-0 | End | 111.9 | 194.6 | 19.4 | 326 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | Min | 115.3 | 242.7 | 19.0 | 378 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | Max | 115.9 | 254.9 | 19.1 | 390 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | End | 115.3 | 247.5 | 19.1 | 382 |
| 2026-05-12-01 | dense-v1-r10y-s1-d1 | Min | 114.7 | 247.8 | 19.3 | 382 |
| 2026-05-12-01 | dense-v1-r10y-s1-d1 | Max | 115.3 | 262.0 | 19.4 | 397 |
| 2026-05-12-01 | dense-v1-r10y-s1-d1 | End | 114.7 | 250.4 | 19.4 | 384 |
| 2026-05-16-01 | base-0 | Min | 118.3 | 201.1 | 20.9 | 340 |
| 2026-05-16-01 | base-0 | Max | 118.9 | 205.8 | 21.0 | 345 |
| 2026-05-16-01 | base-0 | End | 118.3 | 202.0 | 21.0 | 341 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | Min | 121.8 | 270.0 | 21.2 | 414 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | Max | 122.6 | 287.7 | 21.3 | 431 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | End | 121.8 | 287.4 | 21.3 | 430 |
| 2026-05-16-01 | dense-v1-r10y-s1-d1 | Min | 122.1 | 275.6 | 20.9 | 419 |
| 2026-05-16-01 | dense-v1-r10y-s1-d1 | Max | 122.6 | 288.3 | 21.0 | 431 |
| 2026-05-16-01 | dense-v1-r10y-s1-d1 | End | 122.1 | 284.9 | 21.0 | 428 |
| 2026-05-21-01 | base-0 | Min | 106.6 | 190.8 | 17.3 | 315 |
| 2026-05-21-01 | base-0 | Max | 107.2 | 193.4 | 17.4 | 318 |
| 2026-05-21-01 | base-0 | End | 106.6 | 191.6 | 17.4 | 316 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | Min | 110.0 | 260.3 | 17.3 | 388 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | Max | 110.1 | 269.5 | 17.4 | 397 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | End | 110.1 | 262.6 | 17.4 | 390 |
| 2026-05-21-01 | dense-v1-r10y-s1-d1 | Min | 111.2 | 260.6 | 17.4 | 389 |
| 2026-05-21-01 | dense-v1-r10y-s1-d1 | Max | 111.3 | 273.4 | 17.5 | 402 |
| 2026-05-21-01 | dense-v1-r10y-s1-d1 | End | 111.3 | 262.3 | 17.5 | 391 |
| 2026-06-02-01 | base-0 | Min | 118.5 | 205.1 | 20.1 | 344 |
| 2026-06-02-01 | base-0 | Max | 119.3 | 209.2 | 20.2 | 348 |
| 2026-06-02-01 | base-0 | End | 118.8 | 209.2 | 20.2 | 348 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | Min | 130.4 | 277.0 | 20.1 | 427 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | Max | 130.4 | 287.0 | 20.2 | 438 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | End | 130.4 | 278.9 | 20.2 | 429 |
| 2026-06-02-01 | dense-v1-r10y-s1-d1 | Min | 129.7 | 281.1 | 20.1 | 432 |
| 2026-06-02-01 | dense-v1-r10y-s1-d1 | Max | 130.4 | 290.1 | 20.2 | 440 |
| 2026-06-02-01 | dense-v1-r10y-s1-d1 | End | 129.7 | 288.2 | 20.2 | 438 |

### Calendar held navigation memory

This records post-action memory after reproducing real held right-arrow navigation in week view against a practical full visible window. The harness holds right arrow for the fixed duration, releases it, then samples memory once per second for 30 seconds. `Min` and `Max` are the lowest and highest values observed during that window. `End` is the final sample.

| Run | Dataset | Statistic | Backend MB | Frontend MB | Network MB | Total MB |
|---|---|---|---:|---:|---:|---:|
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | Min | 116.0 | 269.5 | 19.3 | 405 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | Max | 116.7 | 313.8 | 19.3 | 450 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | End | 116.0 | 273.2 | 19.3 | 408 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | Min | 120.8 | 312.7 | 20.9 | 455 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | Max | 121.5 | 373.5 | 21.0 | 516 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | End | 120.8 | 315.1 | 21.0 | 457 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | Min | 110.8 | 362.7 | 17.4 | 491 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | Max | 111.4 | 371.1 | 17.4 | 500 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | End | 110.8 | 363.2 | 17.4 | 491 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | Min | 145.0 | 354.2 | 20.2 | 519 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | Max | 146.3 | 398.1 | 20.3 | 565 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | End | 145.0 | 355.9 | 20.3 | 521 |

### Calendar panel latency

Rows report user-visible panel-open elapsed time for the two calendar panel actions with a practical full visible window. The fixed repetition count is defined by the harness spec and is not repeated in this canonical table.

| Run | Dataset | Action | Median ms | P95 ms |
|---|---|---|---:|---:|
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | click existing event | 88 | 101 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | click empty time slot | 100 | 105 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | click existing event | 156 | 191 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | click empty time slot | 159 | 175 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | click existing event | 121 | 133 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | click empty time slot | 138 | 155 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | click existing event | 127 | 147 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | click empty time slot | 148 | 168 |

### Calendar import operations

Rows report one 1000-event add or update pass against the practical dense dataset. Smaller repeated import rows are diagnostic variance checks and are not part of the long-run record.

| Run | Dataset | Metric | Value ms |
|---|---|---|---:|
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | bulk import 1000 add | 305 |
| 2026-05-12-01 | dense-v1-r1y-s1-d1 | bulk import 1000 update | 330 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | bulk import 1000 add | 592 |
| 2026-05-16-01 | dense-v1-r1y-s1-d1 | bulk import 1000 update | 584 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | bulk import 1000 add | 459 |
| 2026-05-21-01 | dense-v1-r1y-s1-d1 | bulk import 1000 update | 474 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | bulk import 1000 add | 848 |
| 2026-06-02-01 | dense-v1-r1y-s1-d1 | bulk import 1000 update | 902 |

## Package size

Package size is not produced by the benchmark harness, but it is deterministic enough to track here. Use decimal MB from byte size.

```bash
stat -c "%n %s" target/release/bundle/deb/*.deb target/release/bundle/rpm/*.rpm target/release/bundle/appimage/*.AppImage
```

| Date | Phase | What changed | Artifact | MB | Build host |
|---|---|---|---|---:|---|
| 2026-04-02 | Phase 1 | Baseline: calendar, pomodoro, kanban, performance panel | .deb | 7.0 | Linux |
| 2026-05-03 | Phase 1 | Event panel polish and startup memory work | .deb | 7.4 | Linux |
| 2026-05-03 | Phase 1 | Event panel polish and startup memory work | .rpm | 7.4 | Linux |
| 2026-05-03 | Phase 1 | Event panel polish and startup memory work | .AppImage | 80.9 | Linux |
| 2026-05-12 | Phase 1 | Harness v1 benchmark baseline | .deb | 8.1 | Linux |
| 2026-05-12 | Phase 1 | Harness v1 benchmark baseline | .rpm | 8.1 | Linux |
| 2026-05-12 | Phase 1 | Harness v1 benchmark baseline | .AppImage | 83.2 | Linux |
| 2026-06-02 | Phase 1 | Harness v1 benchmark run | .deb | 20.8 | Linux |
| 2026-06-02 | Phase 1 | Harness v1 benchmark run | .rpm | 20.8 | Linux |
| 2026-06-02 | Phase 1 | Harness v1 benchmark run | .AppImage | 95.4 | Linux |
