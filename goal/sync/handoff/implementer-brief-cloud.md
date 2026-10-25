# Brief for implementing agents (concurrent sync goal, cloud host)

You implement one plan step (or fix round) of the concurrent sync goal in the Ganbaru repository. You are a senior Rust and TypeScript engineer with full autonomy over implementation decisions inside the plan.

## Path mapping

Older briefs name paths on a previous host. Map them: `/home/victor/ganbaru` is `/home/user/ganbaru` (main checkout, the orchestrator's; never edit or build there), `/home/victor/.cache` is `/root/.cache`, `/home/victor/.claude/CLAUDE.md` is `/root/.claude/CLAUDE.md`, and `<SCRATCHPAD>` is `/tmp/claude-0/-home-user-ganbaru/801a952b-bd25-5890-82d7-9ac7c14fac3d/scratchpad`.

## Read first

- `/home/user/ganbaru/AGENTS.md` and `/root/.claude/CLAUDE.md`: binding rules (no `any`, no em dashes or double hyphens in prose/comments, sentence case, rustdoc/JSDoc, no Python file edits, no hardcoded values, docs update rules, localization, testing rules).
- `/home/user/ganbaru/goal/sync/README.md` (decisions table, reservations, working agreement) and your milestone plan `/home/user/ganbaru/goal/sync/milestone_N.md`. Read `goal/` only from the main checkout path; never edit `goal/` anywhere. Never link to `goal/` from product code or docs.

## Worktree and branch

- You work only in the worktree directory your task names (for example `/home/user/wt/engine`), on the branch your task names. Never `cd` into `/home/user/ganbaru` except to read `goal/` files. Confirm `git branch --show-current` before committing.
- Set `CARGO_TARGET_DIR=/root/.cache/ganbaru-targets/<worktree folder name>` in every cargo command (for example `/root/.cache/ganbaru-targets/engine` for `/home/user/wt/engine`). Never share a target dir between checkouts: Cargo fingerprints are workspace-relative, so a shared dir can reuse stale artifacts.
- Never push, never open PRs, never amend or rebase, never merge into `feat/sync` yourself (the orchestrator lands). Merging feat/sync into your branch is allowed when your task says so: `git merge --no-ff feat/sync -m "merge: feat/sync into <branch>"`.

## Resource rule (mandatory, 4 cores and 15 GiB shared with another agent)

- Hold the shared lock for EVERY cargo, vitest, pnpm check, or other build or test command: start the command line with `exec 9>/root/.cache/ganbaru-checks.lock; flock 9; `, for example `exec 9>/root/.cache/ganbaru-checks.lock; flock 9; CARGO_TARGET_DIR=/root/.cache/ganbaru-targets/shared cargo test -p ganbaru-sync --lib -j 4 conformance -- --test-threads=4`. Never run two of them at once.
- Commands may wait on the lock. Use the Bash `timeout` of 600000 ms, or `run_in_background` for longer runs and wait for the completion notification. Never end your turn while your own checks are still running. Do not start shell watchers that pgrep their own command line.
- Do not start rust-analyzer, dev servers, Tauri, or the app.
- Do not run the full `pnpm -w run validate` unless your task says so.
- Disk is tight (about 17 GB free for all builds). Add `CARGO_INCREMENTAL=0` to every cargo command and never create extra target directories.

## Work

- Implement exactly your task, following the plan's Design. Where the plan is silent, decide as a senior engineer and record the decision in your progress entry. If something truly cannot be done as planned, implement the closest correct alternative and record why.
- Engine files (`crates/ganbaru-sync/src/{apply,merge,materialize,seal,triggers,repair,tree,visibility}.rs`) are edited only when your task says you are on the engine track.
- Migrations: `crates/ganbaru-db/migrations/YYYYMMDDHHMMSS_description.sql` with the current UTC time (`date -u +%Y%m%d%H%M%S`); it must sort after every migration on feat/sync.
- Tests next to the code, matching existing depth and style; cover the plan's Testing items for your step.
- Update the docs your change affects, in the same commits, in short current-state voice.
- Run your task's focused checks (plus `cargo clippy` with `-- -D warnings` on what you touched and `cargo fmt --check`). All must pass before committing.

## Commits

- Commits are unsigned on this host (approved by the user for the sync goal branches); `commit.gpgsign` is already false. Conventional, one line, English, scoped, for example `feat(sync): add domain registry and local class`. No AI attribution and no Co-Authored-By lines. Never amend.

## Android

The Android toolchain is not installed here. Skip the Android compile check and record "Android compile check: not run (no toolchain on this host)" in your progress entry. Still keep desktop-only code under `#[cfg(desktop)]` and mobile code compiling by inspection.

## Progress entry

Write your progress entry to `<SCRATCHPAD>/milestone-N-step-K-progress.md` (if a progress file already exists in `/home/user/ganbaru/goal/sync/handoff/progress/`, copy it there first and extend it). First line `- [x] Step K: <the step's Progress title>`, then indented `  - ` lines: date, branch, commit hashes, checks with results, which branch of any relevant Implementation check held, plan-silent decisions, notes for later steps. Keep it short. Update it after each commit.

## Final report (your last message, under 300 words)

Branch name, commit hashes and messages, checks run with pass or fail, deviations from the plan and why, anything the next step or the orchestrator must know. No diff dumps.

Target dirs are a pool reassigned between worktrees by the orchestrator, who touches every tracked file of the receiving worktree first so Cargo rebuilds its workspace crates. Use only the target dir your task names.
