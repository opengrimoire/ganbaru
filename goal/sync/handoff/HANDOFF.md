# Sync goal handoff

This folder lets a new orchestrating agent, on another machine with no earlier context, continue the concurrent sync goal exactly as it was being run. Read this file first, then [the goal README](../README.md) (decisions, tracks, reservations, working agreement), then the milestone plans you are working on. `goal/sync/` is tracked on `feat/sync` from this handoff on; the rest of `goal/` stays ignored.

## The goal (the user's request, verbatim)

> Implement goal/sync/ milestones 2 to 12 on the current branch, feat/sync, following goal/sync/README.md (tracks, reservations, working agreement) and AGENTS.md. Resume from the plans' Progress sections.
> - Use the advisor freely: before settling each step's approach, when stuck or changing approach, and before closing each milestone.
> - Use one read-only Explore agent for each milestone's "Current state" check.
> - Run parallel steps through the Agent tool with isolation: "worktree".
> - Use workflows only for read-only scouting or review, under 10 agents.
> - Don't push or open PRs.
> - You have full autonomy in making decisions after assessing them properly as a Rust senior developer would.
> Done when every step except physical acceptance is complete in Progress, validate passes on feat/sync, and you list the pending physical acceptance checklists for me.

The goal prompt of the new session sets the push and commit signing policies and overrides the "Don't push" line above. Relay the signing policy into every sub-agent prompt, since `implementer-brief.md` defers to it. Never open PRs or push to `dev` or `main`.

Paths: `<REPO>` in these files is the repository root and `<SCRATCHPAD>` is your session scratch directory. Worktree paths, agent ids, and old heads named in `briefs/` belong to the previous machine; when you reuse a brief, name the new worktree and head.

Out of scope: the multi-person sharing goal. Its plans (`goal/sharing/`) are not in the repository, so links to them from these plans do not resolve, and the README item about writing its first plan is not part of this goal.

## User decisions already taken

- Timing budgets are non-blocking. Record numbers from ignored budget tests; do not optimize. Batched sealing (engine follow-up 4 part B) is deferred past the goal.
- Physical acceptance is pre-approved as deferred; collect its checklists for the final report.
- Workflow changes for milestone 12 step 10 are approved.
- Milestone 11 step 9's external security review is replaced by a fresh internal Opus security review; note that in its progress entry.
- Sub-agents may edit when their tasks do not conflict.
- Always pass `model` and `effort: medium` to sub-agents: Opus for data-safety work and every review, Sonnet for exploration and ordinary edits, Haiku for mechanical tasks.
- Anything in the Cargo target directories may be deleted.
- Commit every finished phase or fix without asking (conventional, one line, scoped, no AI attribution). Never amend, rebase, or rewrite history: other machines pull fast-forward only.
- Approvals inside the goal are delegated to the orchestrator; ask the user only for destructive, credential, security-protection, or dependency decisions as AGENTS.md and the user's global rules require.

## Binding rules (short list; AGENTS.md is authoritative)

- English, no emojis, no em dashes or double hyphens in prose, comments, docs, or commits; sentence case.
- No Python file edits, no TypeScript `any`, no hardcoded values, pnpm only.
- Never run Cargo concurrently with Node, Vitest, or another Cargo gate. No dev servers, no Tauri launch, no UI checks.
- Never link `goal/` from product code or docs.
- Engine-track files (`crates/ganbaru-sync/src/{apply,merge,materialize,seal,triggers,repair,tree,visibility}.rs`) are edited only by engine-track steps, and engine steps run serially.
- At most two implementing agents at once; read-only reviewers do not count.
- Vault manifest and handoff protocol versions take the next free number at merge. Migrations are `YYYYMMDDHHMMSS_description.sql` in UTC and must sort last at merge; never rewrite a landed migration (an unlanded one may be renamed).
- Per milestone close: `pnpm -w run validate`, a fresh read-only Opus review, then an advisor close.

## State on feat/sync

Updated by the orchestrator session on the mini PC (2026-10-28). Milestone 11 step 7 (merge c35c5dc0), milestone 8 step 5 (merge 26180e64), milestone 9 step 5 (merge b1e35a51), milestone 8 step 6 (merge a0301701), milestone 9 step 6 (merge 5ba787a9), milestone 8 step 7 (merge 49e307c1), milestone 8 step 8 (merge 41d092c7, closing milestone 8), and milestone 9 step 7 (merge 67cbfc05) have landed. Landed topic branches were deleted on origin.

- `VAULT_MANIFEST_VERSION` is 21, handoff `PROTOCOL_VERSION` is 14, the last migration is `20261028081009_chat_assignments_replication.sql`.
- Done in source: milestones 2 to 7, milestone 8 steps 1 to 8 (closed), milestone 9 steps 1 to 7, milestone 10 steps 1 to 4, milestone 11 steps 1 to 8, engine follow-ups 1 to 5.
- Remaining: milestone 9 steps 8 to 10 (docs part of 10), milestone 10 steps 5 to 7, milestone 11 steps 9 and 10, milestone 12 steps 1 to 11, milestone closes, a clean `pnpm -w run validate`, and the final report.

## In-flight branches

Progress snapshots are in `progress/`; their live copies were in the session scratch directory, so check each branch's log for later commits.

1. `feat/sync-peer-exchange`: milestone 11 step 9. Parts A to C are committed. The four-part internal security review (`briefs/m11-security-review-brief.md`, findings in `briefs/m11-security-findings.md`) returned fix first. The first fix round (`briefs/m11-step-9-security-fix-brief.md`) landed on the branch; its delta security review (`briefs/m11-step-9-delta-security-review-brief.md`) returned fix first, and fix round 2 (`briefs/m11-step-9-security-fix2-message.md`) is running. A focused delta security review follows, then landing with protocol 15 or the next free number.
2. `feat/sync-chat-config`: milestone 9 step 8 (brief `briefs/m9-step-8-brief.md`), implementer finishing its checks; it merges feat/sync (step 7 landed) before review and landing.
3. `perf/test-migration-speed`: test suite speed (brief `briefs/test-speed-brief.md`), implementer running. Not a plan step; it lands like one after a review.

WIP patches: none. Briefs copied from the session scratch directory are in `briefs/`; copy them to your scratch directory and replace its paths. The ready-to-paste resume prompt is `NEXT_PROMPT.md`.

Engine lows for the next engine step: `briefs/engine-next-followups.md`.


Cloud host notes: `implementer-brief-cloud.md` is the brief used there (per-worktree Cargo target dirs with `CARGO_INCREMENTAL=0`, never renamed or shared; disk was the binding limit). Recommended scheduling: keep the engine track running continuously (it is the longest chain) and alternate Chat and Notes steps in the second implementer slot.

## Next steps in order

1. Finish, review, and land the in-flight branches above (at most three implementers at once).
2. Milestone 9 steps 5 to 9 and the docs part of 10, one at a time on the `chat` track.
3. Milestone 8 steps 7 and 8 on the `domain` track.
4. On the `engine` track, serially: milestone 11 steps 9 and 10 (step 9 uses the internal Opus security review, a user decision), then milestone 10 steps 5 to 7.
5. Milestone 12 steps 1 to 11.
6. Close each milestone (validate, review, advisor), then a final clean `pnpm -w run validate` on feat/sync.
7. Final report: the pending physical acceptance checklists, deduplicated from each milestone's Manual acceptance section and `final-report-notes.md`, plus the pre-release items recorded there (Calendar duplicate repair references, the untested `drain_and_flush`, step 9 notes on the discovery key and trust in final rejections). If time allows, the Calendar duplicate repair follow-up.

## How the work was run

- Before each step: write a brief (pattern in `briefs/`), ask the advisor about the approach, then launch one implementer: `general-purpose`, Opus, effort medium, `isolation: "worktree"`, in the background. The prompt says: read and follow the brief exactly; feat/sync is now `<sha>`; work in your worktree only and never `cd` into the main checkout; do not start shell watchers that pgrep their own command line; merge commits are `merge: feat/sync into <branch>`; source the Android env script from inside your worktree; never end your turn while your own checks are still running.
- Every implementer follows `implementer-brief.md` (build lock, per-track target dirs, signed commits, progress file format, Android compile check). Replace `<SCRATCHPAD>` in the copied briefs with your own scratch directory; progress files live there until landing.
- After each step: a fresh read-only Opus review with a brief like `briefs/m8-step-3-review-brief.md` (verdict merge, merge with follow-ups, or fix first, findings by severity with file:line and a failure scenario). On "fix first", send the implementer a fix message (`briefs/*-fix-message.md` pattern), then a delta review. Follow-ups that are safe to defer go into the next step's brief.
- Landing: `git merge-base --is-ancestor feat/sync <branch>`, `git merge --no-ff --no-commit <branch>`, check the diff, versions, and migration order, `git commit -qS -m "merge: <title> (sync milestone M step K)"`, confirm `git log -1 --format=%G?` is `G`, run `tick.sh M K <progress file>` (it replaces the step's unticked line in the plan with the progress entry), remove the worktree and branches, prune, delete the Android target dir of that worktree, update the README status line, and commit the goal changes (`docs(goal): record sync milestone M step K`). `land.sh` does most of this.
- Decisions the plan does not cover are taken by the orchestrator, recorded in the step's progress entry, and listed in `final-report-notes.md`.
- Cloud session variant (2026-10-25): commits and merges were unsigned (user-approved), so landing used `git merge --no-ff --no-commit` and a plain `git commit` instead of `land.sh` (which signs and checks `G`); worktrees were created by hand under one folder (one per track, reused across steps by switching to the next branch from `feat/sync`) instead of `isolation: "worktree"`; no advisor tool existed, so reviews replaced advisor calls. Push `feat/sync` and every unlanded topic branch after each landing; the proxy there refused deleting remote branches.

## Machine setup (the old host, adapt to yours)

- Every Cargo, Vitest, or pnpm check ran under one lock so two agents never built at once: `exec 9>$HOME/.cache/ganbaru-checks.lock; flock 9; <command>`, in the background with a long timeout. That host had 7.6 GiB of memory; set `GANBARU_VALIDATE_PROFILE=low-memory` if yours is small.
- Target dirs per track under `$HOME/.cache/ganbaru-targets/`: `engine`, `domain`, `chat`, `review`, and `android-<checkout folder>`.
- Android: `cargo check --target aarch64-linux-android` only, no APK builds. The env script (`~/Android/android-check-env.sh` on the old host) set `JAVA_HOME` (OpenJDK 21), `ANDROID_HOME`, `NDK_HOME` (NDK 30.0.15729638), the `aarch64-linux-android29-clang` compiler, linker, and `llvm-ar` variables, and a per-checkout `CARGO_TARGET_DIR`. The check command and its baseline warnings are in `implementer-brief.md`. Without the toolchain, record the Android check as not run.
- On the old host commits were SSH-signed with the user's key. Elsewhere, follow the signing policy in the goal prompt; without one, an unsigned commit means stop and report.

## Cloud setup (Claude Code on the web)

- pnpm: if `pnpm` fails with "Cannot find module .../pnpm.cjs" (bundled corepack cannot start pnpm 12), put a wrapper earlier on `PATH`: `~/.local/bin/pnpm` containing `exec sh ~/.cache/node/corepack/v1/pnpm/12.10.1/pnpm "$@"`, then `pnpm install --frozen-lockfile` in each checkout.
- Git: if the container's signing key is unusable, `git config commit.gpgsign false` (only under the goal prompt's unsigned approval) and set `user.name VictorBenitoGR` and `user.email victorbenitogr@gmail.com` to match history.
- Disk was the binding limit (about 40 GB allowance; one non-incremental target dir with tests is about 10 GB). Keep at most two or three target dirs, always `CARGO_INCREMENTAL=0`, never share one dir between checkouts and never rename one (build-script outputs embed absolute paths). To hand a dir to another worktree, touch every tracked file of that worktree (`git ls-files -z | xargs -0 touch`). Old workspace artifacts (`ganbaru*` in `debug/deps` older than the last touch) may be deleted. Reviewers read without building.
- Before the final `pnpm -w run validate`, delete the worktree target dirs so one clean build fits.
- `implementer-brief-cloud.md` is the implementer brief for such a host.

## Files here

- `implementer-brief.md`: the rules every implementing agent follows; `implementer-brief-cloud.md` is its variant for a cloud host (paths, disk, unsigned commits, no Android toolchain).
- `briefs/`: the briefs, review briefs, and fix messages of the in-flight steps, usable as templates.
- `progress/`: progress entries of the unlanded steps.
- `wip/`: uncommitted work of agents that stopped mid-change (empty: the earlier patches were applied and landed).
- `final-report-notes.md`: running notes for the final report (landings, decisions, deferred items, physical checks).
- `user-notes.md`: notes for the user's manual checks.
- `tick.sh`, `land.sh`: bookkeeping and landing scripts (`GANBARU_REPO` and `SCRATCHPAD` set the paths).
