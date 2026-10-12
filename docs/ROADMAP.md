# Roadmap

This roadmap records delivery horizons, not implementation history or feature design. Ganbaru AI develops across several domains in parallel, so it groups work by outcome rather than numbered phases. Detailed behavior and current status belong in the [feature index](features/README.md) and feature specifications.

## Implemented foundation

The repository has a substantial local desktop and Android foundation:

- A Svelte 5 and Tauri v2 application with separate desktop and mobile composition roots.
- An active-vault model with embedded SQLite migrations, managed assets, and portable folder identity.
- Calendar planning with recurrence, iCalendar import and export, notifications, and project links.
- Pomodoro rhythms with durable runs, idle and suspend handling, adaptive decisions, and progress surfaces.
- Projects with planning views, tasks, dependencies, custom fields, templates, scheduling links, and history.
- Notes with pages, blocks, databases, templates, links, comments, assets, history, and transfer workflows.
- Quick notes, English and Spanish localization, profiles, and customizable themes.
- Project channels over durable native coding-agent sessions, with workspace tools, terminals, checkpoints, review, and explicit access profiles.
- Local Music libraries and playlists with desktop and Android playback, plus YouTube integration.
- Chromium website blocking, desktop application blocking, and consented Android application enforcement.
- An adaptive Android shell covering the main features, portable backup and restore, and notifications.
- Private-LAN device linking with single-writer whole-vault handoff between one person's desktop and Android devices, plus concurrent Quick notes sync through the coordinator desktop (physical multi-device acceptance pending).

Implemented does not mean release-complete on every platform. The [feature index](features/README.md) and [Android platform document](platforms/android/README.md) identify partial areas and platform limits.

## Active completion work

These domains have useful implementations but still need hardening or missing product slices.

### Calendar and Pomodoro correctness

Complete physical desktop and Android acceptance of scoped Calendar edits, deletion and Undo, recurrence, Focus transitions, idle and suspend handling, and notification delivery.

See [Calendar](features/calendar/README.md) and [Pomodoro](features/pomodoro/README.md).

### Concurrent device synchronization

The sync engine, signed operation log, installation writers, and LAN hub transport exist, with Quick notes as the first concurrently synced domain while the rest of the vault keeps single-writer handoff. Remaining work: physical multi-device acceptance, converting the other domains, collaborative text, compaction, anchored authority with LAN peer exchange, end-to-end operation encryption and enrollment, and delivery beyond the LAN through an optional user-hosted relay or always-on anchor. See [Device linking and synchronization](data/sync.md).

### Android release readiness

- Complete physical-device and emulator acceptance, including gesture navigation and predictive Back.
- Provision durable signing and finish distribution preparation.
- Complete remaining attachment transfers and encrypted scheduled backup.
- Keep notifications, background focus, media, and blocking reliable across Android versions and manufacturers.

See [Android](platforms/android/README.md).

### Projects and Notes maturity

- Finish incomplete planning, database, editor, transfer, and history workflows without weakening the source-of-truth model.
- Keep large collections bounded as they grow.
- Refine project-to-Calendar and project-to-Notes transitions around accepted commitments rather than duplicated state.

See [Projects](features/projects/README.md) and [Notes](features/notes/README.md).

### Chat and local agent execution

- Finish the user-facing agent, mention, reply-thread, scheduling, and access-review flows.
- Harden provider recovery, workspace observation, scratch cleanup, checkpoints, and review across supported providers.

See [Chat](features/chat/README.md), [AI integration](features/ai/README.md), and [Chat access control](data/access-control.md).

### Music and the distraction blocker

- Complete local library repair and assignment workflows and Android background playback reliability.
- Redesign YouTube interaction so Ganbaru never obscures or disables required embedded-player controls.
- Complete blocking rule coverage, diagnostics, and false-positive recovery across platforms.

See [Music](features/music/README.md) and [Distraction blocker](features/distractions/README.md).

## Next product outcomes

These complete missing parts of the core anti-procrastination and anti-burnout loop.

### Diary and sleep routines

Morning and evening diary flows with mood and energy baselines, and an Android sleep alarm. See [Diary](features/diary.md) and [Sleep alarm](features/sleep-alarm.md).

### Human work environments

Saved desktop environments that prepare applications, browser resources, Music, distraction rules, and project context when a Calendar block begins. They never grant AI execution authority. See [Work environments](features/work-environments.md) and [Edge panel](features/edge-panel.md).

### Structured project delegation

Reviewable planning proposals, task-linked agent runs, assignment and review workflows, budgets, and sustainable work-in-progress limits on top of Projects and Chat. A person approves commitments before Projects or Calendar change. See [Guided planning and review](features/projects/guided-planning.md) and [Agents and coordination](features/chat/agents-and-coordination.md).

## Later outcomes

### Human collaboration

Multi-person collaboration with resource-scoped authorization, invitations, revocation, and permission-safe derived data. This is separate from linking one person's devices. The experience is specified in [Contacts and invitations](features/collaboration/README.md); delivery belongs to [Device linking and synchronization](data/sync.md).

### General BYOK assistants and external access

Hosted and local BYOK assistants through the same agent, permission, and provenance model, plus a separately authorized external MCP service. Native coding-provider trust is never reused as general Ganbaru authority. See [AI integration](features/ai/README.md).

### Additional platforms and interoperability

- macOS and iOS after Apple hardware and release infrastructure are available.
- Firefox after shared browser-rule semantics and native-host packaging are stable.
- Broader iCalendar conformance backed by fixtures and evidence.

See [Platforms](platforms/README.md) and [iCalendar compatibility](interop/icalendar/README.md).

## Deferred

Gamification remains deliberately deferred. It must reflect genuine progress, protect recovery, avoid paid chance mechanics, and never become a prerequisite for the core app. See [Gamification](features/gamification.md).

## Dependency rules

- Diary and sleep build on the existing vault, notification, Calendar, Pomodoro, and Android foundations.
- Work environments depend on stable Calendar activation, Music, and anti-distraction adapters.
- Structured delegation depends on Projects, Notes, Chat, and access control.
- Human collaboration depends on domain-specific sync operations and permission-safe derived data.
- BYOK assistants and external MCP depend on the same identity, authorization, and provenance rules as local coordination.
- Apple platform releases depend on build and signing capability, not on unrelated feature completion.
