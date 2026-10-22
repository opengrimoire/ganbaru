# Anti-distraction native application enforcement

Desktop and Android enforce selected applications through different operating-system boundaries. They share user intent and explanations but do not pretend their permissions or observations are equivalent.

## Shared safety rules

- Only explicitly selected applications can be targets.
- Selection records a platform identity plus a user-facing label: the package name on Android, and on desktop the application name, whose process names come from the installed application catalog at enforcement time rather than from the vault.
- Protected applications are rejected at selection and enforcement time.
- Stale, disabled, removed, mismatched, or unauthorized rules perform no destructive action.
- Enforcement is rate-limited and explains how to change the responsible setting.
- Permission denial disables the affected adapter without disabling unrelated Ganbaru features.

## Desktop selection and matching

Application discovery is on demand. The picker excludes system and basic utility targets that are unsafe or unrelated to procrastination. Adding or removing an enforced app requires confirmation.

A desktop rule stores its application name and normalized match names. The vault configuration is not trusted to choose process names: enforcement uses only the rule's own name and the process names the installed application catalog lists for that name, including entries the picker hides in the current desktop session. A typed rule therefore matches its own name, and a picked app also matches its desktop entry's executable. Generic runtimes, shell hosts, session services (systemd, PipeWire, agents), multiplexers, terminal emulators, and compositors are protected and never sufficient targets because closing them could affect unrelated work.

Residual risk: an edited vault configuration can still add a visible rule whose own name is an unprotected user application, so the rule list in Settings is the place to review what the blocker can close.

## Desktop phase enforcement

Process or foreground checks run only while a relevant phase rule is active. Process observation, open-app counting, and close revalidation consider only processes whose real user ID is the signed-in user's; other accounts' processes on a shared machine are never counted or closed. Ending the phase stops checks immediately, and late observations are ignored.

Linux is the only desktop with a close adapter today; other desktops refuse close requests. On Linux, enforcement binds the exact observation (process ID, start time, executable, and names) to the exact persisted rule in the active vault. Immediately before a graceful close and before any stronger fallback, the backend revalidates process identity, start identity, active vault, fresh phase or limit state, target rule, and protected-app policy.

A PID alone never authorizes termination because it can be reused. A process-name match alone is also insufficient. On Linux the protected check covers every observed name, the kernel-reported executable basename (which a process cannot rename), and 15-byte truncated `comm` names that prefix a protected name. On Linux the backend opens a pidfd for the PID before the first identity check and sends every signal through it, so a process that exits and has its PID reused after the check never receives the signal; kernels without `pidfd_open` signal the PID directly right after the same check.

## Desktop usage limits

Limits count focused app time where foreground identity is available, and labeled open-app time on limited Wayland sessions. An unavailable adapter never fabricates usage or closes a target on an unproven observation. See [Usage limits](usage-limits.md#desktop-counting).

## Android observation and blocking

Android uses Usage Access to count explicitly selected packages and an opt-in Accessibility Service to observe package transitions for selected-app enforcement. The service does not inspect view content, text, gestures, notifications, or arbitrary package history.

When a selected package matches a fresh phase rule or exhausted limit, the service performs the standard Home action. It does not draw overlays, inject input, capture the screen, or act as device owner.

Ganbaru, Home, Settings, dialer, system UIDs, unknown packages, and stale projections are rejected.

## Android lifecycle

The lightweight Guardian process receives validated rule and phase projections through a private, signature-protected boundary and maintains a bounded journal while the main Activity is absent. Journal import accepts only the current vault identity. The projection is rebuildable device state, not canonical user configuration.

Usage Access and Accessibility require separate localized disclosures immediately before opening system settings. Returning from settings refreshes actual status. Revocation or denial leaves a useful read-only configuration surface and direct recovery action.

See [Android native services and data](../../platforms/android/native-services-and-data.md) and [Android permissions and security](../../platforms/android/permissions-and-security.md).
