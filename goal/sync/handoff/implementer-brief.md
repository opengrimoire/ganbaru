# Brief for implementing agents (concurrent sync goal)

You implement one plan step of the concurrent sync goal in the Ganbaru repository. You are a senior Rust and TypeScript engineer with full autonomy over implementation decisions inside the plan.

## Read first

- `<REPO>/AGENTS.md` and `~/.claude/CLAUDE.md`: binding rules (no `any`, no em dashes or double hyphens in prose/comments, sentence case, rustdoc/JSDoc, no Python file edits, no hardcoded values, docs update rules, localization, testing rules).
- `<REPO>/goal/sync/README.md` (decisions table, reservations, working agreement) and your milestone plan `<REPO>/goal/sync/milestone_N.md` (Current state, its "Re-verification" and "Decisions taken at start" subsections, Design, Testing, Work breakdown, Implementation checks). `goal/sync/` is tracked on feat/sync, but the orchestrator keeps the current copy in the main checkout: read these absolute paths and never edit `goal/` in your worktree. Never link to `goal/` from product code or docs.

## Worktree and branch

- If you run in a worktree, it may start on the wrong base. First run `git switch -c <worktree branch from the plan> feat/sync` (the local `feat/sync` branch is visible in every worktree) and confirm `git log -1 feat/sync` matches your HEAD. If you run directly in `<REPO>` on `feat/sync`, commit there.
- Set `CARGO_TARGET_DIR=$HOME/.cache/ganbaru-targets/<track>` in every shell command that runs cargo, where `<track>` is given in your task (for example `engine` or `network`). Each track keeps its own persistent target directory across steps; never point two worktrees of different tracks at the same one (stale artifacts). The first build in a fresh track directory takes a while.
- Frontend checks in a worktree need `pnpm install --frozen-lockfile --offline` once.
- Never push, never open PRs, never amend or rebase, never merge into `feat/sync` yourself (the orchestrator merges).

## Resource rule (mandatory; several agents share one machine)

- Hold the shared lock for EVERY cargo, vitest, pnpm check, or other build/test command: start the command line with `exec 9>$HOME/.cache/ganbaru-checks.lock; flock 9; ` (the plain `flock <file> cmd` form is refused by the harness), for example `exec 9>$HOME/.cache/ganbaru-checks.lock; flock 9; CARGO_TARGET_DIR=$HOME/.cache/ganbaru-targets/engine cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4`. Never run two of them at once.
- Commands may wait on the lock; use the Bash `timeout` of 600000 ms, or `run_in_background` for longer runs and wait for the completion notification.
- Do not start rust-analyzer, dev servers, Tauri, or the app.
- Do not run the full `pnpm -w run validate` unless your step is the milestone's validation step; the orchestrator runs it.

## Work

- Implement exactly your step's contents from the Work breakdown table, following Design. Where the plan is silent, decide as a senior engineer and record the decision in the plan (see below). If a step truly cannot be done as planned, implement the closest correct alternative and record why.
- Do not edit shared engine files (`apply.rs`, `merge.rs`, `materialize.rs`, `seal.rs`, `triggers.rs` in `crates/ganbaru-sync/src`) unless your step is on the engine track (milestones 2, 10, 11, 12).
- Migrations: `crates/ganbaru-db/migrations/YYYYMMDDHHMMSS_description.sql` with the current UTC time (`date -u +%Y%m%d%H%M%S`).
- Tests next to the code, matching existing depth and style; cover edge cases named in the plan's Testing section for your step.
- Update the docs your step's contract changes (the plan's Documentation section says which), in the same commits. Keep doc edits short and in current-state voice.
- Run the step's focused checks from the Work breakdown (plus `cargo clippy` on the crates you touched with `-- -D warnings` and `cargo fmt --check`). All must pass before committing.

## Commits

- Follow the commit signing policy stated in your task prompt; without one, commits must be signed (automatic via git config). Conventional, one line, English, scoped, for example `feat(sync): add domain registry and local class`. No AI attribution. One or more commits per step. If signing fails and your task prompt does not allow unsigned commits, stop and report.
- Check: `git cat-file commit HEAD | grep -q gpgsig && echo signed`.

## Plan bookkeeping (after checks pass and you have committed)

- You cannot write outside your worktree. Write your progress entry to `<SCRATCHPAD>/milestone-N-step-K-progress.md` (if that write is also refused, put the entry at the end of your final report). Format: first line `- [x] Step K: <the step's Progress title>`, then indented `  - ` lines: date, branch, commit hashes, which branch of any relevant Implementation check held, and decisions taken where the plan was silent. Keep it to a few lines. The orchestrator pastes it into the plan.
- If you run directly in `<REPO>` (not a worktree), edit the plan's Progress line yourself instead.
## Final report (your last message, under 250 words)

Branch name, commit hashes and messages, checks run with pass/fail, deviations from the plan and why, anything the next step or the orchestrator must know. No diff dumps.

Android compile check: this host has the Android toolchain for `cargo check` only (no APK builds, no device). After touching code that compiles for mobile (`#[cfg(mobile)]`, `android.rs`, mobile plugins, or shared app and sync code), run under the build lock and never next to another gate: `bash -c 'source ~/Android/android-check-env.sh && cargo check --target aarch64-linux-android -p ganbaru-tauri-app -p ganbaru -p ganbaru-mobile-distractions -p ganbaru-mobile-documents -p ganbaru-mobile-media -p ganbaru-mobile-notifications -p ganbaru-mobile-sync --lib -j 4'`. It must finish with no errors and no warnings beyond the baseline (keys_after, ChatDefaultsScope::WorkingFolder, received_request_by_peer, reported_roots, active_database_path). The env script picks a target dir per checkout (android-<worktree folder>); if you export variables inline, set CARGO_TARGET_DIR=~/.cache/ganbaru-targets/android-<your worktree folder name>, never a shared one. ganbaru-chat native-runtime modules do not compile on Android; gate app code using them with `#[cfg(desktop)]`.

If the Android toolchain is not installed (for example in a cloud environment), skip the Android check and record it as not run in your progress entry.
