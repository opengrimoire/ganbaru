# Platform documentation

Platform documents describe operating-system composition, capability differences, lifecycle, permissions, native services, and release validation. Feature behavior remains canonical in [feature documentation](../features/README.md).

## Android

[Android](android/README.md) is the current mobile target. Its source implementation includes the local offline shell, application-private vault, Calendar, Projects, Notes, provider-free Chat, Quick notes, Pomodoro, Music, Themes, localization, backup and restore, native notifications, and selected-app distraction blocking. Release readiness still requires the documented signed-artifact and device validation.

## Desktop

[Linux and Windows](desktop/README.md) are the current desktop targets. Desktop-specific documentation currently includes the tray. Desktop composition and native-service architecture live under [architecture](../architecture/README.md).

## Future Apple platforms

macOS and iOS remain planned. Shared contracts can be reused, but permissions, background execution, storage, media, notifications, credentials, and distribution require platform-native designs. Android or desktop behavior must not be copied into Apple documentation as if platform capabilities were identical.
