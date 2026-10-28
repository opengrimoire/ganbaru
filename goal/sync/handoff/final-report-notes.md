## M3 review low findings left open (2026-10-22)
- Android foreground app does not run discovery on a failed dial; recovery waits for the next worker pass (up to 15 min). Fix idea: schedule the expedited one-time worker on a foreground dial failure.
- CLIENT_DISCOVERY tokio mutex held across the 3 s browse; concurrent callers queue.
- Android connectivity probe is uncached (main thread round trip each pass); Linux caches 60 s.
- Address ranking cap of 8 can be filled by private bridge addresses, hiding overlay or global IPv6.
## M2 review finding deferred to M10 step 4
- Hub keeps unreferenced uploads in replica-writable classes (profile images); the generic unreferenced sweep in M10 step 4 must cover every class on the hub.
## M10 step 1 physical items
- Vault lock on Windows (swap closes and relocks), Android (command registration, release on exit, backup restore lock: uncompiled), FUSE and SMB lock behavior.
- Step 6 note: a failed vault switch leaves the old vault unlocked until its next connection.

## M4 step 4 contacts review (2026-10-22)
- Findings 1-9 sent to the contacts agent for fixing (block vs concurrent remove, standing trust revocation, idempotent pending receive, duplicate pending sent rows, revision clamp docs, long migrated ids, card re-verify on accept, frontend card revision refresh, status line qualifier).
- Physical: contact requests and accepts across desktop A, desktop B, phone P with concurrent block/remove and trust edits.

## M4 steps 1-3 review (2026-10-22)
- Findings 1-6 sent to a fix agent (theme editor stale overwrite, replica config.json re-import, theme deleted during edit, rename updated_at max, restore name uniqueness, asset class docs).
- Deferred to M10 sweep: replaced profile pictures leave orphan files.
- Physical: concurrent theme edits on two devices with the editor open; preference reset on one device while a replica reopens.

## M2 review fix gaps (2026-10-22)
- F9 (sync status read in one transaction) and the F4/F5 writerless dirty-gap path have no dedicated tests.
- Optional check that composite `key_inputs` match the primary key columns in order was not added.
- F4/F5 uses an iterative duplicate choice (up to 8 rounds, then ignores exclusions) instead of provisional visibility.
- Protocol 9 (acknowledgements): all linked devices must update together; physical check.
- Protocol 10 (M11 step 2, sync device key in enrollment): all linked devices must update together and re-pair; physical check.
- M4 known gap: after an active theme fallback (linked device deleted the active theme), the saved theme choice is only reconsidered at startup (`reloadOnce` uses in-memory `activeId`).
- M5 step 4: Android paths uncompiled; release-only 1,000-item search refresh budget test (ignored) not run; on Android, music availability after synced changes catches up only at the next source refresh.
- M6 step 1: restore, rollback and handoff replacement rely on the app closing database connections before the swap so the projects conversion runs on next open; verify in M6 step 2 and include in M6 physical acceptance (restore a backup, then sync).

## M5 review (2026-10-22)
- Findings 1-9 queued for a fix agent (brief m5-review-fix-brief.md): recreate after delete for items, locations, collections (high); preset over 16 layers; docs status; intended uses recoverable; resolution state register and terminal states; restore defaults; eligibility counts; rekey invariant; doc and copy cleanup. Plus scenario tests for six acceptance rows.
- Manifest version: M5 fixes and M6 step 4 both bump; second to merge takes the next number.

## M10 step 3 compaction (2026-10-22, merged 713d74f0)
- Performance budgets pending: release numbers on target hardware (debug: 255 ops in 32.5 ms).
- Reseal after a fork can leave the head at a compacted sequence (consistent, worth a later look).
- ganbaru-sync-replica lib suite takes about 210 s (was about 104 s on 45e78d3a); investigate in M10 step 4.
- Physical: a device dormant past compaction gets bootstrap required; relink flow for devices with no link time.

## M10 step 4 asset collection (2026-10-23)
- Android-gated edits (runtime/mobile.rs, Android archive call in vault/backup.rs) uncompiled.
- Sweep keeps only references from live rows and retained versions; restoring an old `.database-rollbacks` copy can reference assets already collected (24 h grace). Physical: restore a rollback older than a day and check images, artwork, profile pictures.
- Replica test suite 212 s to 21 s via a migrated template.

## Milestone 11 external security review
- The plan requires an external security review of steps 1 to 4, 6, 7, and 9 before step 9 merges. Agents cannot perform it; a dedicated internal review agent runs as a stand-in before step 9, and the external review stays pending for the maintainer.

## M5 close (2026-10-23)
- Review fixes merged a0025a3b; manifest is now v6, so development vaults with Music sync state need a reset. M6 step 4 takes manifest v7.
- Physical rows for M5 (step 7): unbound and "Not on this device" labels, root binding and refresh, YouTube playback on linked devices, the YouTube playlist follow flow, drag reorder, recovery offer and restore in Settings, play counts after playback, Focus phase playback, soundscape playback including past 16 merged layers.
- Known limitation: Music successor keys diverge or duplicate when a device missed a delete or saw a different deletion set (restore of the old key next to `{key}-2`). Not covered by tests; check physically when re-adding removed sources.
- M6 step 3 deferrals: cross-project Chat references keep NO ACTION (milestone 9); project deletes applied by sync get no Chat attachment file sweep (milestone 10 collection); Projects store replica flag moves to step 5. Physical: delete a project with Chat attachments and confirm the files go.
- M11 step 1 landed 0c9ab833; step 2 drops certificate v1 and kind 2, so dev vaults with sync state need a fresh vault until Reset sync state lands in M11 step 6, and every device re-pairs.
- M6 step 4: a cleared Projects custom field value and a concurrent edit settle on the newest existence write (latest wins), not a guaranteed clear; accepted as LWW. Projects recovery restore moved from step 6 to step 5 (Rust), step 6 keeps presentation.
- M6 step 5 landed (c197227b). Fix 929ec5df: the replica guard test probes `calendars`, since Projects is now replica-writable. Two Notes Vitest files (NotesView.preview, editor-component-registry) failed in an unsharded two-worker full run but pass alone; watch for flakiness under load in the final validate.
- M6 step 6 landed (bafcf7b7): Projects conflicts use per-domain commands `projects_conflicts` and `projects_resolve_conflict` (same pattern as Quick notes and themes). Recovery previews show titles and a task description preview, not parent names (adapter presentation lacks parent context). "Keep removed" on a cycle-hidden dependency seals an existence write of 0. No component tests for conflict cards; mapping and helpers are tested.
- M6 review (Opus): no high findings. Replica deletes never hit guarded Calendar tables (group delete and event link or unlink are owner-gated; status and priority deletes are soft). Fixes in flight: conflict command tests, restore cycle and depth checks, detached-window owner prompt, archived folder environment, task history on conflict resolution, unlinked vault conflict listing, logged emoji removal. Deferred: Chat file sweep leak in project deletion (M9); 64-byte folder id migration guard not added (dev vaults only). Test gap to cover in M7: applying a Projects delete on a guarded replica with event links.
- M6 manual acceptance (A owner and coordinator desktop, B linked desktop, P Android; offline edits with network disabled, then reconnect):
  | Scenario | Expected |
  | --- | --- |
  | P creates a project offline | It appears on A and B with default sections, statuses, priorities, and views; its managed folder appears on A only when opened; A's Chat view gets the default channel. |
  | A and B edit one task title offline | Conflict shown on all three; resolution converges. |
  | A and B reorder tasks in one section | Same order everywhere. |
  | A adds T blocks U, B adds U blocks T | One edge remains everywhere and a conflict names the hidden one. |
  | A puts T under U, B puts U under V | Depth 1 everywhere, same result. |
  | A deletes status S, B moves a task into S | The task stays in S, S stays visible while used. |
  | A and B set one single-select field to different options | One value everywhere; no orphan option rows. |
  | B sets a number field to 12.345678, P enters 0.1234567 | 12.345678 shows the same on A and P; P's entry is refused. |
  | B adds an external folder | A sees the binding as "locate", sets its own path; B's path unaffected. |
  | A deletes a group while B edits a task inside it | Group gone everywhere; a recovery entry restores the project with B's edit. |
  | B sets a custom emoji project icon | Icon bytes reach A and P. |
  | A deletes playlist L, B assigns L as focus playlist | Project shows no focus playlist everywhere. |
  | B tries to delete a group | Ownership prompt, no partial write. |
- M7 user action: measure the largest encoded timing value (event plus overrides) in the real vault; if over 256 KiB, M7's fallback (override content in its own Register group) applies. Step 1 measured only repository fixtures.

## M6 review fixes landed
- Merge 67dbee24 (fix/sync-projects-review): restore cycle and depth rules, conflict command tests and task history, empty conflicts on unlinked vaults, archived folder environment refused, emoji removal logging, ownership prompt in detached windows, Projects apply budget measure. Focused checks green (projects 97, chat 148, sync 250, replica 74, tauri 1030, clippy, client check, vitest 1478).

## M11 step 3 (removal cutoffs)
- Measurements (release, this machine): removal excluding 1,024 ops 308 to 355 ms (1 s budget); displaced retention about 6.5% (10% margin, may be flaky on noisy machines); rederive of 1,000 entries 143 ms; apply of 256 ops about 200 ms and seal of 100 notes 57 to 74 ms both over budget at base.
- Step 3 open questions decided for step 4 (advisor): dependents of excluded ops are ready when the selected state excludes the dependency (removal or fork); kind 4 entries get their own excluded storage bound per writer.
- Maintainer decisions: hardware baseline for seal and apply budgets; retention test variance.
- M11 step 3 landed as 66dffe31 (signed). Focused checks: contracts 101, sync 260 (2 ignored), replica 74, projects 97, tauri 1030, db sync 8, clippy, fmt.
- Release budgets on feat/sync 66dffe31 (this machine, median of 5): capture 67 us (200 us, pass); seal of 100 notes 73 ms (20 ms, fail); apply of 256 Quick notes ops 229 ms (50 ms, fail); apply of 256 Projects ops 464 ms (50 ms, fail, confirmed release); retention 268.9 vs 267.3 ms allowed (10.7% vs 10%, fail by noise); removal of 1,024 ops 339 ms (1 s, pass); rederive 145 ms (no budget).
- Decision (user gave full autonomy, 2026-10-23): after M6 closes, run one bounded engine-track profiling step on apply and seal (Projects and Quick notes). Take cheap wins; if none, lower the per-transaction batch cap so one write transaction stays within the budget. Milestone 1 recorded apply 45 to 48 ms and seal 15 ms (machine unknown).

## Bundle contracts (8043d98f)
- Dependency conflict panel lazy-loaded to keep the first-use route at 377 modules.
- Android ceilings raised by the 11 Projects sync UI modules (conflict store, API, presentation, i18n, owner-required guard); shell +1, projects +9, others +5, total 1312.

## M6 closed (45977229)
- Physical acceptance: milestone_6.md "Manual acceptance" table (13 scenarios, two desktops plus Android).
- Dense dataset migration and first seal budgets not measured; profiling step measures first seal.
- M6 fix review (Opus): no findings. Low note accepted: the one-time conversion deletes custom emojis whose icon file is missing; it only touches rows without a stored size (pre-conversion dev vaults), and there are no external users.

## M7 step 2a decisions (blocker resolved)
- Calendar owned tables exceed the engine's static worst-case owned bound (320 KiB). Decision: opt-in Domain bound mode on OwnedTable (manifest.rs); the domain keeps every value within the blob bound, and seal's existing invalid-row path is the safety net. Calendar generalizes the 256 KiB estimate to every Domain-bounded group; oversized iCalendar properties (inline ATTACH) are dropped from preservation with a diagnostic.
- Override component FK becomes an owned reference; owned uniqueness enforced by writers; events group 27 unused; icalendar_objects gains import_id for the export rule.

## M11 step 4 landed (a0de7409)
- Deviation: after the anchor confirms one fork branch, exclusion cannot restore the versions that branch displaced; a clone destroyed before it reseals leaves divergence. Resolved in M11 step 5 (engine fix folded into the step 5 brief, so the peer simulation can assert convergence).
- For step 6: a device that forked and later becomes anchor is not frozen; handover must clear the freeze; anchor reseal under a frozen epoch needs an end-to-end app service test.

## M7 step 2a landed (77363400)
- Budgets: seal of 1,000 imported events 1.94 s (3 s budget); apply on second replica 1.33 s desktop; Android unmeasured (physical acceptance).
- Deviations recorded in milestone-7-step-2a-progress.md (D1 to D6, flat owned tables, date-keyed sets, id charging, 100 task links per event, import skips events repeating owned keys).
- Archive hold gap and task link churn handed to part b.

## M7 step 2b landed (cdf5bc15)
- Calendar replica writable, hooks, restorers, timing conflict admission filter, conflict commands, task link diff writes.
- Engine follow-up queued (engine slot after M11 step 5, with profiling): archive hold gap. `sync_capture` CHECK requires held rows be deleted; relax via a sync_capture rebuild migration and review `captured_in` early-seal skip, `release_hold`, insert trigger reinsert branch; then assert no table 164 rows or tombstones in `calendar_a_deletion_undone_inside_its_hold_publishes_no_event_tombstone`.
- Fixed on feat/sync: receiver test pinned PROTOCOL_VERSION 10 (stale after M11 step 4); assertion removed. Lesson: the merge filter `sync` missed it; run full tauri-app lib tests on merges that bump protocol.

## M7 step 2c and 4 landed (f948d1d2, 582946c3)
- 2c raised bundle ceilings with measured reasons (first-use 378, Android total 1320).
- Step 4: rules in SQLite, ten commands, config.json keeps only chat, native host reads a device-local rules file bound to the active vault path. ROADMAP synced list updated (907b371b).
- Silent backend refusal of rule edits handed to M7 step 5 as a small UX fix.

## M11 step 5 landed (488cad46)
- 48 seeds per peer and star sim converge. Budgets: fork exclusion member 375 to 440 ms, anchor 274 to 344 ms (1 s). Retention flaked (member 298 to 305 ms against 277 to 282 ms anchor-relative limits); robustness handed to the engine follow-up.
- Open for step 6: restatement tags anchor-local (non-anchor admin restoring a late change may see the restatement as newer); user edit equal to the restated value counts as restatement; no focused stale-row test (seed 21 only); app sealing and acknowledgement rotation may orphan captures; cutoff can fall below confirmations after a handover without a vector; pre-migration rows have no creation entries.
- Migration order: sync_row_creations (20261023202142) sorts before distraction_rules (20261023233521); both new on feat/sync, independent, no external users, left as is.
- Engine follow-up (archive hold, profiling, retention robustness) spawned from 488cad46 on perf/sync-engine-follow-up.

## M7 step 5 landed (8ad8db4f)
- Took handoff protocol 12, manifest v10, migration 20261024015213 (clears existing samples; dev only). Table 278 totals; samples Local; flush at most every 5 min per bucket plus exit, Android background, and snapshot flushes.
- Android bundle ceiling 1320 to 1321 for the edit-status module. `pnpm --dir apps/client run bundle-contracts` needs Turbo build output; use `pnpm -w run bundle-contracts`.
- Physical acceptance item: a cloned installation sharing one device id undercounts usage (both write the same total keys).
- Step 6 cleanup: native-messaging spool error text still says "linked-device"; block events stay owner-only until step 6; runtime snapshots device-local.
- Old device-local spool tables are not dropped (dev only, no shims); fresh spool has 2 tables (tested).

## Engine follow-up review (perf/sync-engine-follow-up)
- Opus review: e0de1b27 (held creates), 89a180e5 (cascade root cache), retention test change sound. 91da4e56 blocking: 16-change op and apply caps let receivers seal repairs against half-applied constrained batches (lost Projects moves, Music unique swap tombstones). Fix forward requested on the same branch (prefer constrained writes kept in one op, else no mid-batch apply split); recheck by the same reviewer before merge.
- Maintainer item: retention test is now sound (11 alternating pairs) but pair ratios 0.6 to 1.9 under load mean the 1.1 margin needs a quieter machine to judge; seal of 100 notes still 78 ms against 20 ms (bounded planning is the follow-on).
- Replica test harness never runs seed_captures; migration-seeded `local` calendar events fail to seal there without it.

## M7 step 3 (Focus history), merged 12f3f0c2
- Manifest v11. Migration 20261024045821_focus_replication.
- Engine track backlog: Focus seal of a closed run (40 segments, 300 events) is about 270 ms against 100 ms; the cost is the engine's per-row seal cost (about 0.78 ms per row), not the Focus adapter. Bounded seal planning is the follow-on.
- Deviations: plan, outcome, and inheritance groups are Register; inherited_from_run_id is plain text; partial unique indexes became gated triggers; Calendar edits through the Focus owner need both domains writable; the admission skip applies to automatic starts only.
- Physical: start confirmation names the other device; the desktop automatic admission skips an occurrence another device focuses on; closed runs appear on the other device.

## Engine shape needed before M7 step 6
- Block events (table 279) must publish with their run when `run_id` is set and immediately when null. The engine cannot express this: `publication.rs` `condition_over` makes a null reference never publish, and `publication_problems` requires the `published_with` column to be an immutable Cascade reference (baseline FK is SET NULL). Engine-track step: optional parent publication (null publishes immediately), and decide Cascade versus SetNull for the run reference.
- M8 step 6: `AssetStore::read` returns `Vec<u8>` (`crates/ganbaru-sync-replica/src/assets.rs:101, 225-236`), buffering whole files; conflicts with the "one chunk plus hashing state" memory budget for 50 MB Notes files.

## Engine follow-up, merged 88865ad1
- Archive hold (closes the M7 step 2 archive hold deviation), cached cascade roots, apply bounded at 16 ops or 16 changes per transaction, constrained groups and published parents with children sealed together, continuation across a writer's large operations.
- Budgets: apply Quick notes 23.5 ms, Projects 38.3 ms (50 ms); seal of 100 notes 85 ms (20 ms, fails); Focus closed-run seal 286 ms (100 ms, fails); retention ratio 1.046 (1.1, noisy); dense Music first seal 71.5 s (no budget).
- Reviewer follow-ups: continuation is unbounded for a writer whose every operation exceeds 16 changes; a group can still split at the 512 KiB cut or at the 32 MiB pass boundary, which reopens repairs sealed against half-applied groups during large catch-ups.
- Asset sweep: a row attached after the sweep's second reference read that points to an existing aged file can lose it. M8 step 6 (history pins) and M9 step 6 (queued attachments) attach paths must refresh the file's modification time (or use an engine `touch`) when attaching an existing file.
- M8 step 2 must first fix (delta review of step 1): callout and button continuation blocks in `import_writer.rs:278` `continuation_payload` must keep the required `icon` (an over 128 KiB callout import fails), with a callout split test; `agent_bridge_export.rs` `load_pages_with_descendants` leaves its two depth placeholders unbound (the binds sit in `load_exact_pages`), so descendants are never exported; add a descendants test. Also: a migration test for a nested page with `__ganbaru_project_id` ending NULL; history-and-recovery doc must say older-format versions are refused. Step 3: add the `project_id` CHECK during the `notes_pages` rebuild, drop the CHECK near line 604 and the four-way parent CHECK, and revisit the per-row derived project walk cost in shell loads. Block moves mark only the destination page dirty.
- M9 plan contradiction: `chat_agent_runs.assignment_id` is SET NULL in Implementation checks but CASCADE in References; code uses CASCADE. M9 step 4 must make `after_materialize` handle memberships, grants, and teammate policy (their audience triggers are gated since step 2).
- Engine follow-up 2 landed (af3b1709): operation format 4 with the group-continues flag; chains apply only once closed; `PublishedWith.null_publishes` (M7 step 6 must rebuild `distractions_block_events.run_id` to ON DELETE CASCADE and declare `null_publishes: true`); `DeviceTable.assets` Local asset references (M8 step 6 and M9 step 6 declare). Bounded seal planning (part 4) not done: 100 notes seal about 63 ms against 20 ms, Focus closed run about 264 ms against 100 ms. Follow-up 3 brief written for bounded deferral and take-back.
- M7 step 6 landed 0dacb4e4 (review: merge with follow-ups; Medium spool stall on a vault-refused row, docs claim, segment without run, seal test workload, missing tests moved into M7 step 7). Android device checks: plugin compiles without `usageOnly`; Guardian export returns usage and block kinds in one batch; a block during a running Focus session reaches the vault with its run; acknowledgement only after commit.
- M8 step 2 review: fix first (delete_link Own pairing, copy with archived related rows). Fix message in m8-step-2-fix-message.md, queued for the next free slot (resume agent a9f8865cf7a34ad7c). Step 4 follow-ups: relation pairing read from config can flip under concurrent edits; duplicate property names collapse in the name-keyed schema.
- Queue after slots free: M8 step 2 fix round, then M9 step 3 (brief m9-step-3-brief.md).
- Engine follow-up 3 finished (fix/sync-chain-deferral, f3a489ae): Part 1 chain deferral fixes; Part 2 safe seal wins: 100 notes 84 to 64 ms (budget 20), closed Focus run 421 to 215 ms (budget 100). Both still fail; reaching them needs batched multi-row statements in every seal phase (about 30 statements per note row at 20 to 27 us each). Candidate engine follow-up 4 (batched sealing), engine track, after M11 work or when the engine track is idle. Continuing group budget first real number 57 ms (recorded against 50 ms, record only). Retention ratio test is noisy (1.181 once, 1.091 rerun).
- M8 step 2 fix round started (resumed agent).

## Engine follow-up 3 review follow-ups (for engine follow-up 4)
- Medium: chain opening takes candidates in (clock, writer) order (apply.rs:557, 614-616; gate apply.rs:1275), so a hostile paired writer with low clocks can win every opening and stall an honest writer's chain. Fix: deterministic fair selection (longest waiting or round robin).
- Low: `SealReport::record` (seal.rs:66) ignores `stale_left`; `expand_references` (repair.rs:345) exceeds the bound; docs qualifier "receivers that can seal" missing at bounds.rs:81 and docs/algorithms/sync/README.md:13.
- Missing tests: single writer back-to-back chains (original FU2 finding), take-back through apply with more than 4,096 rows, local writer frozen mid-chain.
- Deferred part B: batched multi-row sealing to meet seal budgets (100 notes 64 ms vs 20; closed Focus run 215 ms vs 100). Re-measure after M8 and M9 tables land. Known deviation until then.
- User-gated steps inside the goal (not physical acceptance): M11 step 9 merges only after the external security review; M12 step 10 needs maintainer approval for workflow changes.
- User decision (2026-10-24): timing budgets are not blocking; finish the implementation first, optimize later (batched sealing deferred past the goal). Physical acceptance is approved as deferred. GitHub Actions workflow changes are approved (M12 step 10). External security review for M11 step 9: replaced by an internal fresh Opus security review under the user's full decision autonomy; record it in the M11 progress entry.

## Milestone 7 step 7 landed (ba9b6b09)
- Budgets: Calendar import seal 1.64 s (budget 3 s), desktop Calendar apply 1.71 s (Android not measured, budget 10 s), closed Focus run seal 187 ms median (budget 100 ms, missed, deferred perf).
- Android: no SDK on the host; the Kotlin review was clean. The checklist for the first device build is in milestone-7-step-7-progress.md and goes into physical acceptance.
- Known gaps for the milestone review: the Android Guardian batch imports in one transaction (one refused event blocks the batch); untested: reminders following the winning time, Restore into Local end to end, equality with the previous default distraction config, Calendar unchanged owned row ids.

## Milestone 8 step 2 landed (5b680dc6), delta review: merge with follow-ups. For step 4:
- `write_page_properties` (page_property_storage.rs:158) still falls back to the name when an entry's id names no property; match the migration (stale id stays orphan).
- `canonical_properties` (data_sources/schema.rs:256) is case-insensitive on names, `decompose_schema` (property_storage.rs:165) exact; make them agree.
- Docs `docs/data/schema/notes-and-projects.md`: unique property names, hidden orphan values on non-row pages.
- From the fix round: concurrent name collisions need deterministic resolution; relation pairing from config may need edge re-canonicalization on merge.
- Android toolchain installed (cargo check only). 3258767c fixed Android compile breaks from 0a0f1110, e59d62db, 3fe5d42e. Known low gap: deleting a project group on Android leaves that project's managed Chat files on disk (no Chat repository on mobile). Baseline mobile dead-code warnings: keys_after, ChatDefaultsScope::WorkingFolder, received_request_by_peer, reported_roots, active_database_path.
- Engine follow-up 4 part A landed f900d1c2 (fair chain opening by stored order, stale_left in SealReport). Review: land with follow-ups (engine follow-up 4 part C, run before M11 step 6): (1) Medium: stored_at_ms is wall clock; a backward clock step lets a busy writer delay another chain up to the step size; store max(now, space max stored_at_ms) or a counter, check acknowledge.rs:163 and stability.rs:103; or qualify docs. (2) Low: clear `winner` on Heads reload (apply.rs:1338-1376, ~1520/1530) or set `more`. (3) Low: add opposite-stored-order convergence test across two receivers. (4) Low: rename or scale `a_planned_row_brings_every_row_that_references_it_whatever_the_count`.

## M11 step 6 split (advisor, 2026-10-24)
- Step 6 lands as 6a (`feat/sync-anchor-service`: FU4 part C, keys, charter, enrollment and reconciliation, outbox, propose, sequencing, counted set from the log, protocol 13) then 6b (`feat/sync-anchor-handover`: handover and repin, restore self-handover, recovery kit, quarantine, clear fork freeze on handover, cutoff below confirmations after handover). Brief: m11-step-6a-brief.md; write 6b after 6a's first progress update.
- Queue order when a slot frees: M8 step 3 first, then 6a. M11 step 7 can run beside 6b.

## M7 closed in source (f5db2ce7, docs fix 50708f56)
- Pending follow-up "Calendar duplicate repair references": task links and Music assignments of a hidden duplicate must follow the survivor (adapter callback in repair.rs, Calendar adapter implements it). Deadline: before tombstone compaction or a user-capable release. Plan: engine follow-up after M11 step 6a, domain half paired with it.
- M7 device checklist additions in milestone_7.md review fixes entry.
- Re-import doc checked: the test adds the alarm before it reaches the importing replica, so "edits that arrive after the import merge as usual" holds; the file wins over edits that already arrived (reviewer reading of diff.rs).
- Duplicate repair deadline: no remaining milestone drops rows under tombstones (M10 compaction empties envelopes only, rows and tombstones kept), so it is a pre-release item for the final report's pending list; still schedule it after 6a if a slot allows.
- Known untested path: `drain_and_flush` flush-on-failure order (usage_totals.rs:44-67) needs an AppHandle. Device checklist: force a spool step failure and confirm usage totals still flush.
- Full validate after M7 fixes deferred to the goal's final validate (fix round ran the full Rust gate; no frontend change).

## M9 step 3 landed (a3093119) and M8 step 3 blocked
- M9 step 3 review: land with follow-ups. Fixed on feat/sync by me: `custom` ranked least restrictive; chat schema doc (Custom ranking, home device unread); double comma in schema/sync.md. Not edited: migration comments that narrate history (lines 3, 412 of 20261025015209), left because the migration may already be applied on a dev vault and checksums are validated. Remaining follow-ups go into M9 step 4 brief (hook test for older stricter pointer, stale check by id, copy-to-revision check, plan text note).
- M8 step 3 blocked: engine lacks a Tree on one reference column of a two-reference placement group, repair to a domain root placement (page root) instead of NULL, cut-off detection against that root, and sibling order grouped by the whole placement. Plan: engine step "tree placement roots" after M11 6a (engine serial), then rerun M8 step 3 with the agent's findings (progress file milestone-8-step-3-progress.md: parent_data_source_id FK deferred to step 4, both tables rebuild, unplaced columns, hook invalidates index fingerprints).
- M8 step 3 decision (with advisor): no engine change. Blocks declare no TreeSpec; block cycles and depth move into the Notes repair pass. Accepted deviation: a cycle's target page comes from hook-derived page_id, so concurrent cross-page moves can produce a placement register conflict that converges; a cycle with no known page stays unplaced until a device that knows it repairs. M8 step 3 resumes on the domain track in the next free slot.
- M11 6a landed 419c6128 (review: merge with follow-ups, all carried into the 6b brief m11-step-6b-brief.md part 1). 6b waits for a free implementer slot (M9 step 4 and M8 step 3 running).
- M9 step 4 review: fix first (2 High priority rules, 2 Medium hook issues). Fix message m9-step-4-fix-message.md queued for the next free slot (resume the step 4 agent).
- M8 step 3 review (2026-10-25): fix first (orphan row pages, restore abort on synced vaults, pairing repair, detach_page, durable repair marker). Fix message at m8-step-3-fix-message.md, queued for the next free implementer slot (agent ae93dc68c6342a252).
- M9 step 4 delta review: fix first (High 1 remainder for grant and scratch overrides vs inherit; coordinator kept documented replace precedence, fix only the inherit vs explicit concurrent case via conflict-aware hook or fail-closed effective view). Message m9-step-4-fix2-message.md queued for the next free slot.

## Cloud session (2026-10-25)
- Host: cloud container, 4 cores, 15 GiB, about 40 GB disk allowance. Commits are unsigned (the container has no usable signing key; the user approved unsigned commits on feat/sync and its topic branches for this goal). The Android compile check is not run anywhere in this session (no toolchain). No advisor tool is available in this session, so milestone closes use a fresh Opus review only.
- Build dirs: one non-incremental Cargo target dir per worktree, reassigned between worktrees by touching every tracked file of the receiving worktree (Cargo fingerprints are workspace-relative, so a shared dir without touching would reuse stale artifacts).
- M11 step 6b: the orchestrator bumped handoff `PROTOCOL_VERSION` to 14 on the branch (charter-bound proposals v2, `SyncForward` and `SyncForwarded`, `sync_admin_key` in Enroll, rejection code 14 `catching_up`).
- M9 step 4 decision: the concurrent case where one device clears a folder grant or scratch approval override (inherit) while another sets a looser explicit value cannot be resolved without an engine shape (hook access to concurrent versions of non-surfaced groups and a captured group write from a hook). Deferred to an engine follow-up that must land before milestone 9 step 9, because Chat writes stay owner-only until step 9, so the concurrent case cannot occur earlier. Documented in `docs/data/schema/chat.md`.
- M11 step 6b landed (78910455) at handoff protocol 14. In 6b the anchor moves but the coordinator (hub, pairing listener) stays; members keep dialing the coordinator until step 9 adds the peer book and repin. Using the recovery kit asks the live anchor to promote this enrolled desktop; forced handover with a lost anchor and kit use on an unenrolled desktop need step 9. Reset and recovery kit controls are backend commands until step 8.
- Physical (M11): handover between two desktops while a phone proposes; restore an anchor desktop from an old backup and check members accept its self-handover; quarantine a device and lift it; recovery kit shown once, saved, and used to promote another desktop.

## Local session on the mini PC (2026-10-25, signed commits, Android check available)
- Host: the mini PC, commits SSH-signed with the user's key, Android `cargo check` run per implementer brief. Worktrees through the Agent tool's `isolation: "worktree"` plus one manual worktree for check reruns.
- M8 step 4 landed 833a93da (manifest 16, migration `20261025212528_notes_databases_replication.sql`). Final check rerun on this host found one stale ganbaru-db test asserting the step 2 template block schema; rewritten by the orchestrator (d39657cf). Review: merge with follow-ups, carried into the step 5 brief: stale rollups after a remote page delete or move (medium), template block type and payload checks in the local ensure path, option `property_key` adapter check, history for remotely removed values, `block_id = id` and page template id renewal on restore.
- Engine follow-up 5 (hook settles of concurrent group versions, used by M9 step 9) landed 74c19103 after its fix round (fcb258e7: settle only when the call can seal and no chain is open, savepoint around the settle write, docs on replicated inputs and monotonic settles). Delta review: merge with follow-ups (Low, carried to the next engine step): a domain trigger `RAISE(ABORT)` maps to sqlx `Other`, so a trigger-refused settle fails apply instead of being skipped (treat every constraint code as refused, fixture trigger test); `defer_foreign_keys` makes the savepoint's FK branch dead, so an FK violation from adapter-derived columns would fail the outer COMMIT (refuse reference derived columns in `Engine::with` or fix the progress claim).

### Milestone 11 step 7 security review (2026-10-25)

- A fresh Opus security review stood in for the external review of the Keystore step and returned fix first. High: `UnrecoverableKeyException` was treated as permanent key loss although Android 12+ wraps transient errors in it, so one transient error unlinked the device and wiped key files; a 4-byte `GKWR` envelope panicked before the length check. Medium: deleting before unlinking was not crash safe, an unlinked phone with a lost key could not relink, and StrongBox fallback was too narrow. Low: JNI exception order, substring matching of remote error text that let a hostile coordinator trigger a wipe, an over-broad wipe across vaults, and missing host tests. The fix round is queued for the next free implementer slot. Whether `androidx.test:runner` is a new test-only dependency is still open; if it is new, it needs maintainer approval before release.

### Milestone 11 step 8 (2026-10-25)

- Authority UI landed (merge e4b65518). The review found no authorization hole and returned merge with follow-ups; the coordinator applied the three mediums before landing: the recovery kit file is written owner-only, quarantine on an admin desktop warns that it also proposes removal, and discarding a late change asks for confirmation. Lows go to step 9 (`briefs/m11-step-9-followups.md`). The Android bundle module limit rose from 1323 to 1329 for six new sync modules. The manual checklist for the new screens is in milestone 11's Progress entry for step 8.

### End of the local session (2026-10-26)

- The session ended for its budget. Landed: milestone 8 step 4, engine follow-up 5, milestone 11 step 8. Pushed and unlanded: `feat/sync-android-keystore` (step 7 fix round done, delta review and the `androidx.test:runner` dependency approval pending) and `feat/sync-notes-comments` (milestone 8 step 5 committed, full checks and review pending). `pnpm -w run validate` was not run on feat/sync this session; its last full pass predates these landings, and the focused gates of each landing passed.

## Orchestrator session on the mini PC (2026-10-27)

- `androidx.test:runner:1.5.0` (milestone 11 step 7): the maintainer delegated the decision. Kept as a direct androidTest-only declaration: it is a Google AndroidX test artifact, the app module already resolves it through espresso-core 3.5.0, it never ships in an APK, and declaring it directly makes the plugin's instrumented test compile on its own. Gradle resolution is unverified on this host.
- Milestone 8 step 5 landed (merge 26180e64) after a fresh Opus review with verdict merge with follow-ups; the follow-ups (one Medium on the comment page refresh cost, Lows on history, option `property_key` validation, anchor thread reference, comment size bound, remote reaction limits, docs, and tests) were carried into step 6 (`briefs/m8-step-6-followups.md`), since none loses data.
- Milestone 11 step 9 part A (28bb1032) got a fresh Opus review before part B: continue, no blocking security hole (admission read from the log per connection, mandatory client certificates, pinned dials). Its four Mediums (member hubs refusing to relay other devices' geneses, anchor marks not recorded from a direct anchor exchange, peer book trusting reporter timestamps, no TLS-level listener tests), Lows, and doc fixes lead the part B brief (`briefs/m11-step-9b-brief.md`), followed by the unfinished part A items (separate anchor pin, pairing on a non-coordinator anchor, deferred step 6 items).
- Milestone 9 step 5 landed (merge b1e35a51, manifest 18) after a first Opus review (fix first: an empty-channel from_grant boundary hid new posts; migrated revisions could exceed the new byte limits and abort the migration) and a delta review (merge with follow-ups). Deferred to step 9: from_grant stamps can widen once Chat writes open (anchor the grant to a converged point) and synced inheriting memberships stay unstamped (fail closed, no history). Carried to step 6: search drops messages whose author participant is gone. Accepted: the migration's revision cut leaves search rows of cut messages untouched (pre-user development data only). The narrower profile switch rank (step 4 follow-up 3) goes to step 7.

- Milestone 8 step 6 (Notes assets) landed as merge a0301701 with manifest 19 and migrations `20261028002934_notes_assets_replication.sql` and `20261028030227_notes_page_history_asset_ids.sql`, after a review, a delta review, and a verification review. Decisions: Notes files are kept by a digest reference whatever their extension; page history and undo snapshots record `asset_ids` so restores keep their files; assets resolve by class and digest, never by path alone; HTML export falls back to a referenced path of the same bytes. Deferred: an end to end test of the export fallback map; a row found by a consistent path wins over a digest-named reference (only a trusted paired device can cause it); snapshots older than the `asset_ids` column pin nothing (no compatibility rule); the files byte check reads only the signature head.

- Milestone 9 step 6 (Chat attachments) landed as merge 5ba787a9 with manifest 20 and `20261028052141_chat_attachments_replication.sql`, after a review (merge with follow-ups), a delta review (fix first), and an orchestrator check of the last fix. Decisions: queued, draft, and scheduled files are kept through the attachment rows' digest columns, not `asset_refs`; `DomainManifest.unconverted` declares unconverted tables only so the sweep keeps their files (a `Local` declaration would be emptied by snapshot copies while the parent travels with the vault); the two Notes databases tests compare parsed JSON, since `agent-client-protocol` turns on serde_json `preserve_order` in workspace builds. Carried to step 7: scheduled sends copy from `chat_attachments` by id, receive-created attachment rows are never reclaimed, and table 391 waits for scheduled message replication. No end-to-end hub transfer test for Chat attachments (the replica harness has no Chat tables).

- Milestone 8 step 7 (Notes editor sync) landed as merge 49e307c1 with no migration or manifest change, after a review (fix first), a delta review (merge with follow-ups), and an orchestrator check of the follow-ups. Decisions: surfaced conflicts show in one strip above the page instead of inline banners; suggestions get no decision banner because accept outranks reject; held block edits refuse navigation, tab moves, and window close with a localized notice that opens the page, and a page removed elsewhere moves its held text into a notice with Copy text; resolution writes the version and seals the group in one transaction. Known gaps: held edits are memory-only, so Android reclaiming a hidden app loses them (follow-up for the Android shell); template and property configuration conflicts keep the shown version without a choice; Restore with my text cannot restore a purged block (page history keeps it). Android bundle ceilings rose to projects 324, notes 440, chat 328, total 1342.
- Milestone 11 step 9 security decisions. Handover precedence at a fork: a handover signed by a kit-lineage recovery key, then the anchor's own continuation, then a device-admin handover. Kit lineage is the charter key and keys installed by rotations the current kit key signed; an admin may still regenerate a lost kit, but that kit cannot overrule a stolen anchor. Handovers are vetoed by revocation evidence from the continuation subtree only, and ranking counts kit-lineage handovers first, so selection stays a pure function of stored entries. The pre-existing data loss at simulation seed 3 is fixed by extending the anchor re-seal: a replica that applied an operation while anchor re-seals the affected groups when it later excludes that operation.
