# Anti-distraction native application enforcement

Desktop and Android enforce selected applications through different operating-system boundaries. They share user intent and explanations but do not pretend their permissions or observations are equivalent.

## Shared safety rules

- Only explicitly selected applications can be targets.
- Selection records stable platform identity plus a user-facing label.
- Protected applications are rejected at selection and enforcement time.
- Stale, disabled, removed, mismatched, or unauthorized rules perform no destructive action.
- Enforcement is rate-limited and explains how to change the responsible setting.
- Permission denial disables the affected adapter without disabling unrelated Ganbaru AI features.

## Desktop selection and matching

Application discovery is on demand. The picker excludes system and basic utility targets that are unsafe or unrelated to procrastination. Adding or removing an enforced app requires confirmation.

A desktop identity can include executable, application identifier, desktop entry, and normalized match names. Generic runtimes or shell hosts are not sufficient targets because closing them could affect unrelated work.

## Desktop phase enforcement

Process or foreground checks run only while a relevant phase rule is active. Ending the phase stops checks immediately, and late observations are ignored.

On platforms with a safe close adapter, enforcement binds the exact observation to the exact persisted rule. Immediately before a graceful close and before any stronger fallback, the backend revalidates process identity, start identity, active vault, fresh phase or limit state, target rule, and protected-app policy.

A PID alone never authorizes termination because it can be reused. A process-name match alone is also insufficient.

## Desktop usage limits

Limits count focused app time where foreground identity is available, and labeled open-app time on limited Wayland sessions. An unavailable adapter never fabricates usage or closes a target on an unproven observation. See [Usage limits](usage-limits.md#desktop-counting).

## Android observation and blocking

Android uses Usage Access to count explicitly selected packages and an opt-in Accessibility Service to observe package transitions for selected-app enforcement. The service does not inspect view content, text, gestures, notifications, or arbitrary package history.

When a selected package matches a fresh phase rule or exhausted limit, the service performs the standard Home action. It does not draw overlays, inject input, capture the screen, or act as device owner.

Ganbaru AI, Home, Settings, dialer, system UIDs, unknown packages, and stale projections are rejected.

## Android lifecycle

The lightweight Guardian process receives validated rule and phase projections through a private, signature-protected boundary and maintains a bounded journal while the main Activity is absent. Journal import accepts only the current vault identity. The projection is rebuildable device state, not canonical user configuration.

Usage Access and Accessibility require separate localized disclosures immediately before opening system settings. Returning from settings refreshes actual status. Revocation or denial leaves a useful read-only configuration surface and direct recovery action.

See [Android native services and data](../../platforms/android/native-services-and-data.md) and [Android permissions and security](../../platforms/android/permissions-and-security.md).
