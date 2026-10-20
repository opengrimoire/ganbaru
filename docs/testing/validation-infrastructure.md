# Validation infrastructure

This document explains the resource, ordering, caching, and production-build constraints behind the commands in [Testing](README.md).

## Rust baseline

The workspace uses Rust edition 2024 and Cargo resolver 3. `Cargo.toml` declares Rust 1.98 as the supported minimum; `rust-toolchain.toml` pins compiler 1.98.0 with Clippy and rustfmt for local, pull request, and release validation. Shared package metadata is inherited from the workspace, while package versions remain independent. `rustfmt.toml` explicitly selects the 2024 formatting style for consistent CLI and editor output. Format the entire Rust workspace with `cargo fmt --all`.

Run commands through rustup without a conflicting toolchain override. CI installs the repository-selected toolchain with `rustup show`, then adds the targets required by each job. A Linux gate does not replace the independent Windows composition check or Android APK build. macOS and iOS are future platforms and are not validated by these jobs.

Toolchain or edition changes require `validate:full` and the platform checks. Compatibility diagnostics must be reviewed for temporary lifetimes, lock and resource cleanup, and native unsafe boundaries rather than fixed mechanically to suppress warnings.

The shared Clippy policy allows `collapsible_if`: nested guards and edition-2024 let-chains are both valid styles, so choose the form that makes control flow and resource scope clearest. Other compiler and Clippy warnings remain errors in the normal gate.

## Root execution order

The complete normal gate runs in this order. Concurrency comes from the validation profile described below.

1. Rust formatting and Clippy.
2. Rust workspace tests. Running tests peaks far below compilation (under 500 MB), so test threads cost little memory.
3. Provider protocol snapshot checks, Svelte Check with a 3,072 MiB Node old-space limit, then TypeScript checking.
4. Sequential Vitest shards, excluding benchmark-harness tests.
5. Tailwind diagnostics through Turbo.
6. Desktop and Android production builds and bundle contracts through Turbo.

Rust runs first because compiler and linker peaks are less predictable. Rust and frontend tools do not overlap. Sequential Vitest shards release transformed module graphs between groups.

### Validation profiles

`GANBARU_VALIDATE_PROFILE` selects the concurrency of Clippy, Rust tests, and Vitest; `apps/client/scripts/validation/run-profiled.mjs` owns the exact values and rejects unknown names.

| Profile | Cargo build jobs | Rust test threads | Vitest |
| --- | --- | --- | --- |
| `standard` (default) | 4 | 4 | Two sequential two-worker shards |
| `low-memory` | 1 | 2 | Four sequential one-worker shards |

The standard profile peaked at 5.2 GB of used system memory on a 7.6 GiB machine running nothing else, and is also what CI runs, so local and CI results match. Fast validation is the default because most contributor machines have that memory free. Set `low-memory` permanently in the shell environment of a machine that cannot spare it while a browser or other applications run; it trades wall time for one compiler process at a time. The choice is left to the machine owner rather than detected, because free memory changes during a run.

Hosted Linux CI runs the same complete gate as two parallel jobs on separate 16 GiB runners, `validate:rust` and `validate:frontend`, with the standard profile. Each job reports the runner memory peak. The Rust job restores registry, Git, and dependency build artifacts from a cache keyed by the toolchain and `Cargo.lock`; only trusted pushes to `dev` save it, after removing workspace crate artifacts, and a push without an exact match builds cold so stale artifacts never accumulate. CI does not substitute static checks for regression tests or bundle contracts.

Benchmark fixture and harness contracts are deliberately outside `validate`. Run `pnpm -w run test:benchmark-contracts` when changing the harness. Performance measurement remains manual release-build work.

## Resource policy

The 3,072 MiB Svelte Check heap leaves headroom over the measured live heap of about 1,916 MiB after a full collection (whole-process resident peak about 2.5 GiB under this limit). A limit within a few hundred MiB of the live heap fails intermittently, because whether a run survives then depends on collection timing. The checked program holds only app source, tests, and the libraries they import, so the heap tracks ordinary codebase growth. Treat exhaustion as a controlled failure. Investigate graph growth and checker topology before raising it again.

Cargo development builds keep only line tables as debug information, and the test profile inherits that setting so tests and development builds reuse the same dependency artifacts. The Notes HTML sanitizer dependencies are compiled with full optimization in both, because their unoptimized parsing makes import tests take several times longer. One-job builds prevent competing compiler or linker peaks, but they cannot bound a single crate: a cold rebuild of the Tauri app crate's test target peaks at about 4.4 GB in one compiler process, and an incremental rebuild at about 3 GB. Keep Tauri-free logic in workspace crates so that peak does not keep growing. Restore full native debug information only for a concrete debugging session:

```sh
CARGO_PROFILE_DEV_DEBUG=full CARGO_BUILD_JOBS=1 pnpm --dir apps/client tauri dev
```

The repository-owned Tauri wrapper sets one Cargo build job for development commands when the caller has not supplied one. On Wayland it removes an inherited exact `GDK_BACKEND=x11` override unless `GANBARU_DEV_PRESERVE_GDK_BACKEND=1` is set. Android commands select JDK 21, with `GANBARU_ANDROID_JAVA_HOME` as the explicit override. The Android Gradle daemon stops after ten idle minutes so its heap does not stay resident during later gates.

Persistent swap is a system safety margin, not a replacement for bounded jobs. Do not clear Cargo or Turbo caches as a routine memory fix. On Linux desktops where `systemd-oomd` kills by memory pressure, it kills a whole cgroup, usually the terminal that started the gate. Running a broad gate in its own scope, for example `systemd-run --user --scope pnpm -w run validate`, limits such a kill to the gate.

Cargo never removes stale artifacts. Version bumps, lockfile and toolchain updates, per-package feature sets, and each Android ABI leave separate copies, so `target/` grows by tens of gigabytes per month of active work. Run `pnpm -w run clean:rust` when it grows large (with no dev run or gate active); the next build is a cold rebuild.

## Cache behavior

Cargo reuses compatible compilation artifacts, but tests still execute. Turbo hashes declared inputs, configuration, environment inputs, and command arguments. Each Vitest shard has a distinct cache key. Bundle contracts cache their declared build output.

A warm run may restore frontend tasks while a cold or changed run performs substantially more work. A fast cached result is not evidence that a cold build has the same resource profile.

When changing validation topology, measure at least one affected cache-miss run and one warm repeat. Confirm both coverage and invalidation.

## Production bundle contracts

Unit tests cannot prove the final production import graph. The bundle contract performs real Vite builds and inspects emitted module metadata.

Desktop contracts inspect the transitive static imports of the entry, vault setup, and setup-time onboarding prewarm roots. These paths must keep the full App surfaces, terminal packages, Markdown rendering and sanitization, editor packages, and review runtime and helper dependencies out of their closures. Chat may load Markdown for messages, but terminal and review dependencies remain behind their existing dynamic imports. Checks identify emitted source modules and dependency package paths, so renaming or regrouping chunks cannot bypass these boundaries. Common App surfaces remain resident after vault activation.

Desktop and Android builds use different platform entries. The Android wrapper sets the platform before Vite configuration loads and writes to the isolated `.bundle-contracts/android/` directory. Contracts verify required roots and platform adapters, follow static imports transitively, enforce source-module ceilings, and reject desktop-only authority from the mobile artifact.

Both contracts reject emitted chunks that statically import each other in a cycle. Production chunks initialize their modules lazily, so a chunk cycle can call an initializer before it is assigned and fail only in release builds. A module imported both statically and dynamically is the usual cause; route the dependency through a registration module instead.

The Android artifact intentionally includes mobile anti-distraction, notification, document, and media adapters. It rejects desktop anti-distraction process control, the desktop App shell, PTYs, Git and provider execution, Rodio, desktop media controls, tray and title bar code, benchmark surfaces, desktop working-folder tools, and heavy editor graphs that are not part of the mobile route.

The machine-readable ceilings and required or forbidden module sets are authoritative in:

- `apps/client/scripts/bundle-contracts/baselines/first-use.json`
- `apps/client/scripts/bundle-contracts/baselines/android.json`

Both checks fail before inspecting the build when a baseline names an application source path that no longer exists, so renaming or deleting a module cannot silently disable a rule. Dependency package entries are not checked on disk because forbidden packages may be intentionally absent.

Ceilings are set to the measured import graph without extra headroom, so an unexpected eager import fails the contract. Raising a ceiling requires a concrete user-visible rationale. Record the reason in the change, not as a chronology in this document. Uncommon panels and heavy layouts, such as Notes database layouts, link editing, and deletion confirmation, stay behind lazy boundaries so initial routes remain small.

## Android project and pull request build

The generated-project contract verifies pinned Gradle, Android Gradle Plugin, Kotlin, SDK, NDK, Java, application identifiers, manifests, backup exclusions, provider paths, and Rust task behavior.

The independent pull request job builds one ARM64 debug APK on the pinned Ubuntu and Android toolchain with Rust warnings denied. It intentionally targets the reference-device ABI to bound pull request cost. The protected release workflow builds every supported ABI into signed universal APK and AAB artifacts.

Run the direct Android package contract with:

```sh
pnpm --dir apps/client run check:android-bundle
```

The root bundle contract builds desktop first, then Android through Turbo.

## Provider protocol snapshots

Routine checks validate the committed Codex app-server schema and OpenCode OpenAPI contract without requiring globally installed tools. A recorded CLI version is provenance for a snapshot, not a developer installation requirement.

Provider maintenance can compare an installed Codex app-server schema with the committed snapshot:

```sh
pnpm --dir apps/client run check:codex-protocol-installed
```

The scripts under `apps/client/scripts/provider-protocols/` check by default and regenerate only with `--write`; the OpenCode script also needs `--source <path-or-url>` to compare or regenerate. Use `generate:codex-protocol` or `generate:opencode-protocol` only after a reviewed protocol change.

## Changing validation topology

Before changing concurrency, sharding, ordering, heap limits, cache inputs, profiles, or target selection:

1. Record the current command topology and task counts.
2. Run the proposed command with representative changed inputs.
3. Confirm that no test files, Cargo targets, bundle roots, or platform checks disappear.
4. Compare wall time, peak memory, and swap use.
5. Repeat with warm caches.
6. Preserve portable standard-toolchain behavior.
7. Update root scripts, task configuration, `AGENTS.md`, and documentation together.

Prefer orchestration and bounded execution changes before removing coverage.

## Troubleshooting

Reproduce the first failed broad stage with the narrowest relevant command. Do not start another broad command while the original gate is active.

If a focused Vitest command collects the full suite, stop and correct its path or argument placement. If a Turbo result appears stale, inspect declared inputs and the task hash before disabling caching. If Cargo work becomes unexpectedly large, confirm the intended package and library or binary target and keep frontend tools stopped during diagnosis.
