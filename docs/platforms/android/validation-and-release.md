# Android validation and release

Android is release-ready only when a signed, minified release artifact passes the build, device, lifecycle, permission, policy, upgrade, and data-recovery gates below. Source implementation or a debug install alone is not release readiness.

## Build baseline

- Tauri `~2.11.1`.
- Minimum API level 29 (Android 10), compile and target API level 36.
- Gradle wrapper 8.14.3 and NDK 30.0.15729638.
- ARM64, ARMv7, x86, and x86_64 release libraries.

Manifests and build files are authoritative. Update this summary when the baseline changes instead of keeping version history.

## Artifacts and signing

Pull request CI builds a bounded ARM64 debug APK for feedback. The release workflow builds the signed universal APK and AAB for all supported ABIs. Signing credentials, keystore handling, and identity continuity are documented in [release signing](../../operations/release/signing.md); debug keys never sign production artifacts, and debug builds use the separate `.dev` application identifier.

Generated Android project changes are reviewed as source. Regeneration is deliberate and followed by a diff, so permission, provider, service, signing, backup-rule, and manifest changes cannot disappear unnoticed.

Packaged native libraries must load on devices with 16 KiB pages. A successful Rust target compile is not sufficient evidence.

## Release gates

The release candidate must pass:

- Clean install and supported upgrade paths.
- First use, app-private vault creation, folder import, backup, restore, and reinstall recovery.
- Offline launch and behavior for the promised local feature set.
- Process death, Activity recreation, reboot, package replacement, clock, and timezone recovery.
- Notification, alarm, foreground-service, Music, and anti-distraction special-access grant, denial, revocation, and recovery.
- Compact, wide, portrait, landscape, keyboard, cutout, gesture navigation, and system Back behavior.
- Minified WebView bridge and capability operation.
- No desktop-only providers, binaries, capabilities, or assets in the artifact.
- Store-policy review for Accessibility and other declared special access.
- Privacy, backup, data-safety, license, and distribution disclosures that match actual behavior.

## Current validation state

Reference-device checks cover the Android 10 shell, lifecycle, layout, navigation, and core feature flows. Still required before delivery: predictive Back, broader OEM background management, complete special-access revocation, all supported ABIs, physical cross-process Pomodoro and distraction blocker acceptance, multi-device handoff, signed minified artifacts, and the full release matrix. The detailed scenario matrix lives in [Android testing](../../testing/android.md).

## Distribution and updates

The distribution source owns installation and updates; Android does not package the desktop self-updater. Direct GitHub installations check the latest published release through the GitHub API. When it contains the expected versioned universal APK, the update surface opens that asset, and the system package installer accepts the upgrade only when package identifier and signing certificate match. The release page is the fallback when the APK is absent or the response is unusable.

Release promotion follows the [release process](../../operations/release/README.md).
