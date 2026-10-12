# Pomodoro progress displays

Pomodoro progress appears in app chrome, the desktop tray, an Android ongoing notification, and the Calendar timeline rail. They share canonical run and segment state but answer different immediate questions.

## Shared principle

Glance surfaces show time until the next meaningful transition, not total productivity, cycle statistics, or a reward score. The user should be able to decide whether to finish the current thought or prepare for a break without reading a dashboard.

## App chrome ring

The title-bar or mobile top-bar ring appears during active focus, including manual pause. Its arc is the remaining focus opportunity, clipped by the Calendar event end: full at focus start, shrinking toward empty. Manual pause uses a restrained pulse; if the event end approaches during pause, the arc reflects that hard boundary.

The ring is hidden during breaks and when no focus is active. It does not show cycle number, total focus, future breaks, or daily statistics. Its menu exposes applicable Pomodoro actions and compact Music controls.

## Desktop tray ring

The tray mirrors the app chrome ring's focus-only metric and pause state, and stays visible when the main window is hidden. It adds no other progress definition. Rendering and menu behavior are in [Desktop tray](../../platforms/desktop/tray.md).

## Android ongoing notification

Android shows an ongoing notification during active focus, break, and manual pause, with a system countdown to the deadline and a progress indicator. It names the event when available, otherwise the phase. Paused state is explicit.

A foreground service and the native accepted phase own background continuity. Notification visibility is not the correctness boundary, because newer Android versions allow individual dismissal. Phase-completion alerts use a separate channel. See [Android native services](../../platforms/android/native-services-and-data.md).

## Calendar rail

Day, work-cycle, and week views show a narrow time-aligned rail beside Pomodoro-enabled events. Month view omits it because its cells cannot show this detail legibly and it is primarily for planning. Days without a Pomodoro-enabled event have no rail.

| State | Meaning |
| --- | --- |
| Focus fill | Persisted completed, interrupted, or active focus time, excluding pauses and never extending beyond now |
| Break marker | Planned future breaks and official active or completed break time, including extensions and ten seconds of end grace |
| Empty | Future focus, absence, pause, suspend, stopped gaps, unstarted breaks, and overtime beyond grace |

Theme tokens own the colors, with no extra shade for active, projected, or historical variants. Event and session details explain the rail's meaning without relying on color.

## Rail rules

- Focus fill runs from a segment's actual start to its end (or now while active), minus every pause. One segment with several pauses shows several bands. Future focus is never painted as done.
- Persisted breaks show only the interval that actually ran, capped at planned end plus ten seconds. Extensions move the planned end and therefore count.
- Projected breaks come from the current run plan. When an event still has time but no active run, projections restart from now to show that recovery remains reachable. Projections are never persisted, and a break that never starts leaves no historical marker.
- Historical fill renders from the original occurrence identity, so recurrence edits never move past focus onto a newly edited schedule.

## Visibility matrix

| Surface | Shown | Hidden |
| --- | --- | --- |
| App chrome ring | Active focus, including manual pause while opportunity remains | Break, no run, or expired event |
| Desktop tray ring | Same as app chrome ring, regardless of main-window visibility | Break, no run, or expired event |
| Android ongoing notification | Active focus, break, or manual pause | No active run or completed deadline |
| Calendar rail | Day, work-cycle, or week with a Pomodoro-enabled event | Month or a day without such events |

## Accessibility and motion

Progress surfaces expose text equivalents for remaining time, phase, and pause state. Pulse is never the sole paused indicator and respects reduced motion where the platform permits.
