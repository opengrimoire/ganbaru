# Dependency audit posture

This file records reviewed Rust and npm advisory exceptions and allowed warnings. Lockfiles and current command output are authoritative; an old date or prior clean result is not evidence for the current lockfile.

As of 2026-11-18, `pnpm -w run audit:deps` reports no known npm vulnerabilities beyond the two reviewed ignores below, and `pnpm -w run audit:rust` exits successfully with the one reviewed ignore and the six allowed warnings listed below.

A successful Rust audit exit does not mean warning-free. The cargo-audit output policy allows warnings, so maintainers must review new unsound, unmaintained, and yanked findings separately.

## Commands and gates

- `pnpm -w run audit:deps` checks workspace npm dependencies.
- `pnpm -w run audit:rust` checks `Cargo.lock` using `.cargo/audit.toml`.
- `pnpm -w run audit` runs both.
- `pnpm -w run validate:full` runs the audits followed by the normal code gate.

Run `validate:full` for dependency, lockfile, release, audit, and other security-sensitive work. Never add or broaden an ignore merely to make the command green.

## Configured vulnerability ignores

`.cargo/audit.toml` ignores only the Rust advisories below. An ignore is added only after path review recorded here, and is removed when its assumptions stop holding.

### RUSTSEC-2026-0459, SQLite uninitialized page-cache read

SQLite before 3.54.0 (CVE-2026-119375) can return uninitialized page-cache memory through ordinary queries when it opens a database beside a `-wal` file whose page size differs. The fix needs `libsqlite3-sys` 0.39, and sqlx-sqlite 0.9.0, the latest release, requires `libsqlite3-sys` below 0.38. Ganbaru only opens a foreign `-wal` when the user opens a vault folder someone else prepared, and the leaked bytes then land only in that vault's own query results. Bundles never carry one: archives and backups are written with `VACUUM INTO`, extraction admits only the vault marker, `ganbaru.sqlite`, `assets/`, and `projects/`, and replacing a vault removes the target's database sidecars. The relay never opens peer databases. Remove the ignore when a sqlx release accepts `libsqlite3-sys` 0.39 or later, and upgrade then.

## Configured npm ignores

These advisories are ignored through `auditConfig.ignoreGhsas` in `pnpm-workspace.yaml` after path review.

### GHSA-gcqx-w48m-w2h4, deepmerge prototype poisoning

deepmerge 4.3.1 lets a merged source object poison the prototype of the returned object, and no fixed release exists. It is reached only as a development dependency through `@sveltejs/vite-plugin-svelte`, which uses it once to merge plugin options from the repository's own Svelte and Vite configuration at build time; no user, vault, or network data reaches it, and it is not shipped in the app. Remove the ignore when a fixed deepmerge ships or the plugin drops it.

### GHSA-fc5q-39xc-vjpf, picomatch extglob ReDoS

picomatch 4.0.7 and earlier backtrack exponentially on single-branch repeated extglobs such as `+(*)`, so a crafted glob pattern can block the event loop, and no fixed release exists. It is reached only as a development dependency through Vite, Vitest, Tailwind, and their file globbing and watching (`pnpm why picomatch --prod` finds no path). Those tools match only the repository's own configuration globs at build and test time; no user, vault, or network data supplies a pattern, and it is not shipped in the app. Remove the ignore when a fixed picomatch ships.

## Allowed upstream warnings

### chrono

RUSTSEC-2026-0543 reports chrono as unmaintained, with jiff and time as suggested successors; it is not a vulnerability. Ganbaru uses chrono for timestamp parsing and formatting across most crates, and rmcp and schemars also depend on it, so replacing it in first-party code alone would not remove it. Revisit when those dependencies offer a chrono-free build or a chrono vulnerability is reported, and migrate first-party code to jiff then.

### GTK3 and GLib

- RUSTSEC-2024-0429: unsound iteration in glib 0.18.
- RUSTSEC-2024-0370: unmaintained proc-macro-error.

Both come through the Linux Tauri and Wry WebKit stack and native Linux integrations. Ganbaru does not use the affected glib `VariantStrIter` API. Remove them when the upstream stack can move to maintained bindings without replacing them with a broader or less trustworthy runtime.

### D-Bus split crates

RUSTSEC-2026-0370, RUSTSEC-2026-0371, and RUSTSEC-2026-0372 report zvariant, zbus_names, and zvariant_derive as unmaintained because zbus 6 absorbed them. They are reached through zbus 5 in notify-rust (Linux notifications), secret-service (the Linux keyring store), and the Tauri opener and single-instance plugins. notify-rust 4.19 already uses zbus 6, but moving it alone would ship both zbus stacks. Remove them when the others move to zbus 6.

## Bundled SQLite

SQLite CVEs are not RustSec advisories, so neither audit reports them; track the bundled version by hand. sqlx builds SQLite from the source vendored in `libsqlite3-sys`; confirm the version from that crate's `sqlite3/sqlite3.h`, not from its release notes.

- Current: SQLite 3.51.3 through `libsqlite3-sys` 0.37.0 and sqlx 0.9.0. It fixes CVE-2025-7709 (FTS5 corrupt index, reachable from database content), CVE-2025-6965, CVE-2025-29087, and CVE-2025-3277. `ganbaru-db` tests assert this minimum.
- Open: CVE-2026-11822 and CVE-2026-11824 are fixed in SQLite 3.53.2, which needs `libsqlite3-sys` 0.38, and sqlx 0.9.0 does not allow it. Both need arbitrary SQL with defensive mode off, so the defensive build below mitigates them. CVE-2026-119375 is the reviewed ignore above. Upgrade when a sqlx release accepts `libsqlite3-sys` 0.39.
- Build: `.cargo/config.toml` compiles SQLite with `SQLITE_DEFAULT_DEFENSIVE` and `SQLITE_TRUSTED_SCHEMA=0` through `LIBSQLITE3_FLAGS`. `ganbaru-db` pools also set `trusted_schema=OFF` and `cell_size_check=ON`, and validate untrusted schema catalogs as described in [Application boundaries](application-boundaries.md#vault-and-configuration).

## Updater archive extraction

`tauri-plugin-updater` builds without its default `zip` feature. Released updater assets are a raw AppImage and Windows installers, so the plugin never needs to extract a zip or AppImage tarball, and the feature would only add an archive extraction path. Re-enable it only if the release feed starts shipping archived updater assets, and then only with a zip release free of extraction advisories.

## Generated catalog sources

Codegen scripts that download upstream data name an exact version and check the bytes against the SHA-256 recorded in `apps/client/scripts/codegen/pinned-sources.json`; the vscode-icons generator pins a Git commit instead. A changed digest for a pinned URL is refused. A new upstream version is recorded only when the maintainer passes `--pin-new-source`, and its generated diff is reviewed before commit.

## Review procedure

For a new finding:

1. Record the advisory, affected version, and finding class.
2. Use `cargo tree` with the exact package version to identify every reverse path.
3. Determine whether the vulnerable API is compiled, shipped, and reachable from untrusted input.
4. Prefer a compatible upstream update and run the full dependency gate.
5. If no safe update exists, document the unreachable or mitigated path and the condition that ends the exception.
6. Add a configured ignore only for a vulnerability advisory with a reviewed rationale. Never ignore broad warning categories.
7. Revisit every exception during dependency work and before releases.

Keep this page limited to active exceptions and material warnings, removing entries once the audit no longer reports them.
