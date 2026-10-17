# Release distribution

GitHub Releases is the artifact source. Desktop self-updates, Linux repositories, AUR, and direct Android installation use different delivery channels and must not be described as interchangeable.

## Desktop updater

Release builds generate an ignored `src-tauri/tauri.release.conf.json` that embeds the public updater key and points at the repository's latest-release `latest.json` asset.

The desktop build produces installers with the public updater configuration. A protected job signs updater artifacts and writes `latest.json` with tag-specific asset URLs. Android does not consume this feed.

Automatic checks run at most once per day by default and can be disabled. They never download or install without an explicit user action.

AppImage and Windows installations can use Tauri's signed update path. Package-manager installations show the appropriate update command instead of trying to install an AppImage. Ganbaru copies, but never executes, privileged apt, dnf, zypper, yay, or paru commands.

## Linux package repositories

Published releases update:

- APT: `https://opengrimoire.github.io/ganbaru/packages/apt`
- RPM: `https://opengrimoire.github.io/ganbaru/packages/rpm`

The published `.deb` can install a Deb822 source file and repository key. The published `.rpm` can install dnf or zypper repository configuration and its verification key. Package scripts remove configuration only on a full removal or purge, not during normal upgrades.

The package-repository job runs only after the GitHub Release is published. It downloads the released packages, regenerates metadata, signs apt and RPM metadata, and updates the `gh-pages` branch.

## AUR

`ganbaru-bin` repackages the released `.deb` and removes Debian-specific repository files. A dedicated post-publication job updates the AUR package.

The app can suggest `yay -Syu ganbaru-bin` or `paru -Syu ganbaru-bin` based on the detected helper. It does not run the command.

## Android

The release workflow builds a signed universal APK for direct installation and a signed AAB for future store submission. Direct APK updates require the same package identifier and signing certificate. A future store channel follows that store's signing and update policy.

Android release acceptance must verify signature, application identity, coexistence with the `.dev` build, offline startup, data behavior, and the supported ABI set.

## Recovery boundaries

- Losing the Tauri private key breaks trust for the desktop updater until users install through a separately trusted path.
- Losing the Android app-signing key prevents updates to direct installations with that identity.
- Losing the package-repository key requires users to trust a replacement key.
- Losing the AUR key affects publication access but does not alter installed package identity.
- A published release tag must never be moved. Correct a bad release with a new version and a documented recovery path.

The release process and inspection checklist are in [Release process](README.md). Credential setup is in [Signing](signing.md).
