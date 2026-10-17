# Calendar recurrence editing

**Status: Implemented.** Native preview, reviewed Save, creation, and Project bulk scheduling share one Rust boundary on desktop and Android; installed-app acceptance remains pending.

Recurrence preview is a non-mutating projection of what Save would produce. It shows the visible calendar as it will look if the current draft and scope are saved, but never detaches, splits, deletes, archives, transfers sessions, or writes data before Save. The scope semantics themselves (`Only this`, `Following`, `All`, protection) are defined in [Recurrence model](recurrence.md).

## Product contract

- Preview and Save use the same native plan over the same prepared rows.
- Switching scope re-projects one shared draft; it does not reset explicit edits.
- Missing fields and explicitly cleared fields are distinct.
- The selected occurrence stays visible unless the operation truly removes it.
- Save commits the Calendar mutation and Pomodoro reference changes atomically.
- After Save, the visible window refreshes from canonical persisted expansion. A preview or cache never becomes a second source of truth.
- Preview contours refer only to rendered events and always clear on close or completion. A visually absent block never keeps an invisible hit area.

## Ownership

The frontend owns the draft, immediate pointer feedback, and rendering. While native review runs, the selected card moves immediately over cached occurrences; this overlay makes no recurrence or protection decisions.

Rust owns expansion, scope and protection decisions, draft validation, the visible preview, and the atomic write. It accepts no frontend clock or execution evidence: acceptance time, the device date, and Focus history are read natively. Typed civil times carry their display zone separately from the event's home zone, and Rust resolves them without changing the home zone.

Why: protection decisions depend on recorded Focus history and the current time. Letting the frontend decide would allow a stale view or skewed clock to rewrite history.

## Field operations

Nullable and structurally meaningful fields use explicit operations:

- **Unchanged:** the user left the baseline value.
- **Set:** the user supplied a new value.
- **Cleared:** the user explicitly removed the value.

For recurrence, a missing property never means repeat was turned off. Turning repeat off and back on resolves from the final value compared with the baseline. Equivalent rules normalize to unchanged, and equivalent metadata does not cause a split.

## Scope selector

The selector appears only when the event belonged to a saved recurring series when the session opened, for both the template and generated occurrences. It is hidden for a standalone event (including one gaining recurrence) and for the selected active occurrence, where the effective scope is `Only this`.

## Preview

Changes that affect recurrence meaning request a debounced native preview tagged with the edit generation; a stale result cannot replace a newer draft. The preview projects only the affected series and leaves other cached events untouched. Preview and committed results keep stable identities, and the selected occurrence is tracked by identity rather than geometry, since two overrides can share a displayed time.

Delete and archive previews use the same affected-set model: protected rows can keep their geometry while a contour explains that they will be archived.

## Save

Save revalidates against current data even if a preview was reviewed. The review is bound to the source rows, protection evidence, normalized intent, and scope; if any changed, Save is rejected rather than reinterpreted. One transaction applies template updates, exceptions, detachment, splitting, materialization, archive operations, active-run reference transfers, Focus reconciliation, and the retry receipt.

Detach and split copy the complete event: alarms, attendees, categories, extended properties, organizer, task links, Focus configuration, Music assignments, and an independent copy of imported iCalendar data. Mutations have explicit size limits (10,000 rows and 16 MiB) and fail rather than truncate.

## Retry and lost replies

Every Save carries a retry identity bound to the complete request. A retry returns the recorded result without re-executing; reusing the identity with different intent fails. If the outcome is uncertain (lost reply, uncertain commit), the request stays unresolved and the UI offers Retry with the exact original request, even across panel close and Calendar remounts. Only an authorized receipt lookup can conclude that a request was never accepted; this lookup also works after the vault becomes read-only. A failed visible-window refresh after a confirmed Save does not repeat the write.

## Creation

Panel creation uses the same preview, Save, and receipt recovery. Rust validates the draft, canonicalizes recurrence, and assigns the event identity. The frontend shows one immediate card while review is pending; its temporary ID is presentation-only and never authorizes a write.

## Active Pomodoro sessions

A continuing active run keeps its identity through recurrence edits. When its occurrence receives a new event identity, current references transfer atomically while the original run identity, timestamps, and segment facts stay unchanged. Removing Focus configuration stops the owned run in the same transaction.

`End now` applies to one ongoing timed occurrence: it keeps the recorded start, ends at the native acceptance time, and completes the run, including an open pause, without starting a later phase. `Enable Focus` applies to an ongoing standalone event without Focus configuration and starts a run only when no other run is active.

An earlier closed run also protects an ongoing occurrence: its start and date kind cannot change, its end cannot move before the current time, and its repeat chain cannot change.

## Opening other events during preview

Opening another card during preview resolves its baseline from persisted occurrences and re-verifies the source after details load. A proposed-only card cannot open as a saved source, and discarding a preview never carries unsaved values into another editor.

## Quality requirements

- Planning and projection stay deterministic.
- Protection decisions consider the complete affected range, not only visible dates.
- Recurrence that cannot be enumerated safely stops with a diagnostic rather than rewriting history.
- Preview and Save have parity tests, and transactions prove rollback across Calendar and Pomodoro changes.
- Required user scenarios live in [Calendar recurrence testing](../../testing/calendar-recurrence.md).
