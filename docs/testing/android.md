# Android testing

**Status: Reference.** Android testing distinguishes source checks, emulator coverage, physical-device behavior, and signed release acceptance. Platform contracts live in [Android](../platforms/android/README.md).

## Required targets

| Target | Coverage |
| --- | --- |
| Physical Android 10 ARM64 phone | Minimum-version behavior, real storage providers, notifications, audio, background execution, process death, reboot, and system navigation |
| API 29 compact emulator | Install, rotation, Activity recreation, insets, keyboard, Back, denied permissions, offline startup, and deterministic core flows |
| API 33 emulator or device | Notification runtime permission grant, denial, repeated denial, and Settings recovery |
| API 34 or newer emulator or device | Foreground-service policy, exact-alarm state, predictive Back, current WebView, and modern background restrictions |
| Wide or tablet configuration | Rail navigation, responsive overlays, multi-pane limits, rotation, and keyboard behavior |
| Supported ABI release artifacts | Installation, startup, native-library loading, and 16 KiB page-size compatibility where applicable |

## Core offline flows

- Fresh install and `Start from zero`.
- Import a complete vault through a document tree.
- Create a portable backup, uninstall, reinstall, and restore.
- Cold and warm offline startup: the readiness indicator hands off to a usable Calendar, and other destinations open without another app-wide loading surface.
- Calendar creation, editing, recurrence, reminders, and project linkage.
- Project task views, detail, settings, scheduling, and wide-view touch panning.
- Notes pages, blocks, databases, history, assets, Archive, and Trash.
- Provider-free Chat channels, messages, replies, search, drafts, and scheduling.
- Quick notes lifecycle and conflict-safe persistence.
- Themes, localization, profile assets, Settings, and vault identity.

## Navigation and layout

- Compact and wide primary navigation.
- Portrait and landscape safe areas, cutouts, and system-bar contrast.
- Three-button and gesture navigation.
- Keyboard open, resize, rotate, dismiss, and focused-field recovery.
- Android Back through dialogs, sheets, builders, hierarchy navigation, editors, task detail, and destination roots.
- Predictive Back on a supported newer target.
- Split-screen and Activity recreation.

## Lifecycle and data

- Activity background and removal without corrupting drafts.
- Process death with and without final lifecycle callbacks.
- Vault switch, failed activation, and rollback.
- SQLite pool closure during restore.
- Low-storage and interrupted import or restore.
- Reboot, package replacement, clock change, and timezone change reconciliation.
- Missing or revoked document-tree permission.
- Managed asset selection with oversized, mislabeled, malformed, and missing files.

## Calendar and Pomodoro

The rule under test: Calendar alarms are reminders, and only committed native execution creates runs or focus history.

- Notification permission grant, denial, revocation, and app-settings recovery.
- Exact-alarm granted and denied fallback.
- Calendar test notification and channel behavior.
- Event edits cancel and rebuild native deliveries.
- A due commitment while the Activity is absent or the PC is off produces a reminder and no run or focus history.
- A late explicit start records its actual start, not the Calendar boundary.
- Phase deadlines, missed break returns, and repeated recovery create no extra focus phases. A late alarm for a closed phase never replaces a newer phase's ongoing notification.
- A delayed Guardian phase publication is rejected after the run stops or vault ownership changes, including after process death and in a minified release build.
- Changing notification language never supplies execution state or recovers a run from notification storage.
- Foreground-service start, ongoing notification, pause, resume, stop, process recreation, and event deadline.
- Tap routing validates identifiers and opens the correct context.
- Manufacturer battery or autostart guidance never claims an unobservable enabled state.

## Music

The native owner holds the queue and decoder; the WebView is only a projection.

- Select, retain, revoke, and reselect a Music document tree. Scanning stays off the main thread and within bounds.
- Media3 playback while backgrounded, with audio focus, notification, lock-screen, headset, and trusted controller actions. Controllers can play, pause, seek, and skip but cannot replace items, change repeat or shuffle, or release the player.
- Queue, playlist, review, assignment, resume, missing item, and offline subset behavior.
- Suspend the WebView while audio advances; on return the UI shows the current queue and position without replaying old effects or starting a second decoder.
- A hidden browser source resumes paused after its host lease expires; only explicit Play restarts it.
- During slow loads or a blocked document provider, pause, seek, stop, change items, switch vaults, change Focus phase, and destroy the service. The newest intent wins, old callbacks are ignored, a stopped decoder never reloads, and resolver work stays bounded.
- After service loss, the selected track and position are retained and explicit Play resumes the same track.
- A vault handoff waits for playback to actually stop or reports a timeout; the old vault's canceled playback never resumes.
- Desktop audio, soundscapes, path reveal, and loopback media capabilities are absent.

## Distraction blocker

- Separate Usage Access and Accessibility disclosures.
- Grant, deny, revoke, and recover each access independently.
- Selected launchable-app filtering and protected package rejection.
- Fresh phase and exhausted-limit Home redirection.
- Paused phase, stale projection, vault switch, and rule removal stop enforcement.
- Activity removal, Guardian process recreation, reboot, package replacement, and low-memory recovery.
- Daily and weekly rollover across local midnight and timezone changes.
- Journal import accepts only current-vault rows and does not duplicate acknowledgements.
- Notification deep links open the relevant settings.
- Store-policy declaration matches observed behavior.

Runtime cases without the WebView are in [Distraction blocker runtime acceptance](distractions.md).

## Vault handoff pairing

- Grant and deny camera access from the user-initiated QR pairing scanner.
- Camera capture stops after a code is decoded, the scanner is canceled, or the pairing surface closes.
- Malformed, expired, replayed, and wrong-vault invitations cannot enroll the phone.
- Pairing and later authenticated reconnects work on a private LAN without internet access.
- Run the complete [local vault handoff acceptance](vault-handoff.md).

## Security and artifact checks

- No broad storage, package, overlay, capture, contact, microphone, or location permission. Camera access is limited to user-initiated QR pairing.
- Native components have intended exported state and internal broadcasts require signature authority.
- Production content policy and WebView bridge reject unapproved remote or generic commands.
- Minification preserves required command serialization without widening capabilities.
- Android artifact excludes desktop provider processes, PTYs, Git, Rodio, tray, benchmarks, native messaging, and desktop blocker code.
- Backup and device transfer exclude the unencrypted live vault.

## Release acceptance

Run the complete matrix on the signed minified candidate. Record artifact identity, package version, signing certificate identity, target, device or emulator image, WebView version where relevant, result, and known exceptions.

A debug success or source compile does not satisfy release acceptance.
