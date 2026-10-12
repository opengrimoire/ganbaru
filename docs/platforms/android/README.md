# Android

Android is Ganbaru AI's current mobile target. It reuses shared domain contracts while replacing desktop filesystem, process, media, notification, and background-work assumptions with narrow Android adapters.

## Delivery status

Android status has three independent axes:

1. **Implemented in source:** the shared and native code exists and passes its focused build or contract checks.
2. **Validated on reference devices:** the behavior has passed the documented physical or emulator cases.
3. **Release-ready:** signed, minified release artifacts pass the complete device, policy, permission, upgrade, and distribution matrix.

The current source is substantially implemented but the Android product is not yet release-ready.

## Capability summary

| Capability | Current Android state |
| --- | --- |
| App-private vault, first use, import, portable backup, and restore | Implemented in source |
| Calendar, Projects, Notes, Quick notes, Settings, Themes, and localization | Implemented through shared responsive surfaces |
| Explicit Pomodoro starts, commitment reminders, accepted-phase service and recovery | Implemented in source |
| Music library and selected-tree local audio through Media3 | Implemented in source |
| Provider-free Chat channels, messages, replies, search, drafts, and scheduling | Implemented through the shared responsive workspace |
| Local coding-agent processes, PTYs, Git, terminals, and working folders | Intentionally unavailable |
| Selected-app distraction usage limits and Home redirection | Implemented in source; policy and physical acceptance remain |
| Local multi-device linking, whole-vault ownership handoff, and read-only refresh | Implemented in source; physical multi-device acceptance remains |
| Browser filtering on Android | Planned |
| Concurrent sync, remote execution, diary, and sleep alarm | Planned |

## Product boundary

The first useful offline release includes the app-private vault, Calendar, Projects, Notes, provider-free Chat communication, Quick notes, Pomodoro, Music, Settings, localization, themes, import/export, and native deadline delivery. Sync and remote agent execution are not prerequisites for a useful local mobile app.

Android 10, API level 29, is the minimum supported version. The app uses one Activity and one WebView shell, with native services for work that must survive Activity or process lifecycle boundaries.

## Non-negotiable rules

- Canonical user data stays in the active vault.
- The default Android vault is application-private and can be removed by uninstall.
- Portable backup and restore are explicit user workflows.
- The app does not request broad storage, broad package visibility, overlay, screen capture, contacts, microphone, or location authority. Camera access is requested only from the user-initiated desktop-linking QR scanner.
- Desktop processes and capabilities are excluded at build time, not merely hidden in the UI.
- JavaScript timers do not own background deadlines.
- Permission denial leaves truthful recovery and useful unaffected features.
- Android Back unwinds consumable frontend state before yielding to the operating system.

## Documentation map

- [Architecture and capabilities](architecture-and-capabilities.md)
- [Mobile experience](experience.md)
- [Native services and data](native-services-and-data.md)
- [Permissions and security](permissions-and-security.md)
- [Validation and release](validation-and-release.md)
- [Android testing](../../testing/android.md)
