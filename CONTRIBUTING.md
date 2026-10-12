# Contributing

Ganbaru AI uses pull requests for review, CI history, and release notes. Keep changes focused and use the smallest branch that describes the work.

Participation in issues, pull requests, and other project spaces is governed by `.github/CODE_OF_CONDUCT.md`.

Branch and release restrictions are supply-chain controls, not a judgment about individual contributors. Signed desktop artifacts, updater metadata, release tags, workflow files, and CI state are privileged paths. The May 2026 TanStack npm compromise showed how a CI trust-boundary issue, including GitHub Actions cache poisoning through `pull_request_target`, can become a package release compromise. This repository keeps normal contribution review, integration, and release authority separate for that reason. See TanStack's postmortem: <https://tanstack.com/blog/npm-supply-chain-compromise-postmortem>.

## Rust toolchain

Install Rust through rustup. Repository commands use the exact compiler and the Clippy and rustfmt components in `rust-toolchain.toml`; CI and release jobs use that same file instead of following the floating stable channel.

Rust 1.98 is the supported minimum compiler version, with 1.98.0 pinned for reproducible validation. This is the tested project baseline, not a claim that earlier compilers cannot compile individual crates. All workspace packages inherit Rust edition 2024, the compiler requirement, authors, and license from `[workspace.package]`. Package versions stay local because the app, internal libraries, and mobile plugins have different version histories.

Cargo resolver 3 prefers dependencies compatible with the declared compiler requirement when resolving versions. Keep `Cargo.lock` committed and use locked builds in CI; the resolver is not a replacement for dependency review or security audits.

Toolchain upgrades are deliberate maintenance changes. Update the pin and supported compiler requirement together, run `pnpm -w run validate:full`, and verify Linux, Windows, and Android builds. Review platform-specific code as well as host compatibility diagnostics when changing editions. Rustfmt uses the 2024 style edition in `rustfmt.toml`, matching the workspace language edition. Run `cargo fmt --all` to format the Rust workspace with the pinned toolchain.

## Branch flow

- `main` is the release source branch. It should move through release pull requests from `dev` by merge queue.
- `dev` is the integration branch for accepted work between releases.
- Normal work happens on short-lived topic branches created from `dev`.
- Topic branches open pull requests into `dev`.
- Topic pull requests merge into `dev` with merge commits so their signed commits, authorship, timestamps, and Git topology remain part of the repository history.
- Release preparation changes, such as version bumps, happen through normal pull requests into `dev` before `dev` is promoted to `main`.
- Releases are created from `app-v*` tags on the release commit, not from every merge to `main`.

Do not push directly to `main` or `dev`. Changes enter through pull requests. Any emergency exception would first require a deliberate, reviewed ruleset change because neither protected branch has a bypass actor.

Only organization admins may merge release pull requests into `main`, create or update `app-v*` tags, approve the protected release environment, or publish GitHub Releases. Today that means the organization owner unless release authority is explicitly delegated. The exact intended GitHub rulesets are documented in [repository policy](docs/operations/repository-policy.md).

## Pull requests

Use concise PR titles that would read well in release notes. Conventional prefixes are preferred, for example:

- `feat(vault): add setup language selector`
- `fix(calendar): preserve imported attendee status`
- `docs(release): clarify branch flow`

Before opening a PR:

1. Make sure the branch is based on current `dev`.
2. Keep unrelated edits out of the branch.
3. Update the owning doc when the change alters intended behavior, a data or security invariant, a public contract, or an important decision. See the maintenance rules in [docs/README.md](docs/README.md).
4. Run the relevant local gate from `AGENTS.md`. For normal code and UI changes, use `pnpm -w run validate`.
5. Include a short PR summary and note any checks that were not run.

When a PR should not appear in generated release notes, add the `skip-changelog` or `ignore-for-release` label.

## Release pull requests

Release PRs come directly from `dev` into `main` and land through the `main` merge queue, so `dev` never needs to be updated with `main`. Version bumps happen beforehand through a normal pull request into `dev`. After the release commit lands, an organization admin tags it with `app-v*`, which builds a draft GitHub Release; publishing that draft updates the apt, RPM, and AUR packages. Pull requests that target `main` and do not come from `dev` should be retargeted to `dev` or closed.

The [release guide](docs/operations/release/README.md) owns the full procedure.
