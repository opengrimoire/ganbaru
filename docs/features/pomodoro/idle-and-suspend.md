# Pomodoro idle and suspend behavior

Idle detection prevents away time from being recorded as focus. Suspend detection handles a sleeping operating system separately because it carries different evidence and recovery expectations.

**Status: Partial.** Desktop idle detection, the idle overlay, focus failure, and suspend recovery run through the native owner. Real window visibility and platform acceptance remain pending. Android has no input-idle source and does not run idle detection.

## Idle threshold

Idle detection applies only to focus phases. An event can disable it or store a threshold initialized from global or project defaults. Settings offer 1, 2, 3, 4, 5, 10, or 15 minutes, with 3 as the default.

Changing the global threshold during an active run can update an already enabled detector. It never enables detection for an event that explicitly disabled it. Break phases do not pause for idle because time away is their purpose.

Platform sources, sampling, and timing rules are specified in [Idle detection](../../algorithms/pomodoro/idle-detection.md).

## Idle versus suspend

**Idle** means the system stayed awake but reported no user input through the threshold. The pause is backdated to the detected idle start and uses reason `idle`.

**Suspend** means the native owner's clock jumped, because the system slept or the process was stalled. The pause covers the gap and uses reason `suspend`.

The reasons remain distinct in history. Idle can reveal an interruption pattern; suspend usually reflects an external lifecycle event and should not be presented as failed attention.

## Idle pause

When idle is declared, the same focus segment stays active, a pause begins at the detected idle start, the countdown freezes, the Calendar rail stops focus fill at the pause start, and a full-screen idle overlay appears on the primary display with blockers on secondary displays. Backdating ensures the threshold time is never counted as focus.

## Return before focus failure

Pressing Space closes the idle pause and resumes with the same remaining focus time. The segment continues, and the rail shows separate focus bands before and after the pause.

## Long idle and focus failure

The overlay gives the user 60 seconds to return, counted from when it is actually visible on screen rather than from detection, so slow loading never consumes the grace period. If the time passes unacknowledged, the focus segment is interrupted with a focus-failed reason. Focus credit ends at the detected idle start.

Pressing Space after failure starts a fresh full focus opportunity in the same run and cycle, clipped by the event end. Cycle count and long-break cadence do not advance because one opportunity failed.

The idle overlay has no stop action. Intentional stop remains in the active event or main Pomodoro surface.

## Suspend recovery

On wake, a compact dialog explains the detected suspend duration and offers resume or stop. It is less forceful than the idle overlay because sleep is a known system event rather than inferred user behavior. Resume preserves the remaining time from before suspend. Stop records an interruption without counting suspended time as focus.

## Overlay enforcement

The idle overlay reuses the [break screen](break-screen.md) enforcement boundary and the same limits: it does not prevent process termination or operating-system security actions.

## Suppression

Idle detection does not fire when it is disabled for the event, no focus segment is running, another pause already owns the segment, the webcam appears to be in use, or the platform cannot provide trustworthy activity evidence. An unavailable source is reported explicitly and never fabricates activity or idle time.

## Accessibility

The overlay explains why the timer paused, how long the system believes the user was away, and what Space will do. Idle and failed-focus states use text and sound in addition to color. The repeating idle alert stops at the failure boundary so cues do not overlap.
