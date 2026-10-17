# Time conflict detection

**Status: Implemented.** The native Focus scheduler and the Calendar timeline rail apply the same owner selection. Selecting a scheduled commitment does not authorize execution. Linked-device control and conflict suspension during replication are planned.

## Interval model

Eligible timed events have a Pomodoro configuration, a finite valid range, and are neither all-day nor cancelled. The interval includes its start and excludes its end: at exactly 10:00, an event ending at 10:00 is no longer eligible and an event starting at 10:00 becomes eligible.

## Owner selection

At the current instant:

1. Keep the current executing owner if it remains eligible.
2. Otherwise choose the eligible event with the earliest end.
3. Break equal-end ties by creation identity, then occurrence ID, using locale-independent string ordering.

A missing creation identity sorts as the empty string. Rhythm settings, containing another event, and recent interruption confer no priority. Candidate input order does not change the result.

The scheduler wakes at event boundaries and on Calendar or lifecycle changes. Desktop automatic admission also requires fresh local activity; Android only schedules reminders and requires an explicit start. See [Focus authority and evidence](../pomodoro/focus-authority.md).

## Timeline projection

The rail keeps recorded history for every event, including interrupted or older overlapping runs. Historical evidence is never hidden because another event now owns that window.

For the future proposal, the rail uses the same selector, keeps the selected owner until it ends, and then selects another eligible event. At most one proposed rhythm exists for a given instant. A gap clears inherited rhythm. A containing event can take over the remaining window after a nested owner ends. Planned bands are proposals and never create recorded focus or breaks.

An active phase projects from its recorded start, remaining duration, pauses, and current configuration. An untracked commitment whose start has passed begins its proposed rhythm at the current instant; it does not fill the missed interval as completed work.

## Examples

Event A spans 09:00 to 12:00. Event B spans 10:00 to 11:00.

- At 10:15 with no active run, both scheduler and rail select B because it ends first.
- If A already owns an accepted run when B begins, both keep A while it is eligible.
- If B owns the run, its recorded evidence stays visible despite containment by A.
- At B's end, the remaining A window becomes eligible. A new executing interval still requires admission.
- In a proposal made before 09:00, A is selected first and keeps its proposed window when B begins. This is a forecast, not evidence that A executed.

For equal windows, creation identity and occurrence ID decide; a shorter focus duration does not affect ownership.

## Event changes and limits

Moving, resizing, archiving, deleting, or changing eligibility invalidates selection, and the native Focus runtime reconciles or closes the affected run in the same transaction as the Calendar change. A real gap starts a fresh run; inheritance across a small configurable gap is not implemented. Cross-device schedule conflicts will suspend automatic activation once replication exists.
