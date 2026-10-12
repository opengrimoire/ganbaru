# Repository policy

**Status: Reference.** This is the normative GitHub policy for `opengrimoire/ganbaru-ai`: its rulesets, adjacent Actions settings, and protected release environment. Rulesets for a public repository are visible at <https://github.com/opengrimoire/ganbaru-ai/rules> and are not secrets.

Last verified against live GitHub settings: 2026-09-12, with no drift found.

If live settings differ, treat the difference as configuration drift. Fix the live settings or change this policy through review; never silently rewrite the policy to match weaker enforcement.

Rulesets are the enforcement layer for branch and tag movement. Workflows must not enforce branch routing, especially through `pull_request_target`, unless a separate security review accepts that privileged automation surface.

## Roles

- **Organization admin** is the only bypass actor, for release tags and the protected `release` environment. Today this is the organization owner. Do not use Write, Maintain, Repository Admin, or Deploy keys for release bypass without changing this policy.
- **Project maintainer** has write or maintain access for normal work. Today this is the same person as the organization admin.

GitHub lets anyone with write access merge a pull request once the target branch requirements are met. While the project has one maintainer, limited write access is the strongest practical control. Before granting write access to anyone else, revisit this policy and raise required approvals, stale-approval dismissal, Code Owners, and most-recent-push approval on both branches.

## Branch rulesets

`protect main` and `protect dev` are active branch rulesets with an empty bypass list, each targeting only its own branch.

| Rule | `main` | `dev` |
| --- | --- | --- |
| Restrict deletions, block force pushes | On | On |
| Require signed commits | On | On |
| Require a pull request | On, 0 approvals | On, 0 approvals |
| Require approval for unattributed changes | On | On |
| Require conversation resolution | On | On |
| Allowed merge methods | Merge only | Merge only |
| Required status checks (GitHub Actions source) | `linux validation`, `windows Rust check`, `Android ARM64 build` | Same |
| Do not require status checks on creation | On | On |
| Require branches to be up to date | Off | On |
| Require merge queue | On: merge method, build concurrency 1, group size 1, no wait, all entries must pass, 60 minute check timeout | Off |
| Linear history, deployments, code scanning, code quality, Copilot review, commit metadata, branch name restrictions | Off | Off |

Rationale:

- `dev` is the integration branch; all normal work enters it through visible pull requests. `main` is the audited release staging boundary, not the daily integration branch, and a merge to `main` publishes nothing by itself.
- Merge commits preserve the original signed topic-branch commits, authorship, timestamps, and topology, keeping the repository itself as the audit trail. Linear history is off for that reason, and on `main` it also keeps pull request history for generated release notes.
- Signed commits are required on `dev` too, so unsigned history cannot accumulate and later block promotion into `main`.
- `main` uses a merge queue instead of up-to-date branches. The queue validates the merge result ([GitHub documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)) without forcing `dev` to absorb main-only release merge commits. `dev` traffic is low, so it requires up-to-date branches instead of a queue.
- Deployment gates are off because the `release` environment belongs to the tag-based release workflow, not to branch merges.
- Code scanning and code quality gates stay off until they are configured, stable, and documented. Pull request title conventions carry release-note quality better than a commit metadata regex.

## Release tag ruleset

`protect release tags` is an active tag ruleset targeting `app-v*`, with `Organization admin` as an always-allowed bypass actor. It restricts creation, update, and deletion, blocks force pushes, and requires signed commits. Linear history, deployments, status checks, commit metadata, and tag name restrictions are off.

Rationale:

- `app-v*` tags trigger signed desktop and Android release builds. Blocking only force pushes is not enough, because anyone with write access could still create a new matching tag.
- Tags point at signed release commits on `main`, so signed-commit enforcement adds a release identity check on the tag target.
- Release tags and the protected `release` environment are separate controls, and both must exist. The release workflow carries its own build, signing, draft publishing, and environment gates.

## Required adjacent settings

Repository access:

- Keep write, maintain, and admin access limited. External contributors use forks or topic branches and pull requests.

GitHub Actions:

- Default workflow token permissions are read-only.
- Actions may not create or approve pull requests unless a specific reviewed workflow requires it.
- Third-party actions use selected actions and full SHA pins.
- Release jobs are cache-free.
- Required checks run on `merge_group` as well as `pull_request`, otherwise the merge queue cannot satisfy the ruleset.
- The `check` workflow does not run on pushes to `dev` or `main`; those branches are pull-request-only, so push checks would duplicate the checks that already gated the merge.
- No `pull_request_target` workflow for branch routing or contributor messaging. If one is ever needed, it must not check out pull request code, install dependencies, use caches, run build scripts, or read untrusted files.

Protected `release` environment:

- Required reviewer: the organization owner, or a dedicated release team if release authority is delegated later.
- Deployment refs restricted to `main` and `app-v*`.
- Holds the updater, Android, package repository, and AUR secrets listed in [release signing](release/signing.md). Signing secrets are never exposed to pull request, test, or unsigned build jobs.

Secret values cannot be read back through GitHub, so their presence does not replace the signed-artifact and installation checks required for each release.

## Verification

Verify with read-only API calls from an authenticated maintainer session:

```sh
gh api repos/opengrimoire/ganbaru-ai/rulesets
gh api repos/opengrimoire/ganbaru-ai/environments/release
gh api repos/opengrimoire/ganbaru-ai/environments/release/secrets
gh api repos/opengrimoire/ganbaru-ai/actions/permissions
gh api repos/opengrimoire/ganbaru-ai/actions/permissions/workflow
```

Inspect each ruleset detail endpoint returned by the first command and compare targets, bypass actors, pull request rules, merge queue, status checks, deletion and force-push protection, signed commits, and tag restrictions. Also compare workflow triggers and job environments in `.github/workflows/`. Changing live policy is a deliberate maintainer action reviewed alongside this file.

Review this policy before adding maintainers, after GitHub ruleset feature changes that affect these options, and after any supply-chain incident that changes the threat model.
