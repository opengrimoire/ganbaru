# Pomodoro plan and history

A Pomodoro run stores facts about what occurred. It does not pre-create future focus and break phases. The future plan is derived from the run snapshot, inherited state, the current event boundary, and recorded adaptive decisions. Segments are written only when a phase starts.

This separation means stop, reconfiguration, Calendar edits, and recovery never have to delete speculative rows or rewrite history.

## Durable roles

**Run.** The session header. It snapshots event and recurrence identity, planned event window, actual boundaries, rhythm, idle setting, title, start and end reason, heartbeat, and inherited state. One event may have several runs. A cross-block transition or a reconfiguration creates a new run instead of moving older segments.

**Segment.** A focus, short break, or long break that actually started. It records phase, rhythm position, planned and actual boundaries, chosen duration, and status. An active segment is the canonical current phase. A completed segment reached its accepted boundary. An interrupted segment ended for stop, event expiry, focus failure, reconfiguration, crash recovery, or another recorded reason. Planned and skipped bands in the UI are projections, never rows.

**Pause.** An interval inside one segment, with a reason and a start, and a null end only while open. A segment may contain several non-overlapping pauses. Pause time is excluded from progress. Resume closes the pause and moves the phase deadline; a pause never closes the segment by itself.

**Run event.** Append-only audit evidence for lifecycle decisions such as skipped break, extension, reconfiguration, transition, stop, completion, idle detection, focus failure, and recovery. It is not a second state machine. Adaptive choices use dedicated decision and outcome records.

## Future plan derivation

Projection starts from the run start, rhythm snapshot, normalized inherited position, inherited focus, event end, and persisted segments and pauses.

Count rhythms alternate focus and break, choosing a long break after the configured focus cadence. Sequence rhythms follow their bounded repeating steps.

Projection consumes persisted history first and never draws a second band over an existing segment. It then derives only the unpersisted future, clipped at the event end and the display window.

Once an adaptive boundary decision selects a value, the decision and the new segment's planned timestamps are persisted together. Later projection uses the recorded value instead of recomputing a possibly different policy result.

## Inherited progress

Adjacent or overlapping eligible Calendar blocks may continue one work rhythm while each block gets its own run. The outgoing run supplies accumulated non-paused focus, normalized rhythm position, any compatible active break, and its own identity for audit. The new run stores these values directly rather than deriving them later from older runs.

If inherited focus meets the incoming focus duration, the incoming run begins with the appropriate break. Otherwise it continues focus with the incoming duration minus inherited focus. A real gap resets inheritance.

## Phase start transaction

Starting a phase commits together:

1. Closure of the outgoing active segment, with its terminal reason and actual boundary.
2. Any run event explaining the transition.
3. Any adaptive boundary decision.
4. The new active segment and its planned deadline.
5. Execution state and the command receipt.

No reader can observe two active segments or an adaptive decision without the phase it selected.

## Paused time

Progress is elapsed instant time inside the segment minus pauses. Wall-clock labels are never used for duration arithmetic.

An idle pause may be backdated to the inferred start of input inactivity, clamped to the segment start and prior pause evidence. Suspend is handled before idle so one away interval is not subtracted twice. Stopping while paused closes the pause and segment at the selected boundary without converting paused time into focus.

## Reconfiguration

Changing configuration during a phase preserves elapsed non-paused progress:

new remaining = max(0, new phase duration minus elapsed progress)

- Increasing duration extends only the amount still required under the new duration.
- Decreasing duration shortens remaining time.
- Decreasing below elapsed progress makes the boundary immediately due.

The current run closes as `reconfigured` and a new run with the new snapshot carries the elapsed progress as inherited state. Completed and interrupted segments are unchanged.

## Event boundary

The Calendar block end is a hard cap. Projection clips at it. At expiry, the active segment closes as interrupted unless it completed at the same instant. No break segment is written because a focus was planned to finish after the event end; an adjacent eligible block may inherit that focus in its own run.

## Plan versus actual

Analytics compares the plan implied by the run snapshot and recorded decisions with the segments that actually started, their pause-adjusted durations, skips, extensions, focus failures, stops, block expiry, and later adaptive outcomes. Never reconstruct an old plan from the event's current configuration.

## Worked examples

### Ordinary count rhythm

Configuration is 25 minute focus, 5 minute short break, and 15 minute long break every four focuses. A fresh run starts at 09:00.

| Time | Durable result |
| --- | --- |
| 09:00 | Run and active focus segment at position 1 are created. |
| 09:25 | Focus segment completes. Short-break segment at position 1 starts. |
| 09:30 | Short break completes. The run waits for return and creates no focus interval. |
| 09:32 | User accepts the return. Focus at position 2 starts at the actual acceptance time. |

Positions 3 and 4 remain derived until those phases start.

### Idle pause and resume

A 25 minute focus starts at 10:00 with a five minute idle threshold.

| Time | Durable result |
| --- | --- |
| 10:15 | User input stops. Nothing is written yet. |
| 10:20 | A sample reaches the threshold. An idle pause is created, backdated to 10:15. Fifteen focus minutes have elapsed and ten remain. |
| 10:30 | User resumes. The pause closes after 15 minutes and the deadline moves to 10:40. |
| 10:40 | Focus completes with 25 active minutes and 15 paused minutes. |

The segment spans 40 wall-clock minutes but records 25 minutes of focus.

### Shorter reconfiguration

A 40 minute focus starts at 09:00. At 09:28 the user changes the rhythm to 25 minute focus. Elapsed progress already exceeds the new duration, so the focus boundary is immediately due: the run closes as reconfigured and a new run starts with the break selected by the new rhythm. It does not continue for the old 12 remaining minutes.

### Longer reconfiguration

A 25 minute focus starts at 11:00. At 11:10 it changes to 40 minutes. Ten active minutes carry over, so 30 remain and the new deadline is 11:40, subject to the event end.

### Event truncation

An event ends at 15:00. A 40 minute focus starts at 14:30. At 15:00 the event boundary interrupts the segment after 30 active minutes. No break segment is created. An adjacent eligible event may inherit those 30 minutes in its own run.

### Focus failure

Idle is detected and backdated to 13:15. The visible overlay stays unanswered for 60 seconds. The focus segment is interrupted at 13:15 with a focus-failed reason. If the user returns while the event is still active, a fresh focus segment starts with the full configured duration rather than reopening the failed one.

## Recovery

Recovery never materializes a missed future plan. It closes or resumes the one active phase, then projection derives what remains. See [Focus authority](focus-authority.md#recovery).
