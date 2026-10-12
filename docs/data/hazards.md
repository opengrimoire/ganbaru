# Cross-cutting hazards

These scenarios cross feature, persistence, and platform boundaries. They do not replace tests or [invariants](invariants.md); they describe failure sequences to reconsider whenever adjacent behavior changes.

## 1. Event boundary timing

One calendar block ends exactly when another begins. The first run must close at the boundary before the second block starts or inherits progress. A delayed scheduler may process both after the fact, but must preserve boundary order and must not count the delay twice.

Inheritance applies only across adjacent or overlapping eligible blocks; any real gap starts a fresh session. An event shorter than its focus phase ends the active segment as interrupted at the block boundary, and no break row is created merely because the derived plan contained one.

**Governed by:** [Pomodoro state machine](../algorithms/pomodoro/state-machine.md), [Pomodoro schema](schema/pomodoro.md), and invariant 6.

## 2. Event edits during a run

The user shortens, moves, reconfigures, archives, or deletes the event that owns the active run. The timer re-evaluates eligibility and the new block deadline. Elapsed segments stay immutable, a shortened event can interrupt the active phase immediately, and moving an event away from now cannot leave an orphaned running timer. Protected events are archived or detached instead of hard-deleted.

**Governed by:** [Pomodoro state machine](../algorithms/pomodoro/state-machine.md), [Calendar schema](schema/calendar.md), and invariants 6 and 7.

## 3. Crash, suspend, and process kill

These cases require different evidence:

- During a live suspend, the native lifecycle records suspend state and blocks ordinary progress until the return decision is complete.
- On desktop cold start, stale open state is bounded by the last valid heartbeat and persisted pause evidence. Orphaned work is interrupted, never extended to the current time.
- On Android cold start, recovery may use the current time only within a previously committed phase and its accepted deadline. A native reminder supplies no execution evidence. Expired state closes conservatively.
- On Android, an open manual or idle pause remains paused after recovery. On either platform, time away never becomes focus time.

Recovery writes are transactional and idempotent: repeating startup recovery cannot create another segment, pause, or terminal run event.

**Governed by:** [Pomodoro state machine](../algorithms/pomodoro/state-machine.md), [Plan and history](../algorithms/pomodoro/plan-and-history.md), and invariants 5 and 16.

## 4. Overlap and containment

Several Pomodoro-enabled events overlap, including a short event nested inside a longer one. The already-active eligible event stays the owner; switching because another candidate starts would split one real session into artificial fragments. Without an active owner, selection is deterministic and the rail never shows competing bands for the same time.

**Governed by:** [Time conflict detection](../algorithms/calendar/time-conflict-detection.md) and invariant 4.

## 5. DST and timezone boundaries

Calendar identity uses instants plus the event's home zone and local recurrence representation. A local clock label is not an elapsed duration. In New York, a spring interval from 01:30 to 03:30 spans one elapsed hour; in the fall, 01:00 occurs twice, so 01:00 to 03:00 spans two or three hours depending on which instant is selected. Parsing needs an explicit disambiguation policy.

Recurring events walk civil dates in their home zone, so the local start stays the same across a DST change while its UTC offset changes. History rows keep their original instants; changing the device zone affects display, not stored history.

**Governed by:** [Calendar schema](schema/calendar.md) and [Recurrence expansion](../algorithms/calendar/recurrence-expansion.md).

## 6. Rapid and repeated actions

The user double-clicks start, skips twice, stops while a transition is committing, or two windows issue the same command. Commands need stable receipts or state preconditions. Database uniqueness prevents duplicate active state, but callers must handle the losing operation without reporting success for a write that did not occur. Notification, overlay, and media side effects run only after the canonical transition commits and must tolerate repeated delivery.

**Governed by:** transactional command services, command receipts, and invariants 1 through 3.

## 7. Multiple runs for one event

Stopping and restarting the same event creates another run. Every segment, pause, adaptive decision, and run event belongs to its exact run. Display may aggregate runs but must not merge their identities or infer one continuous session. Recurrence instances keep template and occurrence identity; two dates of one series are not the same occurrence because they share a title.

**Governed by:** [Pomodoro schema](schema/pomodoro.md), [Calendar schema](schema/calendar.md), and invariant 5.

## 8. Pause boundaries

Manual and idle pauses freeze the remaining duration, and resume moves the phase deadline by the effective pause. A paused phase never completes because its former wall-clock deadline passed. A pause starting exactly at a phase boundary belongs to one phase according to committed transition order, never both. Repeated pause or resume commands are idempotent.

Idle backdating cannot precede the segment start or overlap a closed pause. Stopping while idle never converts idle time into focus. Suspend handling takes precedence over idle detection when the gap shows the operating system was asleep.

**Governed by:** [Idle detection](../algorithms/pomodoro/idle-detection.md), [Plan and history](../algorithms/pomodoro/plan-and-history.md), and invariant 5.

## 9. Reconfiguration chains

Reconfiguration compares elapsed active work with the new phase duration. If elapsed work already satisfies a shorter duration, the next boundary is due immediately; a longer duration extends only the remainder. Past segments never change. Repeated reconfiguration must not lose inherited focus or assign one segment to two rhythm positions.

**Governed by:** [Pomodoro state machine](../algorithms/pomodoro/state-machine.md), [Plan and history](../algorithms/pomodoro/plan-and-history.md), and invariant 3.

## 10. Notes placement and graph divergence

A page move updates navigation but not project membership, a folder change updates descendants in only one table, or history restore reintroduces a stale parent. Placement, project association, folder ancestry, links, and search projections change in one domain transaction, and cycle checks run against the resulting graph. Restore keeps the current placement unless it explicitly includes a validated move. Exports never repair or override the canonical graph; derived indexes are rebuilt after restore.

**Governed by:** [Notes and projects schema](schema/notes-and-projects.md) and invariant 8.

## 11. Working-folder identity drift

An external path can later point to a different directory through replacement, mount changes, symlinks, or path reuse, and Git's common directory can change independently of the working tree. Every privileged operation revalidates the device-local folder identity. Git, checkpoint, diff, and restore fail closed when only Git identity changed. A missing folder never falls back to a similarly named path.

**Governed by:** [Chat access control](access-control.md), [Chat security](security/chat.md), and invariants 9 and 10.

## 12. Conversation and provider-session conflation

A provider continuation may fail, be replaced, fork, or disappear while the channel remains valid. Provider cleanup never cascades into messages, approvals, assignments, or decisions. Conversely, archiving a channel does not stop a native provider process; shutdown and cleanup remain explicit bounded operations.

**Governed by:** [Chat schema](schema/chat.md) and invariant 11.

## 13. Permission leaks through derived data

Search, summaries, unread state, suggestions, exports, cached prompts, and sync envelopes can reveal restricted content even when direct reads are correct. Every derivative declares its authorization inputs and invalidates when membership, history boundary, profile revision, folder grant, or audience changes.

**Governed by:** [Chat access control](access-control.md) and invariant 12.

## 14. AI identity used as an authority bridge

Mentioning an agent, giving it a role label, or selecting a capable provider can suggest broad project access. None of these grants membership, history, folder, terminal, Git, or tool authority. Assignment review shows unresolved targets and denied capabilities instead of inferring them from instructions.

**Governed by:** [Chat access control](access-control.md) and invariant 13.

## 15. Restricted context retained by a continuation

After a reduction in authority, a provider continuation may still hold earlier restricted context that filtering new reads cannot remove. The revocation workflow interrupts, stops, and discards or quarantines it, and failed cleanup stays visible as retryable state without leaking native paths.

**Governed by:** [Chat access control](access-control.md#revocation-and-materialized-context) and invariant 15.

## 16. Destination audience expands around a reference

A message in a restricted channel is referenced from a broader channel whose participants overlap but are not a subset. Rendering the source title, excerpt, thumbnail, or summary would leak it, so the destination receives an opaque unavailable reference unless strict audience and history checks pass. Declassification creates a new destination-owned statement with provenance; it never relaxes the source.

**Governed by:** [Chat access control](access-control.md#strict-disclosure) and invariant 14.

## 17. Planned work mistaken for execution

A scheduled phone event or cached phase projection must not generate running state, focus history, or phase-dependent enforcement while a desktop is unavailable. Silence from a linked device will mean unavailable status, never inferred focus or idle.

**Governed by:** [Focus authority](../algorithms/pomodoro/focus-authority.md) and invariant 16.
