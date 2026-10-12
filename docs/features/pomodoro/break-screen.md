# Pomodoro break screen

The break screen makes recovery harder to ignore than a normal notification without pretending to take control of the operating system from its owner.

## Purpose

When a short or long break begins on desktop, the app presents a deliberate transition away from work. The surface shows the remaining break and only the controls needed to extend, deliberately end early, or return to focus.

The screen blocks ordinary window switching and accidental dismissal where supported. A user can still terminate the process, power off, use operating-system security surfaces, or otherwise bypass the app. Ganbaru AI is not hostile lock-in or device-management software.

## Surfaces and enforcement

The primary display uses a Svelte overlay so countdown, localization, controls, and accessibility share the application component system. Secondary displays receive quiet blocker surfaces; clicking them returns attention to the primary overlay.

A scoped Rust guard (`pomodoro/overlay/enforcement.rs`) owns topmost placement, monitor reconciliation, power assertions, supported app-switch shortcut suppression, and cleanup, with platform-specific adapters. Display changes reconcile coverage without duplicating controls. Cleanup is idempotent after partial setup, display changes, or exit. Overlays open and close only in response to committed native state; hiding a window never advances the timer.

## Deadline

The countdown derives from the persisted absolute phase deadline, so a slow WebView does not restart it. The break-start sound plays once the surface is visible so audio and visual transition coincide.

## Controls

- `Mod + Shift + Space` extends the break by one minute while the extension allowance remains. Settings allow 1, 3 (default), 5, 10, or 15 extensions, or none. Implementation gap: the native service currently caps total break extension at three minutes regardless of this setting.
- Repeated `Escape` presses end the break early. Settings require 1, 3, 10 (default), 20, or 50 presses, or disable early ending. The awkward default prevents reflexive dismissal while preserving a deliberate escape.
- The screen has no stop action. Stopping remains deliberate through the active event or main Pomodoro surface.
- Terminal completion screens accept one key or click to acknowledge and offer nothing else, because the run has already ended.

## Sounds

Settings control an optional warning before break end and the repeat cadence after it. A repeat setting of none still plays the immediate break-finished cue. A warning whose time has already passed does not fire late. Packaged sounds follow [audio asset rules](../../development/audio-assets.md).

## Overtime and return

When the countdown reaches zero, the screen shows "Ready to return," counts overtime, and repeats the completion sound at the configured cadence. Waiting for any duration never starts another focus interval; the user must accept the return, or the event end closes the session.

The persisted break segment ends at its planned end, and the Calendar rail shows at most ten seconds of grace beyond it. Later waiting is empty time, like pause and away time, so break history stays honest when the user returns much later.

## Suspend and wake

System suspend during a break creates a suspend pause. On wake, the break resumes from its remaining duration rather than counting sleep as break time, and the enforcement guard reconciles displays.

## Visibility

The break screen appears only for active short- and long-break phases and their return wait. It does not appear for idle pauses (which use the [idle overlay](idle-and-suspend.md)), suspend, manual focus pause, or reconfiguration that does not start a fresh break.

## Accessibility

Countdown and controls are readable at distance, localized, keyboard operable, and announced by state. Required actions do not depend on color or animation. Reduced-motion preferences apply to nonessential transitions, and secondary displays contain no unreachable duplicate controls.
