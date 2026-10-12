# Desktop tray

The tray is Ganbaru AI's operating-system glance and control surface when the main window is hidden, minimized, or behind other windows. It answers whether a focus phase is active or paused, how much focus opportunity remains before the next transition, and what media is active. It is not a statistics, cycle-history, or productivity-score surface.

## Pomodoro ring

The tray ring uses the same remaining-focus metric as the app chrome ring. The event deadline clips the opportunity when it arrives before the configured focus duration.

| State | Presentation |
| --- | --- |
| No focus session | Subdued empty ring |
| Focus start | Full remaining-time ring |
| Focus running | Remaining arc shrinks toward empty |
| Manual focus pause | Remaining arc uses a restrained slow pulse |
| Focus complete or stopped | Subdued empty ring |

A full active ring means the opportunity has just begun, while an empty subdued ring means no active focus. The pause pulse stops immediately when the pause ends for any reason. Icon updates stay bounded and never turn the tray into a high-frame-rate animation surface.

## Menu

The Pomodoro section shows status and the applicable actions: pause or resume, extend focus once when the event has room, move to break during focus, and start focus during a break.

The Music section shows the active title or an unloaded state and provides play or pause, previous, next, open Music, and inspect the current Calendar or project soundtrack assignment when one applies.

Unavailable controls stay visible but disabled so menu positions remain predictable. Clicking the tray icon opens or focuses the main window, within platform focus-stealing rules.

## Platform notes

- **Linux:** tooltips are unreliable, so the menu is the accessible status surface. Dynamic icons go through Tauri's safe tray API, which owns the AppIndicator and its generated image files (platform cache, not vault data). Upstream currently deletes the previous image before publishing the next, so some desktops can briefly show a placeholder. Ganbaru AI must not work around this by casting AppIndicator's shared reference to a mutable one; a prewritten-image optimization can return only when upstream exposes a sound update operation. Tauri upgrades require explicit Linux tray verification.
- **Windows:** the standard Tauri tray icon and tooltip apply.
- **macOS (planned):** the menu-bar presentation and standard icon path, unless platform validation requires a specialized adapter.

## Accessibility

Menu text describes ring and media state without relying on icon color. Actions keep stable labels and enabled state, and no critical operation requires interpreting the pause animation.

See [Pomodoro progress displays](../../features/pomodoro/progress-displays.md), [Music](../../features/music/README.md), and [native backend architecture](../../architecture/native-backend.md).
