# Product direction

Ganbaru AI is an anti-procrastination and anti-burnout productivity app. It combines planning, focus, knowledge, communication, and distraction blocking without requiring hosted Ganbaru infrastructure.

The product is free, local-first, privacy-first, donation-funded, and licensed under AGPL 3.0. AI is optional. The app must remain useful when no provider is installed, configured, or reachable.

## Product principles

### Local ownership

The user's Ganbaru AI folder and selected external working folders remain under the user's control. Structured application data lives in SQLite. Canonical documents remain files when their format is naturally file-owned. Export formats do not silently become a second source of truth.

### Sustainable productivity

The product should help people make useful progress without rewarding exhaustion. Calendar capacity, recovery time, Pomodoro history, review load, and later delegation budgets are constraints, not obstacles to bypass.

### Connected systems with clear owners

Features should cooperate through typed, auditable transitions. Calendar owns time. Projects owns committed work. Notes owns durable knowledge. Chat owns communication and provenance. Execution sessions own provider activity. No feature should quietly duplicate another feature's canonical records.

### Progressive complexity

The first useful path should stay simple. Advanced recurrence, custom focus rhythms, automation, access profiles, sync, and AI controls should appear when their context requires them. Powerful behavior must not make ordinary local use harder to understand.

### Explicit authority

Installing a provider, joining a channel, mentioning an AI agent, or possessing a record identifier never grants broader access by implication. Sensitive reads, filesystem access, shell execution, external services, and cross-channel disclosure require explicit authority.

### Self-hosted and donation-funded

Ganbaru AI does not depend on a subscription or a hosted service operated by the project. Product infrastructure such as sync must be user-provisioned and self-hostable, with setup guidance that non-technical users can follow. Optional third-party integrations may use their documented external services.

### Platform-appropriate behavior

Linux and Windows are the primary desktop targets. Android is the first mobile implementation. macOS and iOS follow after their build, signing, and platform contracts are ready. Shared domain logic does not imply identical capabilities on every operating system.

### Text selection

Interface labels, controls, and explanatory dialog text are not selectable. User-authored content, editable fields, code, and terminal output remain selectable so people can copy and work with their data.

## Product areas

The [feature index](../features/README.md) is the current capability map. The main product areas are:

- Calendar planning, recurrence, import, export, notifications, and capacity.
- Pomodoro focus rhythms, recovery, idle handling, and progress displays.
- Projects for tasks, planning views, dependencies, history, templates, and later structured delegation.
- Notes and Quick notes for durable knowledge and lightweight capture.
- Chat for project communication and optional local coding-agent execution.
- Distraction blockers for browsers, desktop applications, and selected Android applications.
- Music and app sounds integrated with focus and platform media controls.
- Themes, localization, profiles, and adaptive desktop and mobile shells.
- Future diary, sleep, work environment, sync, collaboration, BYOK assistant, and gamification systems.

Detailed behavior belongs in the relevant feature document. Delivery order belongs in the [roadmap](../ROADMAP.md).

## Scope boundaries

Ganbaru AI is not a hosted project-management service, an enterprise device-management product, a browser surveillance tool, or an automatic authority broker for AI. It does not collect telemetry or require a Ganbaru account.

Planned collaboration does not change the local-first source-of-truth model. Planned AI does not change the explicit authorization model. Planned gamification must measure genuine progress without gambling with real money or punishing recovery.

## Related documents

- [System map](system-map.md): ownership and cross-feature transitions.
- [Feature index](../features/README.md): current implementation status and feature specifications.
- [Architecture](../architecture/README.md): current code and platform boundaries.
- [Data architecture](../data/architecture.md): storage categories and the Ganbaru AI folder.
- [Roadmap](../ROADMAP.md): remaining delivery sequence.
