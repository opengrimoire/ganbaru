# Security policy

Ganbaru handles sensitive local data, including calendar entries, notes, diary content, work patterns, and optional AI conversations. Please do not report vulnerabilities through public GitHub issues.

## Reporting a vulnerability

Use GitHub private vulnerability reporting:

```text
https://github.com/opengrimoire/ganbaru/security/advisories/new
```

Include:

- A short summary of the issue.
- The affected version, commit, platform, or workflow.
- Reproduction steps or a proof of concept.
- The impact you believe the issue has.
- Any suggested mitigation or patch, if you have one.

Do not include real private user data, API keys, signing keys, passwords, or unredacted personal files in the report.

## What to report privately

Report these privately:

- Data exposure, data loss, or unauthorized access to the Ganbaru folder.
- Bypass of Tauri permissions, filesystem restrictions, updater checks, signing, or release protections.
- Dependency, build, CI, release, cache, or workflow behavior that could compromise published artifacts.
- Remote code execution, command execution, path traversal, injection, or privilege escalation.
- Extension behavior that exposes private browsing, blocker, or app data.
- Sync, backup, or AI-provider behavior that could leak private user data.

Normal bugs, usability issues, feature requests, and documentation problems should use public GitHub issues.

## Response process

The project maintainer will triage private reports, ask for clarification if needed, and keep fixes private until disclosure is safe. Public disclosure should wait until a fix or mitigation is available unless there is a clear user-safety reason to disclose earlier.
