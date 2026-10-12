# Idle detection

Idle detection pauses a running desktop focus phase when operating-system input inactivity reaches the event's threshold. It never captures keystrokes, pointer content, camera frames, or application text.

The overlay and user choices are defined in [Idle and suspend](../../features/pomodoro/idle-and-suspend.md). This document defines activity sources, sampling, pause timing, focus failure, and the interaction with suspend.

## Platform sources

The native adapter (`pomodoro/idle.rs`) returns elapsed idle milliseconds when the platform source succeeds, an explicit unavailable value when it fails, and a best-effort webcam-in-use signal.

| Platform | Idle source | Webcam signal |
| --- | --- | --- |
| Linux | GNOME Mutter `IdleMonitor.GetIdletime` through `gdbus`, then `xprintidle` | Any other process holding an open `/dev/video*` file descriptor |
| Windows | `GetLastInputInfo` against the system tick count, handling one 32-bit wrap | Current user's camera capability-use registry state |
| macOS | `ioreg` `HIDIdleTime` for `IOHIDSystem` | Known camera helper processes running |
| Android and others | Unavailable | None |

Platforms without a source report unavailable. Mobile lifecycle recovery is separate and never infers idle from missing foreground activity.

## Webcam suppression

When the webcam appears to be in use, idle pausing is suppressed so a meeting or recording is not marked as failed focus just because keyboard and pointer input stopped. The signal is best effort: false negatives may still produce an idle pause, and false positives may delay one. No camera stream, frame, microphone data, window title, or meeting identity is collected or persisted, and the signal is not retained as adaptive history.

## Threshold contract

The run's configuration (or an explicit runtime override) supplies `idle_timeout_minutes`. Null disables detection. A configured value must be an integer from 1 to 120; zero is rejected rather than treated as disabled. The settings UI offers 1, 2, 3, 4, 5, 10, and 15 minutes.

Idle triggers when the reported idle duration is greater than or equal to the threshold, and only when all of these hold:

- The platform is desktop and the sample is fresh and valid.
- A run is open and in running mode (not paused, suspended, or waiting).
- The active segment is focus.
- The threshold is enabled.
- The webcam is not in use.

## Sampling

While a run is open, the native owner samples activity every 15 seconds. Because the pause is backdated to the inferred idle start, sampling delay never counts as focus; it only delays when the overlay appears. The interval is implementation policy, not stored meaning, and may change after measurement.

## Creating an idle pause

At threshold crossing, the idle start is the sample time minus the reported idle duration, clamped to the active segment and prior pause evidence. The owner persists an idle pause from that start, appends an `idle_detected` run event, and enters idle pause mode. The focus segment remains active while the pause is recoverable.

## Focus failure

The 60-second grace starts only when the idle overlay reports that it was painted and visible. The overlay acknowledges one specific run, segment, and detection time within the current vault generation, and Rust records the first accepted acknowledgement using its own clock.

- Duplicate or stale acknowledgements cannot extend the grace or acknowledge a newer warning.
- A failed acknowledgement write is rejected and may be retried; slow or failed presentation leaves the pause recoverable, and the event end still applies.
- Grace is measured with a monotonic clock, so civil clock corrections cannot shorten or extend it.
- Recovery and vault resume discard visibility markers; a restarted owner must show and acknowledge its own warning.

When the grace elapses, the focus segment is interrupted at the idle start with reason `focus_failed`. Returning afterward starts a new focus segment with the configured duration, capped by the event end. The failed segment is never reopened. The 60-second value is user-visible policy; changing it requires updating feature copy and tests.

Rationale: counting from detection would let a slow or hidden overlay fail the user's focus before they ever saw a warning.

## Return before failure

Resuming closes the idle pause at the return time and shifts the phase deadline by the pause duration. Stopping closes the segment and run without counting the idle interval as focus. Repeated resume or stop commands are idempotent.

## Interaction with suspend

A clock discontinuity means the system slept or the owner stalled, not that the user stopped typing. Suspend handling runs first and idle detection is skipped while suspended, so one away interval never produces two pauses. Startup recovery never queries current idle time to reclassify an offline interval as idle.

## Unavailable sources

A failed platform query produces an explicit unavailable sample. The owner does not treat it as activity, does not create or backdate an idle pause, and exposes the unavailable state in the accepted snapshot. A user-visible degraded-state indicator is planned.
