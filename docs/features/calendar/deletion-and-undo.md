# Calendar deletion and undo

**Status: Implemented.** Reviewed native deletion, archive, and short-lived Undo are implemented for desktop and Android; installed-app acceptance remains pending.

Ganbaru AI has no general Calendar undo and redo stack. Creates, edits, moves, resizes, and recurrence changes commit directly. Delete and archive are the only Calendar actions with an Undo opportunity.

## Delete versus archive

Future untracked events may be hard deleted. Events that started, have timer history, are active, or own another durable reference (task links, imported preservation state) are archived instead. One scoped recurrence operation can combine archive of protected occurrences with removal of mutable future expansion. The action label reflects the native review: Delete, Archive, or Remove for mixed or still-unreviewed selections, and the confirmation explains the effective result.

Scope uses original recurrence identities even when an override moves the displayed occurrence:

- Starting from an already started occurrence is history-only: scoped started occurrences archive while later mutable occurrences stay live.
- A future `All` selection keeps already-started occurrences, removes mutable future members, and archives protected future members.
- A future `Following` selection keeps the original prefix and archives protected members on the removed side.
- Selecting the active occurrence forces `Only this`. A wider operation that affects another active occurrence names the exact run that must stop, and the user must explicitly confirm stopping it.

Stopping a run happens in the same transaction as the archive, recurrence changes, history reference updates, and retry receipt. Rust recaptures the reviewed source before writing; if the selection changed since review, the command is rejected rather than reinterpreted, and no run is stopped.

Archives preserve the complete event: metadata, task-link kinds and timestamps, Focus configuration, Music assignments with their versions, and an independent copy of imported iCalendar data. Original execution identities and timestamps never change. Removing a calendar keeps archives of its protected events (under the built-in local calendar's storage custody) with their original calendar identity. Restoring an archive validates that its calendar, recurrence, and linked tasks still admit it, and fails transactionally otherwise. Storage details live in the [Calendar schema](../../data/schema/calendar.md#deletion-and-repair).

## Pending and completed state

When deletion begins, the event panel closes and a toast reports the pending operation. The calendar applies the plan's final visible result at once, so a scoped operation does not remove occurrences one by one.

After persistence succeeds, the toast reports whether events were deleted, archived, or both and offers `Undo` for five seconds, plus a dismiss control. Only the latest completed operation can be undone; starting another deletion finalizes the previous one.

## Undo scope

Undo restores the snapshot and recurrence structure captured by the operation:

- A standalone delete restores the event.
- `Only this` restores the prior exception and occurrence state.
- `Following` restores the prior recurrence boundary and any detached survivor.
- `All` restores the prior historical and mutable series state.
- Archive undo restores archived records to their active representation.

If Undo expires, is dismissed, or the app closes, the operation stays committed. Archived history remains recoverable through archive surfaces; a hard-deleted future event has no indefinite recovery.

## Undo guarantees

- The undo snapshot lives only in native process memory, is bound to the current vault and process, and is never written to SQLite or accepted from the frontend. This avoids keeping an indefinite copy of data the user chose to delete.
- Expiry uses monotonic time, also capped by wall time, so sleep or clock rollback cannot extend the window. Retries and restarts cannot renew it.
- Undo checks that the deleted source has not been recreated or edited since; it rejects rather than overwrites later changes.
- Undo has its own retry receipt, so a retried Undo returns its recorded result instead of restoring twice. A lost deletion reply can still be undone within the window once its accepted receipt is confirmed.

## Pomodoro interaction

If deletion stopped an active run, Undo restores calendar data only. It does not restart the run, delete interruption history, or rewrite completed segments, because focus history is append-only.

## Failure behavior

A scoped plan applies atomically. A failure restores the canonical visible window and reports an error rather than leaving a partial split, archive, exception, or run-reference transfer.

See [Recurrence model](recurrence.md) and [data invariants](../../data/invariants.md).
