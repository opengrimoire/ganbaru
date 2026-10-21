# Android permissions and security

Android requests access in context, only after the user enables a feature that needs it. Denial, revocation, or unavailable system support leaves a truthful state and direct recovery action.

## Permission plan

| Access | Purpose | Denied behavior |
| --- | --- | --- |
| Notification runtime permission | Calendar reminders, Pomodoro state, Music media, and selected-app explanations on Android 13 or newer | Feature remains usable without delivery; settings explain how to enable notifications later. |
| Exact alarm special access | Precise Calendar focus reminders and documented deadline cases on Android 12 or newer | Use the documented inexact idle-aware fallback and explain that delivery can be late. |
| Foreground service declarations | Active Pomodoro and Media3 playback | Service-backed background behavior is unavailable when the corresponding feature cannot start truthfully. |
| Usage Access | Count explicitly selected packages and reconcile selected-app limits | No Android usage counting or limit enforcement; configuration remains readable. |
| Accessibility Service | Observe selected package transitions and perform Home for matching rules | No selected-app blocking; usage counting may remain if Usage Access is granted. |
| Document tree and file grants | Import, restore, selected Music tree, and managed files | Only the selected action fails; no broad storage fallback is requested. |
| Camera runtime permission | Scan a desktop pairing QR code while the user is on the linking screen | Linking remains available later from Data settings; no image or video is stored. |

## Special-access disclosure

Usage Access and Accessibility have separate localized disclosures immediately before opening system settings. Each states what is observed, what is not observed, the action performed, and that access can be revoked.

Returning from settings refreshes actual platform state. Manufacturer autostart settings have no reliable enabled query, so the app records only that the user reviewed the page and never displays a false granted state.

## Accessibility boundary

The anti-distraction service requests only the event stream needed to identify package window changes. It does not retrieve view content, text, gestures, keystrokes, notifications, screen images, or arbitrary browsing data.

Enforcement uses the standard Home global action for a fresh explicitly selected package. It does not use overlays, gesture injection, screen capture, or device-owner authority.

This capability requires store-policy review and the appropriate Play declaration before release.

## Package visibility

The app uses targeted launchable-activity queries needed for the on-demand selected-app picker. It does not request `QUERY_ALL_PACKAGES`.

Unknown packages, system UIDs, Ganbaru, Home, Settings, dialer, and protected apps are rejected.

## Storage authority

The app does not request `MANAGE_EXTERNAL_STORAGE` or legacy broad shared-storage access. Live vault data stays application-private. System pickers grant only user-selected inputs and destinations.

Persisted document permissions are retained only for sources that require ongoing access, such as the selected Music tree and chosen artwork. Native Music reads a direct document URI only when it is itself a persisted read grant or lies inside a persisted tree grant, so a supplied locator cannot reach other providers the app can read implicitly, including its own.

## Other excluded permissions

Current features do not request contacts, microphone, location, screen capture, system overlay, device-owner status, or VPN consent. The camera permission is requested only when the QR scanner is shown; preview frames are processed in memory and the stream stops once a code is decoded or the scanner closes. New authority enters the manifest only with a spec, localized disclosure, denial behavior, tests, and policy review.

## WebView and bridge

The mobile capability grants only commands required by shared features and Android adapters. It does not expose a generic filesystem, process, shell, network, or event capability. Document picker and download plugin commands are native-only: Rust derives the app-private paths they read or write, and the webview cannot call them.

Untrusted file and native responses are validated before becoming typed frontend state. External URL opening is limited to supported safe schemes and explicit user action. Production content policy does not allow arbitrary remote script or image authority.

The app WebView denies every frame source in production and development CSP, so no remote or loopback page loads inside the WebView that holds the Tauri bridge. YouTube playback and link resolution run in separate native WebViews owned by the mobile media plugin. Those views have no JavaScript interface or document-start scripts, deny file, content, geolocation, pop-up, and permission access, and load only the Rust-built player page from an app-asset origin. They reach the app only through a web message listener restricted to that origin and the main frame, and Rust validates every command before it reaches the page. Android does not start the loopback Music server, and the playlist builder's inline YouTube embed is not available on Android.

Release builds restrict Tauri's generated document-start scripts to the `tauri.localhost` origins through a Gradle task that fails the build if the generated source changes shape. Debug builds keep every origin so the development server works; frame denial keeps those scripts out of embedded content there. The app WebView grants a permission request only when it asks for camera capture alone from the app origin (or the development server origin in debug builds), which is what device-link QR scanning needs; every other resource and origin is denied, and geolocation is always denied. A Gradle task fails the build if the generated chrome client starts handling callbacks the permission wrapper does not forward.

## Native component exposure

Services, receivers, providers, and activities are non-exported unless an operating-system integration requires exposure. Internal broadcasts use an app-signature permission. Notification taps reach the app through non-exported route activities that record the tap target before opening the launcher, and the exported launcher ignores routing extras, so other apps cannot steer navigation. Document picker results are validated before they are read: only `content` URIs served by another app are opened, never file paths or the app's own providers. The provider chooser for text imports stays available.

## Secrets

Android currently does not run local provider processes. The device linking TLS key, the person key, and sync writer keys are stored today as plain files in application-private storage, outside the vault and the frontend configuration. Android cloud backup and device transfer exclude every application data domain, so these files leave the device only through root access or a compromised operating system. Wrapping them with an Android Keystore key is planned and is the target boundary for every Android credential, including future remote execution credentials (see [network and privacy](../../data/security/network-and-privacy.md)).

## Failure behavior

Permission loss never widens access or falls back to a broader permission. Native projections expire or fail safe, selected-app blocking stops when authority or state is stale, and canonical data remains available to unaffected local features.
