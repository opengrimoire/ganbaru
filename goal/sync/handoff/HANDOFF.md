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

The user pushed `feat/sync` and the in-flight branches once, for this handoff only. Do not push again or open PRs unless the user asks.

## User decisions already taken

- Timing budgets are non-blocking. Record numbers from ignored budget tests; do not optimize. Batched sealing (engine follow-up 4 part B) is deferred past the goal.
- Physical acceptance is pre-approved as deferred; collect its checklists for the final report.
- Workflow changes for milestone 12 step 10 are approved.
- Milestone 11 step 9's external security review is replaced by a fresh internal Opus security review; note that in its progress entry.
- Sub-agents may edit when their tasks do not conflict.
- Always pass `model` and `effort: medium` to sub-agents: Opus for data-safety work and every review, Sonnet for exploration and ordinary edits, Haiku for mechanical tasks.
- Anything in the Cargo target directories may be deleted.
- Commit every finished phase or fix without asking (signed, conventional, one line, scoped, no AI attribution). Never amend, rebase, or rewrite history: other machines pull fast-forward only.
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

Head at handoff: the commit that added this folder, on top of `419c6128 merge: anchor service and engine follow-up 4 part C (sync milestone 11 step 6a)`.

- `VAULT_MANIFEST_VERSION` is 13, handoff `PROTOCOL_VERSION` is 13, the last migration is `20261025051154_sync_stored_order.sql`.
- Done in source: milestones 2 to 7 (only physical acceptance open), milestone 8 steps 1 and 2, milestone 9 steps 1 to 3, milestone 10 steps 1 to 4, milestone 11 steps 1 to 5 and part 6a. The README status line says the same.
- Remaining steps (last step of each milestone is physical acceptance, deferred): milestone 8 steps 3 to 8, milestone 9 steps 4 to 10 (step 10 is docs plus physical acceptance; do the docs part), milestone 10 steps 5 to 6, milestone 11 steps 6b to 10, milestone 12 steps 1 to 11. Then a clean `pnpm -w run validate` on feat/sync and the final report.

## In-flight branches (pushed to origin)

Each was built by an agent that stopped when the previous session ran out of credit. Their briefs, fix messages, and progress files are in `briefs/` and `progress/`. To continue one, check out the branch, apply its WIP patch if it has one (`git apply --index wip/<name>.patch`), read the brief, the fix message, and the progress file, finish the work, run the brief's full check list, commit, then review and land.

1. `feat/sync-notes-structure`: milestone 8 step 3, Notes structure replication. Manifest 14, migration `20261025091420_notes_structure_replication.sql`. A review returned "fix first" (`briefs/m8-step-3-fix-message.md`). Committed fixes: `d86cdd93` (row pages removed with their data source, unplaced row pages hidden), `c0245185` (project history restore in place on synced vaults), `3c30f5af` (pairing repair, cycle pages detached to their project root). Uncommitted and unverified: `wip/notes-structure-uncommitted.patch` (the durable pending-repair marker, `has_pending` in `sync_repair`, and related test and manifest edits). Still open from the fix message: finish item 5, the Low items, the coverage items, then the full check list. Then a short delta review against the fix message, then land.
2. `feat/sync-chat-conversations`: milestone 9 step 4, Chat group B. Manifest 14, migration `20261025060716_chat_conversations_replication.sql`. First fix round done (`61d787aa`, `8e83c802`, all checks passed). A delta review returned "fix first" for one remaining item; the second fix round is written in `briefs/m9-step-4-fix2-message.md` and has not started. The coordinator decision recorded there: keep the documented replace precedence of folder and scratch approval overrides (a folder grant may be looser than its channel), and fix only the concurrent inherit-versus-explicit case through a conflict-aware hook or a fail-closed effective view, or stop and report an engine need. Then a delta review, then land.
3. `feat/sync-anchor-handover`: milestone 11 step 6b, engine track. Brief `briefs/m11-step-6b-brief.md` (part 1: 6a review follow-ups; part 2: handover and repin, restore detection and self-handover, recovery kit, quarantine, fork freeze clearing). Progress so far is in `progress/milestone-11-step-6b-progress.md` (part 1 items 1 to 8 done, quarantine, handover catch-up, restore detection, self-handover, and proposal forwarding done). Uncommitted and unverified: `wip/anchor-handover-uncommitted.patch` (a new `sync/service/anchor.rs`, sync commands, the command permission set, runtime registration, and replica handover tests; the agent was about to run fmt and a compile check). Check part 2 of the brief against the progress file for what remains (the recovery kit is the likely gap), finish, run the full check list including the Android check, then a fresh Opus review, then land as part 6b and tick step 6.

Version conflict: the Notes and Chat branches both use manifest 14. Whichever lands second merges feat/sync, takes 15, renames its migration to a fresh UTC timestamp so it sorts last, regenerates its manifest fixture, and reruns its checks. The anchor branch changes the proposal format and takes the next handoff protocol number at merge.

## Next steps in order

1. Finish and land the three in-flight branches as above (two implementers at a time).
2. Milestone 9 steps 5 to 9 and the docs part of 10, one at a time on the `chat` track.
3. Milestone 8 steps 4 to 8 on the `domain` track (step 4 needs the notes from `progress/milestone-8-step-3-progress.md`: `parent_data_source_id` gains its replicated Cascade reference, row page orphan handling moves to the engine reference).
4. Milestone 10 steps 5 and 6 and milestone 11 steps 7 to 10, serially on the `engine` track. Step 9 uses the internal Opus security review (user decision).
5. Milestone 12 steps 1 to 11.
6. Close each milestone (validate, review, advisor), then a final clean `pnpm -w run validate` on feat/sync.
7. Final report: the pending physical acceptance checklists, deduplicated from each milestone's Manual acceptance section and `final-report-notes.md`, plus the pre-release items recorded there (Calendar duplicate repair references, the untested `drain_and_flush`, step 9 notes on the discovery key and trust in final rejections). If time allows, the Calendar duplicate repair follow-up.

## How the work was run

- Before each step: write a brief (pattern in `briefs/`), ask the advisor about the approach, then launch one implementer: `general-purpose`, Opus, effort medium, `isolation: "worktree"`, in the background. The prompt says: read and follow the brief exactly; feat/sync is now `<sha>`; work in your worktree only and never `cd` into the main checkout; do not start shell watchers that pgrep their own command line; merge commits are `merge: feat/sync into <branch>`; source the Android env script from inside your worktree; never end your turn while your own checks are still running.
- Every implementer follows `implementer-brief.md` (build lock, per-track target dirs, signed commits, progress file format, Android compile check). Replace `<SCRATCHPAD>` in the copied briefs with your own scratch directory; progress files live there until landing.
- After each step: a fresh read-only Opus review with a brief like `briefs/m8-step-3-review-brief.md` (verdict merge, merge with follow-ups, or fix first, findings by severity with file:line and a failure scenario). On "fix first", send the implementer a fix message (`briefs/*-fix-message.md` pattern), then a delta review. Follow-ups that are safe to defer go into the next step's brief.
- Landing: `git merge-base --is-ancestor feat/sync <branch>`, `git merge --no-ff --no-commit <branch>`, check the diff, versions, and migration order, `git commit -qS -m "merge: <title> (sync milestone M step K)"`, confirm `git log -1 --format=%G?` is `G`, run `tick.sh M K <progress file>` (it replaces the step's unticked line in the plan with the progress entry), remove the worktree and branches, prune, delete the Android target dir of that worktree, update the README status line, and commit the goal changes (`docs(goal): record sync milestone M step K`). `land.sh` does most of this.
- Decisions the plan does not cover are taken by the orchestrator, recorded in the step's progress entry, and listed in `final-report-notes.md`.

## Machine setup (the old host, adapt to yours)

- Every Cargo, Vitest, or pnpm check ran under one lock so two agents never built at once: `exec 9>$HOME/.cache/ganbaru-checks.lock; flock 9; <command>`, in the background with a long timeout. That host had 7.6 GiB of memory; set `GANBARU_VALIDATE_PROFILE=low-memory` if yours is small.
- Target dirs per track under `$HOME/.cache/ganbaru-targets/`: `engine`, `domain`, `chat`, `review`, and `android-<checkout folder>`.
- Android: `cargo check --target aarch64-linux-android` only, no APK builds. The env script (`~/Android/android-check-env.sh` on the old host) set `JAVA_HOME` (OpenJDK 21), `ANDROID_HOME`, `NDK_HOME` (NDK 30.0.15729638), the `aarch64-linux-android29-clang` compiler, linker, and `llvm-ar` variables, and a per-checkout `CARGO_TARGET_DIR`. The check command and its baseline warnings are in `implementer-brief.md`. Without the toolchain, record the Android check as not run.
- Commits must be SSH-signed with the user's configured key; if signing fails, stop and report.

## Files here

- `implementer-brief.md`: the rules every implementing agent follows.
- `briefs/`: the briefs, review briefs, and fix messages of the in-flight steps, usable as templates.
- `progress/`: progress entries of the unlanded steps.
- `wip/`: uncommitted work of the two agents that stopped mid-change.
- `final-report-notes.md`: running notes for the final report (landings, decisions, deferred items, physical checks).
- `user-notes.md`: notes for the user's manual checks.
- `tick.sh`, `land.sh`: bookkeeping and landing scripts (`GANBARU_REPO` and `SCRATCHPAD` set the paths).
