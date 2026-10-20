# Release signing

Release credentials belong in the protected `release` GitHub environment. They must never be exposed to pull request builds, ordinary test jobs, development APKs, logs, artifacts, or source control.

## Environment secrets

| Secret | Purpose |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Tauri desktop updater signatures |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Optional updater key password |
| `ANDROID_KEYSTORE_BASE64` | Complete Android release keystore encoded as canonical base64 |
| `ANDROID_KEYSTORE_PASSWORD` | Android keystore password |
| `ANDROID_KEY_ALIAS` | Release key alias |
| `ANDROID_KEY_PASSWORD` | Release key password |
| `GANBARU_PACKAGE_REPO_GPG_PRIVATE_KEY` | ASCII-armored key that signs RPM packages, apt and RPM metadata, the package pool manifest, and release `SHA256SUMS` |
| `GANBARU_PACKAGE_REPO_GPG_PASSPHRASE` | Package repository key passphrase |
| `AUR_SSH_PRIVATE_KEY` | Dedicated AUR publishing key |

Repository variables hold public material:

- `TAURI_UPDATER_PUBLIC_KEY`
- `GANBARU_PACKAGE_REPO_PUBLIC_KEY`

## Tauri updater key

Generate the updater key pair on a trusted maintainer machine:

```sh
pnpm -C apps/client tauri signer generate -w ~/.tauri/ganbaru.key
```

Back up the private key and password in a durable protected store. Losing the key prevents existing users from accepting later updates through the current updater identity. Rotating it requires an explicit user migration path.

This key signs updater artifacts. It is not Windows Authenticode signing, so Windows reputation warnings remain a separate concern.

## Android signing identity

The package identifier and signing certificate together form the permanent direct-install identity. Later APKs update existing installations only when both match.

Generate the production keystore once on a trusted machine. The command is intentionally interactive so secrets do not enter shell history:

```sh
mkdir -p ~/.config/ganbaru
keytool -genkeypair -v -keystore ~/.config/ganbaru/android-release.jks -storetype PKCS12 -keyalg RSA -keysize 4096 -validity 10000 -alias ganbaru
```

Back up the keystore and passwords in two access-controlled locations before distributing the first production APK. Losing a self-managed signing key prevents normal updates to direct installations.

For a local signed build, create the ignored `apps/client/src-tauri/gen/android/keystore.properties`:

```properties
storeFile=/absolute/path/to/android-release.jks
storePassword=replace-with-keystore-password
keyAlias=ganbaru
keyPassword=replace-with-key-password
```

Then run:

```sh
pnpm --dir apps/client tauri android build --ci
```

Debug builds remain independent and use the `.dev` application identifier.

For GitHub Actions, encode the keystore without line wrapping:

```sh
base64 -w 0 ~/.config/ganbaru/android-release.jks
```

CI never builds with the keystore present. An unprivileged job builds minified unsigned universal artifacts with `GANBARU_ANDROID_UNSIGNED_RELEASE=true`, which lets Gradle produce a release build without `keystore.properties`. The protected job installs only Java and the Android build tools, decodes the keystore into a temporary runner directory, aligns and signs the APK with `apksigner`, signs the AAB with `jarsigner`, verifies both, and removes the temporary directory. Passwords reach the tools only through environment variables. Android release certificates are normally self-signed. Strict AAB verification therefore uses the same protected keystore as its explicit trust anchor and requires the configured signing alias; it does not rely on the public Java certificate-authority store.

If Google Play is introduced, decide the Play App Signing and upload-key strategy before the first store release. Preserve certificate compatibility when direct and store installations are expected to share an update identity.

## Package repository key

The apt and RPM repositories require a stable OpenPGP key. Store the armored private key and passphrase in the release environment and the armored public key as a repository variable.

Losing or rotating this key requires package-manager users to trust the replacement before later repository metadata can be verified. Rotation also requires removing `packages/pool-SHA256SUMS` and its signature from `gh-pages`, because the next publication refuses a pool manifest the new key did not sign; that publication then carries forward only the new release.

## AUR key

Use a dedicated SSH key with access only to `ssh://aur@aur.archlinux.org/ganbaru-bin.git`. Do not reuse a personal general-purpose SSH key. Rotate it immediately if exposed.
