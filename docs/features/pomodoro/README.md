# Pomodoro

Pomodoro turns a calendar commitment into an adaptive sequence of focus and recovery phases. It protects attention without rewarding exhaustion or treating one fixed interval as universally correct.

**Status: Partial.** The native owner in `ganbaru-pomodoro` handles admission, execution, recovery, and adaptive decisions on desktop and Android, and Svelte only displays accepted state and sends commands. Physical platform acceptance and linked-device control remain open.

## Current scope

| Capability | Status |
| --- | --- |
| Manual and Calendar-linked runs | Implemented |
| Focus, short-break, long-break, pause, resume, stop, and recovery | Implemented |
| Presets and custom count or sequence rhythms | Implemented |
| Adaptive focus rhythm | Implemented, tuning ongoing |
| Idle and suspend detection | Implemented on desktop |
| Break screen, title-bar ring, tray ring, and Calendar rail | Implemented; desktop acceptance pending |
| Android accepted-phase notification and Guardian publication | Implemented; device acceptance pending |
| Native notifications and deadline reminders | Implemented; delivery acceptance pending |
| Linked-device controller and companion status | Planned |

## Philosophy

- A focus interval is a commitment opportunity, not a test of willpower.
- Breaks are required recovery, not a failure to continue working.
- The current calendar event is the hard scheduling boundary.
- Pauses, idle time, suspend, and interruption remain visible in history.
- Defaults adapt to observed local behavior without hiding the current plan.
- Metrics help the user tune work and recovery; they do not measure personal worth.

## Rhythm model

A count rhythm defines focus duration, short-break duration, long-break duration, and which focus position receives the long break. A sequence rhythm defines a bounded repeating list of focus and break steps. Presets (`creative`, `balanced`, `deep`, `extended`, and `adaptive`) use the same explicit fields as custom rhythms. Configuration is validated in full before it is accepted; a malformed configuration is rejected without changing the active one.

Adaptive mode proposes the next rhythm from local completed history and current context, within a fixed safe range and the active event deadline. The visible plan always states the current opportunity and next transition.

Detailed logic lives in [Pomodoro algorithms](../../algorithms/pomodoro/README.md).

## Run lifecycle

A run starts explicitly from a currently due Pomodoro-enabled Calendar event ("Start scheduled session"), or automatically on desktop when the event is due and fresh local input activity is observed. Android never starts a run from a Calendar alarm. Each run records one configuration snapshot so later preference edits do not rewrite active or historical runs.

UI countdowns render the accepted phase. A missed event or an unanswered break return never creates a later focus interval. See [Focus authority and evidence](../../algorithms/pomodoro/focus-authority.md).

Only one run is active at a time. Starting another requires an explicit stop or an adjacent-event handoff.

## Calendar boundary

An event-linked run cannot plan work beyond the event end. When less time remains than a normal phase, the opportunity clips to the deadline. Event deletion, archive, recurrence edits, and time changes follow Calendar active-session protection. Historical runs keep their original occurrence identity even after the live event is archived or materialized.

## Pause, interruption, and extension

Manual pause freezes the active phase and records why. Idle and suspend pauses remain distinct because they represent different evidence. Stopping records an interrupted or completed terminal state. Undoing a Calendar deletion never restarts a stopped run or removes history.

Each focus phase can be extended once, from the ending warning shown one minute before its deadline. Breaks can be extended from the break screen. Extensions never cross the event end.

## Breaks

Break transitions open a full break screen on desktop and a notification on Android. When a break ends, the app waits for the user to return; overtime is explicit and never silently converts into focus. See [Break screen](break-screen.md) and [Idle and suspend](idle-and-suspend.md).

## Progress surfaces

The title-bar ring, tray icon, Android ongoing notification, and Calendar rail share one semantic plan but present different detail. They never use contradictory progress definitions. See [Progress displays](progress-displays.md).

## Notifications

Native notifications identify the current transition and offer only actions the platform and state can perform truthfully. Rust owns scheduling; Svelte supplies bounded localized text.

- Desktop focus ending warnings fire once per accepted deadline. Linux notification actions carry the displayed run, phase, and vault identity and offer extension only while it is still available.
- Manual-pause reminders repeat at the configured interval (3, 5, 10, or 15 minutes, or off) until dismissed, resumed, stopped, or the event ends. Implementation gap: the native owner currently repeats them every minute and does not read this setting.
- Delayed wakes coalesce rather than replaying missed alerts.
- Android schedules commitment and accepted-phase deadline reminders natively while the Activity is absent. A reminder never creates execution.
- Delivery failures and permission denial never change execution history or prevent timer use. Exact-alarm denial on Android uses a less precise fallback and explains the consequence.

## Cross-feature behavior

- Calendar owns event timing and active-event identity.
- Music can apply phase soundtrack assignments and optional manual-pause ownership.
- The distraction blocker can apply phase rule snapshots.
- Projects provide event and rhythm defaults.
- Work environments may later add context defaults without replacing run state (planned).
