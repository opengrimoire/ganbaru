# Dependency audit posture

This file records reviewed Rust advisory exceptions and allowed warnings. Lockfiles and current command output are authoritative; an old date or prior clean result is not evidence for the current lockfile.

As of 2026-10-04, `pnpm -w run audit:deps` reports no known npm vulnerabilities, and `pnpm -w run audit:rust` exits successfully with the three configured ignores and seven allowed warnings listed below.

A successful Rust audit exit does not mean warning-free. The cargo-audit output policy allows warnings, so maintainers must review new unsound, unmaintained, and yanked findings separately.

## Commands and gates

- `pnpm -w run audit:deps` checks workspace npm dependencies.
- `pnpm -w run audit:rust` checks `Cargo.lock` using `.cargo/audit.toml`.
- `pnpm -w run audit` runs both.
- `pnpm -w run validate:full` runs the audits followed by the normal code gate.

Run `validate:full` for dependency, lockfile, release, audit, and other security-sensitive work. Never add or broaden an ignore merely to make the command green.

## Configured vulnerability ignores

These advisories are ignored in `.cargo/audit.toml` after path review. Each ignore is conditional and must be removed when its assumptions stop holding.

### RUSTSEC-2023-0071, rsa timing side channel

`Cargo.lock` contains rsa through SQLx's optional MySQL macro support. Ganbaru AI uses SQLite only, and no application dependency tree activates sqlx-mysql or rsa in the shipped database path.

Remove the ignore before enabling MySQL, when SQLx no longer locks the affected optional path, or when a compatible fixed path is available.

### RUSTSEC-2026-0194 and RUSTSEC-2026-0195, quick-xml denial of service

Affected quick-xml versions are locked through constrained paths:

- `wayland-scanner` parses dependency-owned Wayland protocol XML at compile time.
- `tauri-winrt-notification` uses the XML escaping helper to build Windows notifications and does not invoke the affected reader, duplicate-attribute, or namespace parsing paths.

The vulnerable parsing is not reachable from user-controlled runtime XML. Remove the ignores when these dependencies accept fixed quick-xml releases, or immediately if a runtime parser reaches an affected version.

## Allowed upstream warnings

### GTK3 and GLib

- RUSTSEC-2024-0429: unsound iteration in glib 0.18.
- RUSTSEC-2024-0370: unmaintained proc-macro-error.

Both come through the Linux Tauri and Wry WebKit stack and native Linux integrations. Ganbaru AI does not use the affected glib `VariantStrIter` API. Remove them when the upstream stack can move to maintained bindings without replacing them with a broader or less trustworthy runtime.

### UNIC crates

RUSTSEC-2025-0075, RUSTSEC-2025-0080, RUSTSEC-2025-0081, RUSTSEC-2025-0098, and RUSTSEC-2025-0100 report unmaintained unic crates reached through Tauri's URL pattern dependency. Remove them when the upstream graph no longer requires those crates.

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
