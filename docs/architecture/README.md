# Architecture

Ganbaru AI is a pnpm and Cargo monorepo containing one Svelte 5 frontend, a Tauri v2 application shell, Tauri-free Rust domain crates, Android adapter plugins, and a Chromium extension. The architecture keeps canonical data local and shares domain behavior across platforms without pretending every platform has the same authority.

Exact dependency versions are authoritative in `package.json`, `apps/client/package.json`, `Cargo.toml`, and the package manifests. This documentation records why the major boundaries exist.

## System shape

| Layer | Primary location | Responsibility |
| --- | --- | --- |
| Product UI | `apps/client/src/` | Svelte components, typed client models, local presentation state, and platform-adaptive shells |
| Tauri composition | `apps/client/src-tauri/app/` | Commands, managed state, lifecycle, platform services, and adapter composition |
| Domain services | `crates/ganbaru-*` | Tauri-free persistence, contracts, provider transports, filesystem boundaries, and reusable logic |
| Durable storage | `crates/ganbaru-db/migrations/` and the active vault | SQLite schema, managed assets, and file-authoritative documents |
| Android adapters | `crates/ganbaru-mobile-*` and `apps/client/src-tauri/gen/android/` | Notifications, document transfer, media, anti-distraction access, and Android application integration |
| Browser integration | `extensions/chromium/` and `crates/ganbaru-native-messaging/` | Chromium blocking, status, and local native messaging |

The desktop binary and mobile library entry points are intentionally thin. `ganbaru-tauri-app` owns application composition. Core crates do not depend on Tauri when the domain can be tested and reused independently.

## Runtime boundaries

### Frontend and native backend

The frontend calls typed wrappers under `apps/client/src/lib/api/`. Tauri commands validate input and delegate to application or domain services. The frontend does not open the SQLite database directly and does not treat an IPC response as trusted merely because it came from the local process.

Long-running or user-sized filesystem and platform work stays bounded and off asynchronous command executors. Database transactions remain on SQLx's asynchronous path. See [Native backend](native-backend.md#asynchronous-and-blocking-work).

### Desktop and mobile

Desktop and mobile select different composition roots and frontend entries at build time. Shared components and domain services are reused where their behavior is genuinely common. Desktop process control, tray, native coding-agent execution, local working folders, updater behavior, and detached windows do not enter Android bundles.

Android uses narrow native plugins for capabilities that cannot be implemented reliably inside a WebView. See the [platform index](../platforms/README.md).

### Canonical and derived data

SQLite owns structured application records and the Notes graph. Files own document formats that are naturally external or user-authored, such as project working files and future diary documents. Search indexes, exported Markdown, projections, caches, benchmark databases, and device-local runtime state are derived or scoped data, not competing canonical stores.

The full storage contract is in [Data architecture](../data/architecture.md).

### Optional integrations

Native coding-agent providers, browser blocking, YouTube, Android system services, future sync, and future BYOK assistants all sit behind explicit adapters. Provider trust, browser installation, operating-system permission, or network reachability never widens application authority on its own.

See [Integration boundaries](integrations.md).

## Technology choices

- **Svelte 5 with runes** provides local reactive state without an external state manager.
- **Vite** builds separate desktop and mobile entry graphs from the same frontend workspace.
- **Tailwind CSS and shared primitives in `components/ui/`** provide reusable UI foundations while Ganbaru owns product-specific components.
- **Tauri v2** supplies the native shell, command boundary, platform configuration, and desktop or mobile composition.
- **Rust** owns SQLite, filesystem safety, provider processes, media playback, native messaging, platform services, and bounded background work.
- **SQLx with SQLite** provides the canonical structured store and embedded timestamped migrations.
- **Turborepo over pnpm workspaces** coordinates frontend tasks. Cargo owns the Rust workspace.

Dependencies are added for an implemented boundary, not speculative future use. Planned Yrs/Yjs, encrypted relay, BYOK, external MCP, diary, sleep, and gamification work does not belong in current dependency manifests until implementation starts.

## Detailed documents

- [Frontend](frontend.md): Svelte structure, state, localization, and loading boundaries.
- [UI foundations](ui-foundations.md): shared surfaces, rows, fields, controls, scroll fades, and hover submenus.
- [Native backend](native-backend.md): Rust workspace, Tauri composition, persistence, and native services.
- [Integration boundaries](integrations.md): Chat providers, browser integration, media, platform adapters, and future networked systems.
- [Architecture decisions](decisions/README.md): focused decisions whose rationale should outlive a particular implementation.
- [Data architecture](../data/architecture.md): sources of truth and the Ganbaru AI folder.
- [Security](../data/security/README.md): threat model and enforcement boundaries.
