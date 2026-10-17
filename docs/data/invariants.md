# Data invariants

These conditions must remain true across UI actions, imports, migrations, recovery, synchronization, and direct service calls. Each invariant names the rule, why it exists, its enforcement boundary, and the tests expected to protect it. Numbers are stable references from other documents; add new invariants at the end.

## Pomodoro and calendar

### 1. Progress never appears after the current time

**Rule.** Historical focus or break progress is clipped to the current instant. Future projections may be visible as planned time, but they must not use the visual language of completed or active progress.

**Why.** A progress rail that colors future time misrepresents work as already performed.

**Enforcement.** Timeline projection and rendering, using persisted segment timestamps and the current clock.

**Tests.** Active, paused, completed, and future segments at exact time boundaries.

### 2. At most one Pomodoro segment is active globally

**Rule.** Zero or one segment may have active status across the database. An open run may temporarily have no active segment during a transactional transition or recovery boundary.

**Why.** The application exposes one global timer and one enforcement state.

**Enforcement.** A partial unique SQLite index, transactional phase transitions, and startup recovery.

**Tests.** Concurrent starts, repeated transition requests, crash recovery, and rapid start or stop actions.

### 3. Historical phases are stable within a run

**Rule.** Completed and interrupted segment positions, phases, durations, and timestamps never move. The active phase keeps its persisted identity. Explicit reconfiguration or an adaptive boundary decision may change future projections, but the decision and selected value must be persisted before the affected segment starts.

**Why.** History must remain auditable while future assistance remains adaptable.

**Enforcement.** Immutable historical rows, boundary-only adaptive decisions, and the reconfiguration transaction.

**Tests.** Reconfigure and adaptive decisions before, during, and after a phase boundary.

### 4. One visible Pomodoro owner occupies a time range

**Rule.** Overlapping calendar events must not render duplicate active or planned Pomodoro bands for the same occupied interval. The already-active event remains the owner while it is eligible. Other conflicts are resolved deterministically.

**Why.** Duplicated bands imply simultaneous timers that cannot exist.

**Enforcement.** One active-first selector shared by the scheduler and the timeline rail, followed by earliest end, creation identity, and occurrence identity. Recorded older runs remain visible as history. See [Time conflict detection](../algorithms/calendar/time-conflict-detection.md).

**Tests.** Partial overlap, full containment, equal windows, active nested events, and identical configuration.

### 5. Persisted evidence owns the past

**Rule.** Past and active progress is reconstructed from runs, segments, pauses, run events, heartbeats, and adaptive decisions. Configuration is used only to derive a future projection where no persisted segment exists.

**Why.** A later config edit must not rewrite what happened.

**Enforcement.** Projection order, run snapshots, segment timestamps, pause normalization, and recovery services.

**Tests.** Config edits after completion, crash recovery, inherited runs, and adaptive changes.

### 6. Past progress is never erased by calendar edits

**Rule.** Resizing, moving, archiving, or deleting a calendar event cannot delete elapsed Pomodoro history. An active run may be interrupted or shortened when its event becomes ineligible, but its completed evidence remains.

**Why.** Calendar planning is editable; work history is durable.

**Enforcement.** Protected-event deletion rules, archive relationships, event snapshots on runs, and explicit run end reasons.

**Tests.** Move, resize, recurrence split, archive, and delete while a run is active or historical.

### 7. Protected calendar events are not hard-deleted

**Rule.** An event with Pomodoro history, project links, imported preservation state, or another protected relationship is archived or detached through a domain command. It is not removed by a generic delete.

**Why.** Hard deletion would break history and interoperability identity.

**Enforcement.** Rust calendar services and foreign-key or trigger constraints. Planned CLI or external MCP surfaces must reuse the same service policy.

**Tests.** Every protected relationship, recurrence templates and overrides, imported events, repeated delete commands, and restore from archive.

## Notes and projects

### 8. A Notes page has one valid owner path

**Rule.** A page is either workspace-rooted, nested under one valid parent page, or placed through one project location. It cannot simultaneously claim incompatible parents or escape its project and folder ancestry.

**Why.** Multiple canonical placements produce divergent navigation, permissions, and history.

**Enforcement.** Page and folder validation, cycle checks, project membership rules, and transactional move commands.

**Tests.** Cycles, stale parents, cross-project moves, trash and restore, folder migration, and project history restore.

## Chat and working folders

### 9. Native Chat work has exactly one execution target

**Rule.** Organizational planning may be targetless. Before filesystem, terminal, Git, preview, checkpoint, restore, or provider process work begins, the run resolves exactly one authorized working folder or one private scratch generation.

**Why.** A single target gives every native operation an unambiguous authority root.

**Enforcement.** Assignment validation, thread and run target constraints, and the authorization service.

**Tests.** Targetless discussion, missing target, conflicting targets, folder and scratch mixing, and target replacement.

### 10. Filesystem access remains below the authorized root

**Rule.** Every native path is resolved from a validated relative path below the current target. Traversal, symlinks, replaced directories, invalid external bindings, and Git common-directory changes fail closed.

**Why.** Provider-native trust and user-authored repository content must not widen application authority.

**Enforcement.** Rust folder authorization at every file, terminal, preview, attachment, Git, checkpoint, and restore boundary.

**Tests.** Traversal, absolute paths, symlink escape, replacement, stale bindings, ignored directories, and Git indirection.

### 11. Organizational conversations outlive provider sessions

**Rule.** A channel, message, decision, or approval is not owned by a provider continuation. Replacing, forking, archiving, or deleting a provider session changes execution linkage without silently changing organizational history.

**Why.** Provider sessions are replaceable implementation resources. Team history is the product record.

**Enforcement.** Separate channel, link, session, canonical-event, and projection identities.

**Tests.** Session handoff, provider change, retry, archive, cleanup, and failed provider startup.

### 12. Effective access applies to every derivative

**Rule.** Search, summaries, unread counts, suggestions, context packages, exports, caches, and synchronized projections apply the same current membership, history, resource, and revision limits as canonical reads.

**Why.** A derivative is a common route around otherwise correct authorization.

**Enforcement.** Shared access services and authorization-complete cache keys.

**Tests.** Revocation and history cutoffs across every derivative read path.

### 13. A mention never expands authority

**Rule.** Mentioning an AI agent or resource does not add membership, history, folder, scratch, or runtime access. The mention resolves only within existing authority.

**Why.** User-authored text is not an authorization channel.

**Enforcement.** Mention resolution and assignment review through [Chat access control](access-control.md).

**Tests.** Unauthorized participant, hidden history, cross-channel references, ambiguous targets, and fabricated IDs.

### 14. Cross-channel disclosure never widens the audience

**Rule.** Content may be copied or summarized into another channel only when the destination readable audience is a subset of the source audience, or an explicit declassification creates new content under the destination policy.

**Why.** A shared participant or project does not make two channel audiences equivalent.

**Enforcement.** Publication, reference, export, and context-building services.

**Tests.** Broader, narrower, overlapping, and history-bounded audiences.

### 15. Materialized context cannot silently outlive authority

**Rule.** When a continuation has already received context that is later revoked, the application interrupts the run, denies tools and publication, discards or quarantines the continuation, and requires fresh authorization before reuse.

**Why.** Filtering future reads cannot remove data already present in model context or scratch.

**Enforcement.** Authorization revisions, revocation workflow, bounded provider shutdown, and retryable cleanup.

**Tests.** Membership, profile, folder, scratch, and audience reduction during active and idle continuations.

## Focus evidence

### 16. Plans and replicas never authorize execution

**Rule.** A Calendar commitment, notification projection, or replicated history record cannot start a run or record a phase. Recovery consumes committed SQLite execution only and never creates projected runs or later phases. Desktop automatic admission requires a fresh local activity observation after the relevant boundary.

**Why.** A plan or a copy is not evidence that work happened on this device.

**Enforcement.** Native Focus owners and recovery services. See [Focus authority](../algorithms/pomodoro/focus-authority.md).

**Tests.** Android reminders without a run, recovery after expired phases, replicated history, and admission without fresh activity.

## Adding an invariant

A new invariant must include a rule, rationale, enforcement boundary, and meaningful failure tests. Prefer one durable assertion over a list of current helper or table names. If the assertion belongs to authorization, make [Chat access control](access-control.md) normative and reference it here.

