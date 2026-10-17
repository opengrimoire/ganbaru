# Pomodoro state machine

The native Focus owner applies one observation or command at a time to the accepted execution state, inside one SQLite transaction, then publishes effects. Svelte sends intents and paints committed projections. See [Focus authority](focus-authority.md) for what may authorize execution.

## Why decisions are explicit

Time and lifecycle code breaks most easily at exact boundaries. Decisions take a complete snapshot and the current time, so tests can construct state directly and assert behavior at event end, phase end, warnings, suspend return, reconfiguration, and overlapping Calendar transitions without a wall-clock wait.

The state machine does not choose which overlapping event owns a run. Calendar selection happens first under [Time conflict detection](../calendar/time-conflict-detection.md).

## Modes

| Mode | Meaning |
| --- | --- |
| Running | A focus or break segment is active and counting down |
| Manual pause | The user paused the active segment |
| Idle pause | Desktop input inactivity reached the threshold during focus |
| Idle failed | The idle grace elapsed; the focus segment is interrupted |
| Suspended | A clock discontinuity paused the active segment |
| Return wait | A phase ended and the next focus needs user acceptance |
| Stopped or expired | The run ended by user action, recovery, or the event end |

## Observation priority

For each observation the owner applies, in order:

1. Startup recovery, when requested, before anything else.
2. Suspend evidence, recorded before expiry so the away interval is excluded from any later closure.
3. Calendar block expiry at or after the event end.
4. Calendar reconciliation (end change or reconfiguration of the same block).
5. The observation itself: automatic admission, activity, idle visibility or grace, deadline, foreground change, or heartbeat.

Block expiry precedes phase completion on the same observation. Work cannot continue outside the owning event merely because a phase also reached zero.

## Suspend detection

The native owner compares wall-clock and monotonic time between its own wakes; no frontend timer participates. A monotonic gap above 15 seconds, any backward wall-clock step, or a forward wall-clock step above 15 seconds is treated as a discontinuity. A running segment receives a closed `suspend` pause covering the gap (clipped to the event end) and waits in suspended mode for the user to resume or stop. Idle detection is skipped while suspended so one away interval never produces both pauses. The threshold is lifecycle policy, not database meaning.

## Phase advancement

When a focus phase reaches its deadline:

- If skip-next-break is set, the owner records a skipped break and starts the next focus position.
- Otherwise it starts the short or long break selected by the rhythm for that focus position.
- On Android while the app is in the background, focus completion instead enters return wait, because no surface can show the break.

When a break reaches its deadline, the segment completes and the run enters return wait. Another focus interval requires acceptance. Count rhythms use their long-break cadence; sequence rhythms use their normalized bounded positions.

Explicit "go to break now" and "start focus now" close the active segment at the command time and record the reason.

The outgoing segment closes before the incoming one starts, and both commit with any run event, adaptive decision, and receipt in one transaction.

## Calendar block activation

When the Calendar reports an eligible block, the owner distinguishes:

- No change to the current block.
- The run's block is no longer current (deleted, archived, or moved away), which stops the run as interrupted.
- Only the current block's end changing, which updates the hard deadline.
- Same-block reconfiguration.
- Transition from an adjacent or overlapping block.
- A fresh session.

Return wait and idle failure are protected from rhythm rebuilds; an end change still updates their deadline.

## Cross-block transition

When an expired run's focus continues into an adjacent eligible block, accumulated non-paused focus is compared with the incoming focus duration at the same position:

- If it meets or exceeds the new duration, the new run begins with the appropriate break.
- Otherwise focus continues with the new duration minus accumulated focus.

A real gap starts fresh. Inherited amounts are stored on the new run so later projection never traverses a chain of older runs.

## Reconfiguration

Reconfiguration preserves elapsed active progress, not the old remaining time.

1. Derive elapsed non-paused progress in the current phase, including inherited progress.
2. Determine the new phase duration at the normalized position.
3. If elapsed already meets a new focus duration, the next break starts. If it meets a new break duration, the break completes and the run enters return wait.
4. Otherwise the current run closes with reason `reconfigured` and a new run starts with the new configuration, carrying the elapsed progress as inherited state. A pause in progress is carried over.

Rationale: each run keeps one immutable rhythm snapshot, so a change creates a new run rather than mutating the old one. Past segments are never changed.

## Pause and extension

Manual pause and resume are allowed only in compatible running state. Idle and suspend pauses own their own resolution flow and cannot be bypassed by the generic pause command. Resume closes the pause and shifts the phase deadline by the paused duration; a paused phase never finishes because its former deadline passed.

A focus phase accepts one extension of up to three minutes. Breaks accept extensions up to a total cap. Neither may cross the event end.

## Side-effect ordering

1. Revalidate the vault generation, command ID, and expected revision.
2. Compute the decision.
3. Persist all canonical rows and the receipt in one transaction.
4. Update in-memory state from the committed result.
5. Publish notifications, overlays, media, tray, anti-distraction, and window projections.

If an effect fails after commit, state remains canonical and delivery retries. User history is never rolled back because a tray or sound effect failed.

## Heartbeats and recovery

An open run writes a heartbeat every 15 seconds. It is not a sample of focus activity. Desktop recovery closes an open run at its last heartbeat; without it, recovery would have to choose the planned phase end or startup time, both of which can greatly overcount. Android recovery resumes or closes the committed phase. Desktop and Android recovery must not be collapsed into one formula. Details are in [Focus authority](focus-authority.md#recovery).
