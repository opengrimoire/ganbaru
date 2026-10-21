# Release distribution

GitHub Releases is the artifact source. Desktop self-updates, Linux repositories, AUR, and direct Android installation use different delivery channels and must not be described as interchangeable.

## Desktop updater

Release builds generate an ignored `src-tauri/tauri.release.conf.json` that embeds the public updater key and points at the repository's latest-release `latest.json` asset.

The desktop build produces installers with the public updater configuration. A protected job signs updater artifacts and writes `latest.json` with tag-specific asset URLs. Android does not consume this feed.

Automatic checks run at most once per day by default and can be disabled. They never download or install without an explicit user action.

Rust owns the update check, download, and install; webviews have no updater plugin access and can only ask Rust to check or to install the update it already offered. Before installing, Rust requires the verified signature's trusted comment to name an asset file carrying the offered version (`_<version>_`), so a validly signed older asset cannot be served under a newer version in `latest.json`.

AppImage and Windows installations can use Tauri's signed update path. Package-manager installations show the appropriate update command instead of trying to install an AppImage. Ganbaru copies, but never executes, privileged apt, dnf, zypper, yay, or paru commands.

## Linux package repositories

Published releases update:

- APT: `https://opengrimoire.github.io/ganbaru/packages/apt`
- RPM: `https://opengrimoire.github.io/ganbaru/packages/rpm`

The published `.deb` can install a Deb822 source file and repository key. The published `.rpm` can install dnf or zypper repository configuration and its verification key, with `gpgcheck=1` so dnf and zypper require the package signature as well as the metadata signature. A first install enables the repository. Upgrades and reinstalls refresh the key and rewrite an existing repository file, but keep `Enabled: no` or `enabled=0` and never recreate a file the user removed. Package scripts remove configuration only on a full removal or purge.

The package-repository job runs only after the GitHub Release is published. It downloads the released packages with `SHA256SUMS` and `SHA256SUMS.asc`, refuses to continue unless the signature is valid for the package repository key and every downloaded file matches, then regenerates metadata, signs apt and RPM metadata, and updates the `gh-pages` branch. The pool is rebuilt only from files listed with the same digest in the signed `packages/pool-SHA256SUMS` from the previous publication plus the new release, so a file placed on `gh-pages` outside this job is removed instead of signed. When neither the pool manifest nor its signature exists, only the new release is published; when one is missing or the signature is invalid, the job stops for a maintainer to inspect the branch.

The signing job signs each `.rpm` with the package repository key before it writes `SHA256SUMS`, and fails unless `rpmkeys` accepts the signature with a keyring that holds only the committed package repository public key. RPMs published before package signing remain unsigned in the pool, so dnf and zypper refuse to downgrade to them.

The signed apt `Release` carries `Valid-Until` 90 days after signing, so a replayed old `Release` stops being accepted. Each release re-signs it, and the `package repo refresh` workflow re-signs it from the verified pool on the first day of each month; it runs in the `release` environment, so a maintainer approves each run, and two missed months still leave clients a valid `Release`. The refresh stops instead of pruning when the pool holds a file the signed manifest does not list. RPM metadata has no signed expiry, so on dnf and zypper the in-app update notice is the signal that the repository has stopped advancing.

## AUR

`ganbaru-bin` repackages the released `.deb` and removes Debian-specific repository files. A dedicated post-publication job verifies the `.deb` against the signed `SHA256SUMS` before it updates the AUR package.

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
