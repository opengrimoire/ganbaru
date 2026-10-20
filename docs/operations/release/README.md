# Release process

Ganbaru releases are built from explicit `app-v*` tags and staged as draft GitHub Releases. Publishing the draft makes the release public and then updates the Linux package repositories and AUR package.

The release workflow builds Linux x64 packages, Windows x64 installers, a signed Android universal APK, and a signed Android AAB. It signs desktop updater assets, produces checksums and `latest.json`, and keeps all signing or publishing jobs behind the protected `release` environment. Builds run in unprivileged jobs without secrets, and the signing job only verifies, signs, and uploads the built artifacts (see [Signing](signing.md) and [Supply chain](../../data/security/supply-chain.md)). This split takes effect with the next `app-v*` tag and has not yet run end to end.

## Authority and safety

- Normal work enters `dev` through topic-branch pull requests.
- A release is promoted through a pull request from `dev` to `main`.
- The `main` merge queue validates the merge result without merging `main` back into `dev`.
- A merge to `main` does not publish anything.
- Only an authorized organization administrator creates an `app-v*` tag, approves the `release` environment, and publishes the draft GitHub Release.
- Release jobs use fixed runner images, SHA-pinned actions, read-only default permissions, and no dependency caches.
- Signing, repository publication, and AUR credentials are scoped to the jobs that require them.

The intended enforcement is defined in [Repository policy](../repository-policy.md). Current GitHub settings must be checked against that document before a public release.

## Release targets

| Target | Artifact or channel |
| --- | --- |
| Linux x64 | `.deb`, `.rpm`, and `.AppImage` |
| Windows x64 | Tauri `.msi` and setup executable |
| Android 10 or newer | Signed universal APK and AAB with ARM64, ARMv7, x86, and x86_64 libraries |
| macOS and iOS | Not part of the current release workflow |

Linux builds use Ubuntu 22.04 to preserve broader glibc compatibility. Windows builds use the pinned Windows runner. Android release artifacts are separate from the desktop updater feed.

## Required repository setup

Before releasing:

1. Create the protected GitHub environment named `release`.
2. Configure its required reviewer and restrict it to `main` and `app-v*` deployment refs.
3. Add the signing secrets listed in [Signing](signing.md).
4. Add the updater and package-repository public-key variables.
5. Enable GitHub Pages from the `gh-pages` branch root.
6. Verify branch, tag, Actions, workflow, and environment protection against [Repository policy](../repository-policy.md).

## Version preparation

Update the version in all three files through a normal pull request into `dev`:

- `apps/client/package.json`
- `apps/client/src-tauri/Cargo.toml`
- `apps/client/src-tauri/tauri.conf.json`

Check whether release notes, migrations, generated artifacts, signing expectations, or platform compatibility statements also changed.

## Publishing checklist

1. Run `pnpm -w run validate:full` on the release candidate.
2. Open a release pull request from `dev` into `main`.
3. Review it and add it to the `main` merge queue after pull request checks pass.
4. Wait for the merge queue to validate and merge the release result.
5. Create `app-v<version>` at the release commit and push the tag.
6. Approve the protected `release` environment.
7. Wait for the release workflow to create and populate the draft GitHub Release.
8. Download and smoke-test the desktop installers and signed Android APK.
9. On Android, verify that production and `.dev` identities coexist and that the installed production build restarts offline.
10. Inspect generated release notes, `SHA256SUMS` and `SHA256SUMS.asc`, signatures, and `latest.json`. Do not edit or replace assets on the draft after the signing job; publication jobs reject any asset that no longer matches the signed `SHA256SUMS`.
11. Publish the draft GitHub Release.
12. Verify the package-repository and AUR publication jobs.
13. Verify the GitHub Pages apt and RPM metadata and the released AUR version.

Do not publish when required checks, signatures, expected artifacts, environment protection, or update metadata are missing.

## Release trigger

The release build has no manual dispatch entry. A pushed, protected `app-v*` tag is the only build trigger because it binds the workflow run directly to the reviewed release commit. Recovery must preserve that tag and commit identity rather than creating a draft from an untagged branch run. Do not move or reuse a tag after a pre-publication workflow failure. Correct the release source through the normal pull request path and use the next application version and tag.

## Release notes

The workflow prepends `.github/release-notes-template.md` and appends GitHub-generated notes categorized by `.github/release.yml`.

Use concise pull request titles because they become release entries. Labels control categories. Use `skip-changelog` or `ignore-for-release` only when a pull request should be omitted.

## After publication

Publishing triggers the package repository and AUR jobs. The signed draft artifacts exist before publication, but apt, RPM, and AUR channels must not update from an unpublished draft.

Distribution details and recovery boundaries are in [Distribution](distribution.md).
