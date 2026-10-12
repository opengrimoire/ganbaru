# Android architecture and capabilities

## Composition

Android uses the Tauri mobile library entry and a mobile-specific Rust composition root. Vite selects the mobile frontend graph before Svelte mounts, and a typed platform profile with an immutable capability registry keeps desktop-only behavior out of the Android runtime path.

The product uses one visible Activity and one WebView. Feature navigation happens inside the shared Svelte shell. Native services and providers do background or transfer work but never create competing application shells. Cross-window synchronization is a build-time no-op because there is only one WebView.

## Build-time exclusion

The Android artifact does not compile or initialize:

- Coding-agent provider processes, PTYs, terminals, Git workspaces, checkpoints, and filesystem observers.
- Working-folder Markdown and generic native path pickers.
- Rodio, desktop media controls, and loopback local-media hosting.
- Desktop process closing and browser native messaging.
- Desktop notifications, overlays, tray, detached windows, preview webviews, updater, and benchmark harness.

Exclusion at build time, rather than hiding controls in the UI, reduces artifact size and keeps dormant desktop authority from becoming reachable through a frontend mistake.

## Shared capability matrix

| Domain | Shared contract | Android adapter or restriction |
| --- | --- | --- |
| Vault | Marker, configuration, SQLite, managed assets, single-writer ownership | Application-private default, document-tree import, portable backup and restore, secure LAN handoff and read-only refresh |
| Calendar | Events, recurrence, reminders, projects, active runs | Native notification and alarm projection |
| Projects | Full local planning and task views | Responsive UI, no local coding working-folder execution |
| Notes | Pages, blocks, databases, history, links, assets | Responsive UI, document picker for bounded files, no working-folder Markdown |
| Chat | Channels, messages, replies, search, drafts, scheduling | Provider-free communication and existing review activity only; no Codex, Claude, Cursor, Grok, OpenCode, shells, terminals, or Git |
| Pomodoro | Canonical accepted runs, history and recovery in `ganbaru-pomodoro` | Commitment reminders, accepted-phase deadline alarms and ongoing notification |
| Music | Canonical library, playlists, assignments, player state | Selected document tree and Media3 local audio; no desktop soundscape engine |
| Distraction blocker | Shared rule and usage-limit intent | Selected packages, Usage Access, Accessibility Home action, native journal, linked-device combined usage |
| Settings | Shared portable preferences | Platform equivalents and direct special-access recovery |
| Updates | Version and release information | Distribution source owns installation and updates; desktop updater absent |

Future encrypted sync can make mobile a cross-device communication and review client. Remote execution must identify the authorized desktop or runner and never pretend work is executing on the phone.

## Feature ownership

Platform documentation defines only Android differences. Canonical feature behavior stays in [Calendar](../../features/calendar/README.md), [Projects](../../features/projects/README.md), [Notes](../../features/notes/README.md), [Chat](../../features/chat/README.md), [Pomodoro](../../features/pomodoro/README.md), [Music](../../features/music/README.md), and [Distraction blocker](../../features/distractions/README.md).

## Future iOS alignment

iOS should reuse portable contracts while supplying its own native storage, background execution, notification, media, and permission adapters. Android-specific services, Accessibility behavior, or filesystem assumptions do not define the iOS product.
