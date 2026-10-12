# Distraction blocker

The distraction blocker adds calm, explainable friction when a selected website or application conflicts with the user's current intention. It combines Pomodoro phase rules with independent daily and weekly usage budgets.

## Current scope

| Surface | Status |
| --- | --- |
| Chromium extension and local native messaging | Implemented |
| Browser domains, categories, allowlists, phase schedules, block page, and usage counting | Implemented |
| Desktop application selection, protected-process enforcement, and usage counting | Partial across operating-system adapters |
| Android selected-app usage and Accessibility-based Home redirection | Implemented in source; release validation pending |
| Firefox extension | Planned |
| Android website filtering and iOS enforcement | Planned |
| Work-environment and morning-routine activation | Planned |
| Content-aware and optional AI relevance rules | Planned |

## Principles

**Useful friction, not hostile lock-in.** The feature helps users interrupt habits. It is not parental control, device management, a firewall, or a promise that bypass is impossible.

**Explain every decision.** A blocked surface names the winning rule or exhausted budget and the active phase when relevant.

**Fail open by default.** Missing, expired, invalid, or disconnected state stops enforcement. A future strict mode would require explicit opt-in and a safe recovery path.

**Local and minimal.** Browser reporting uses normalized hosts, not full URLs. Native adapters observe only the app identity and timing configured rules need. Page content, taps, notifications, and general app history are never collected.

**Protect essential software.** Ganbaru AI, Home, Settings, dialers, system UI, shells, generic runtimes, and other protected applications can never be targets. Destructive enforcement revalidates the exact persisted rule and observed process or package at the final moment.

**Do not judge the user.** Block and usage history can suggest configuration changes. It never proves productivity or becomes a shame, ranking, or engagement surface.

## Non-goals

- Advertising or tracker blocking.
- Complete browsing-history or network inspection.
- Remote surveillance or administration.
- Blocking development, banking, emergency, operating-system, or browser-management surfaces.
- Automatic LLM judgment without explicit provider, context, redaction, and consent.

## Data ownership

Rules, schedules, selected apps, categories, allowlists, and usage-limit definitions are active-vault configuration in `config.json`. Usage samples and normalized block history are canonical SQLite data. Small device-local runtime snapshots let the extension and Android enforce while the main UI is absent; they expire and never become rule authority.

Observation, accounting, and enforcement are native: one desktop owner in Rust, and the Android Guardian service with a Rust publisher for shared totals. Settings only read native projections. Phase rules require a fresh committed Focus phase and never infer a later phase from elapsed time. See [Usage limits](usage-limits.md).

Moving configuration into SQLite would require a separate data-architecture decision; planned work environments do not imply it.

## Documentation map

- [Usage limits](usage-limits.md)
- [Rules and activation](rules-and-activation.md)
- [Browser integration](browser.md)
- [Native application enforcement](native-apps.md)
- [Android platform boundary](../../platforms/android/native-services-and-data.md)
- [Data security](../../data/security/README.md)
