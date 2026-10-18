# Ganbaru

Ganbaru is an anti-procrastination and anti-burnout productivity app. It is free, local, open-source (AGPL 3.0), privacy-first, and lightweight, with opt-in AI. It is built with Tauri v2 (Rust) and Svelte 5 for Linux, Windows, and a pre-release Android implementation. macOS and iOS are future platform work.

Features are highly interconnected. Calendar, Pomodoro, Projects, Notes, Chat with local coding agents, the distraction blocker, Music, Quick notes, themes, localization, and the Android shell exist and are still being completed. Concurrent sync covers Quick notes so far. The diary, sleep alarm, and work environments are planned; gamification is deferred. [docs/features/README.md](docs/features/README.md) owns the current status of each feature.

## Documentation

All documentation lives in `docs/`. Start with [docs/README.md](docs/README.md), which routes readers by decision type and defines the status vocabulary (Implemented, Partial, Planned, Deferred, Reference).

- `docs/product/`: product direction, principles, scope, and system ownership.
- `docs/features/`: intended user-facing behavior.
- `docs/ROADMAP.md`: delivery horizons and deferred areas.
- `docs/architecture/`: code boundaries and durable technical decisions.
- `docs/data/`: sources of truth, schema domains, invariants, authorization, hazards, security, and sync.
- `docs/algorithms/`: pure-logic specifications and worked examples.
- `docs/interop/`: external standards, conformance, fixtures, and client observations.
- `docs/platforms/`: platform-specific capability, UX, delivery, and acceptance contracts.
- `docs/performance/`: benchmark methodology and recorded measurements.
- `docs/testing/`: validation gates, infrastructure, test design, and manual matrices.
- `docs/operations/` and `CONTRIBUTING.md`: branch, PR, ruleset, and release workflow.

Docs describe what the app is meant to be and why, not a transcript of the code. When to update them:

- Update a doc when a change alters intended behavior, a public contract, a data or security invariant, a platform capability, a status claim, or an important decision. Most ordinary changes (refactors, bug fixes, visual tweaks, new private helpers, intermediate development steps) need no doc update.
- Record the rule and, for important or non-obvious decisions, a short reason. Do not record micro-interactions, pixel or animation details, private helper names, module walk-throughs, migration history, or mid-development notes. Source code, migrations, and manifests are authoritative for exact shape.
- Keep one owning document per contract and link to it instead of repeating it.
- Label current behavior and intended end state explicitly. When implementation discovers a better direction than an older spec, update the spec. When meaningful behavior has no suitable spec, add the smallest appropriate document.
- Prefer editing or deleting existing text over appending. A doc update should usually be a few sentences.
- `AGENTS.md` carries durable repository context only; detailed behavior belongs in the corresponding docs area.

## Workspace structure

> Update this map when top-level or listed directories are created, renamed, or removed. Entries are existing paths unless marked as planned.

```
.github/: issue forms, PR and release-notes templates, security policy, code of conduct, Actions workflows and their helper scripts, release notes config
docs/: documentation (see above); assets/ holds documentation images
apps/
  client/: Tauri app (Svelte frontend + Rust backend)
    src/: Svelte frontend
      App.svelte, MobileApp.svelte: desktop and mobile shell roots
      app.css, ui-foundations.css: global styles and shared surface, row, and field utilities
      main-desktop.ts, main-mobile.ts, main.ts: platform bootstraps and virtual entry selector
      lib/: shared frontend code
        api/: typed wrappers around Tauri commands, split by domain (Chat and Notes have several clients)
        components/: Svelte components grouped by feature (calendar, chat, collections, diagnostics, icon-picker, mobile, music, notes, pomodoro, projects, quick-notes, settings, sync, themes, title-bar, vault, and others), with subfolders that mirror lib/ domains (for example calendar/grid/, chat/timeline/, notes/blocks/, projects/list/); ui/ holds handwritten shared primitives
        benchmark/: benchmark runner, samplers, output, and scenarios
        build/: build-time CSS transforms used by the Vite config
        calendar/: shared Calendar logic, types, recurrence rules, navigation helpers, and iCalendar parser/serializer
        chat/: Chat contracts, runtime validation of untrusted responses, and interaction models and controllers
        color/: color math and display helpers
        contacts/: contact trust mapping and presentation helpers
        diagnostics/: memory sampling and report helpers for the performance popover
        distractions/: shared browser and desktop blocking rules
        i18n/: typed localization catalogs, locale resolution, and formatters
        music/: frontend music source and playback helpers
        notes/: Notes contracts, validation, editor operations, databases, and tree helpers
        pomodoro/: Focus command contracts, native projections, and presentation helpers
        profile/, projects/, quick-notes/: domain logic for those features
        scheduling/: lifecycle and notification schedulers
        settings/: Settings section identifiers shared by launchers and the Settings modal
        sync/: sync status presentation helpers
        mobile/: Android shell back handling, layout, top bar context slot, and persistence lifecycle helpers
        stores/: Svelte rune stores and domain controllers for runtime state; large stores keep private modules in a same-named subfolder (calendar/, chat/, music-player/, notes/, projects/)
        themes/: theme definitions, derivation, serialization, operations, editor session, and theme JSON file access
        utils/, vault/, windows/: shared helpers, vault state, and detached window and cross-window sync helpers
        workspace-tabs/: workspace tab model, location mapping, and presentation helpers
    static/: static assets
    scripts/: repo-owned scripts grouped by purpose (release/, bundle-contracts/, checks/, provider-protocols/, codegen/, browser-extension/); tauri-cli.mjs wraps the Tauri CLI
    test-fixtures/: shared test fixtures such as sample .ics files
    src-tauri/: Tauri package (crate ganbaru), configuration, and platform entries
      src/main.rs, src/lib.rs: thin desktop entry and mobile library entry
      app/: ganbaru-tauri-app library with Tauri commands, managed state, and platform integrations
        build.rs: emits the `desktop` and `mobile` cfg aliases used for platform gating
        src/runtime.rs, runtime/: desktop and mobile composition roots and handler registration tests
        src/db.rs, vault.rs, vault/: active-folder authorization, SQLite adapter boundary, vault config, document transfer, Android backup and restore
        src/vault/handoff/: LAN device linking and single-writer whole-vault handoff
        src/calendar.rs, calendar/: Calendar command adapters and the platform device-date source
        src/chat.rs, chat/: Chat command adapters, coordination, access profiles, internal MCP tools, checkpoints, previews, and provider settings
        src/notes.rs, notes/: Notes command adapters, working-Markdown composition, and asset authorization
        src/pomodoro.rs, pomodoro/: Focus command adapters, native execution runtime, idle probes, and timer overlays with enforcement
        src/projects.rs, projects/: Projects command adapters, icon assets, and working-folder device state
        src/distractions.rs, distractions/: browser, desktop, Android, and linked-device blocking, observation, close authorization, and the usage spool
        src/music.rs, music/: Music library command adapters, Android media inspection, native Music session, media controls, YouTube host, and soundscapes
        src/notifications.rs, notifications/: desktop native notifications and Android notification capabilities
        src/sound_effects.rs, system_command.rs: app sound effects and bounded fixed-command execution
        src/contacts.rs, contacts/: person identity storage, contact card assembly, contact and request persistence, and LAN request delivery
        src/sync.rs, sync/: sync service lifecycle, paired transport, Quick note image transfer, recovery listing, status, and sync commands
        src/quick_notes.rs, quick_notes/: Quick notes command adapters and image file commands
        src/tray.rs, app_windows.rs, themes.rs, updates.rs, profile_images.rs: tray, window raising, theme command adapters, updates, and profile images
        src/benchmark.rs, benchmark/, first_use_contracts.rs: benchmark state, seed data, memory reports, and first-use query contracts
      capabilities/: permission declarations (desktop and mobile)
      permissions/: app command permission sets granted by capabilities
      gen/android/: generated Android project and native platform integration
      gen/schemas/: generated Tauri schema files
      package-repo/: generated package repository public key staging (ignored)
      packaging/: Linux package lifecycle scripts (deb/, rpm/), desktop entry template, and polkit policy (linux/)
      icons/, build.rs, tauri.conf.json, tauri.dev.conf.json, tauri.android.conf.json, Cargo.toml
crates/
  ganbaru-db/: Tauri-free SQLite pool registry, connection configuration, migrations, and schema tests
    migrations/: embedded SQLx SQLite migrations
  ganbaru-civil-time/: Tauri-free conversion between instants and zoned wall times, with explicit gap and fold rules
  ganbaru-atomic-files/: Tauri-free atomic file replacement through exclusive random temporary files and symlink-refusing managed folder creation
  ganbaru-image-metadata/: Tauri-free bounded image header parsing, type detection, and dimension limits shared by Notes and Quick notes
  ganbaru-pomodoro/: Tauri-free Focus persistence, validation, recovery, activity admission, transactional execution, and adaptive policy
  ganbaru-quick-notes/: Tauri-free Quick notes persistence, search, lifecycle and trash retention, ordering, tags, images, and conflict resolution
  ganbaru-music/: Tauri-free Music context assignments per Focus phase and the Music library error shape
  ganbaru-music-library/: Tauri-free Music library persistence, queries, playlists, review, transfer, soundscapes, local media refresh and artwork, and session policy
  ganbaru-music-player/: Tauri-free desktop local media inspection and decoding worker that executes committed Music session effects
  ganbaru-projects/: Tauri-free Projects persistence, structure, tasks, history, custom fields, dependency cascades, reordering, Calendar scheduling rows, and validation
  ganbaru-calendar/: Tauri-free Calendar persistence, iCalendar import and export, native reads, scoped edits, deletion and Undo, and recurrence expansion
  ganbaru-distractions/: Tauri-free distraction blocker contracts, application rules, usage limits, usage normalization, and elapsed accounting
  ganbaru-notes/: Notes domain, persistence, transfers, history, assets, and bounded filesystem operations
  ganbaru-chat-contracts/: provider-neutral Chat IDs, commands, events, DTOs, errors, and protected workspace path rules
  ganbaru-chat-providers/: provider processes, transports, drivers, cancellation, and registry
  ganbaru-chat/: Chat persistence, runtime, Git workspaces, checkpoints, review, and application services
  ganbaru-working-folders/: Tauri-free working-folder IDs, bindings, and device-state operations
  ganbaru-contacts/: Tauri-free person key pairs, signed contact cards, request signatures, and trust kinds
  ganbaru-handoff/: Tauri-free LAN handoff control protocol, message bounds, compatibility, QR encoding, staged bundle validation, device pairing identity, membership, and transfer state, and Linux firewall authorization
  ganbaru-sync-contracts/: Tauri-free sync identifiers, clocks, version vectors, writer certificates, order keys, and the signed operation format
  ganbaru-sync/: Tauri-free sync engine with the replication manifest, capture triggers, sealing, apply and merge, conflicts, recovery, and the simulation harness
  ganbaru-sync-replica/: Tauri-free replica sync with guarded vault access, installation writer keys and record, carry-forward, recovery offers, and the LAN hub and client exchange of operations and Quick note images
  ganbaru-themes/: Tauri-free user theme persistence, seed resets, upgrade dismissals, and row validation
  ganbaru-native-messaging/: ganbaru-native-messaging browser host binary
  ganbaru-mobile-*/: Android plugins for documents, the distraction blocker, media, and notifications
extensions/chromium/: Chromium extension (manifest v3); chromium-dev/ is a generated, ignored dev copy
Cargo.toml, rust-toolchain.toml, rustfmt.toml, turbo.json, pnpm-workspace.yaml, package.json: workspace configuration and root scripts
```

Planned work without source directories yet includes the self-hosted sync relay, the Firefox extension, the diary, BYOK assistant, edge panel, sleep alarm, work environments, and gamification.

## Ganbaru folder structure

> Everything the app produces belongs to one active vault. On desktop, production defaults to `Documents/Ganbaru` and development to `Documents/Ganbaru Dev`, unless the user selects another location. Android keeps the active vault in application-private storage. Asset subdirectories are created on demand. Planned paths are labeled.

```
Ganbaru/
  vault.json: folder marker, id, display name, and schema version
  config.json: folder-local preferences, UI state, and anti-distraction settings
  ganbaru.sqlite: SQLite source of truth for structured data, Notes, and indexes
  .database-rollbacks/: desktop copies of the database kept by Reset local data, bounded and restorable from Settings > Data
  notes/daily/, notes/projects/: reserved note document directories
  notes/exports/: derivative Markdown exports (planned, not authoritative)
  diary/morning/, diary/evening/: dated diary entries (planned feature)
  projects/{project-id}/: managed working folder created for every project
  reports/: generated project status reports (planned feature)
  assets/: managed user assets (profile/, chat/attachments/, chat/browser-artifacts/, notes/page-icons/, notes/page-covers/, notes/files/, project-icons/, quick-notes/images/)
  templates/: reserved file-based project and methodology templates
  .yjs/: reserved Yjs document state cache
```

Music files stay wherever the user keeps them; the vault stores library metadata, source identities, playlists, assignments, and playback state, not media bytes. Desktop backups go to a user-selected path outside the vault. Android portable backups are exported to shared Downloads and restored through the system document picker.

Tauri's platform app config directory stores device-local bootstrap and runtime state only (`app-state.json`, benchmark state and SQLite, anti-distraction runtime snapshots, Chat preview web data per working folder). Production and dev builds keep separate app config directories and therefore separate active-folder pointers.

## Key conventions

- **Tooling:** pnpm workspaces with Turborepo, Vite, plain Svelte 5 with runes (not SvelteKit), and Tauri v2. Rust uses edition 2024, resolver 3, and rustfmt style 2024.
- **State management:** Svelte 5 runes (`$state`, `$derived`, `$effect`), no external state manager. Durable runtime authority (Calendar recurrence and mutations, Focus execution, Music sessions) lives in Rust; the frontend renders native projections.
- **Data architecture:** documents (diary entries, project docs, reports, attachments) are files on disk and the file is the source of truth where its format is canonical; SQLite may index them. Structured data and document graphs (Notes pages and blocks, Calendar events, project tasks, Focus configs and runs) live in SQLite as the source of truth. Markdown for Notes is derivative import, export, or bridge output only.
- **AI integration:** three opt-in paths. (1) Implemented: local coding-agent Chat in project channels over Rust-owned native provider sessions (Codex, Claude, Cursor, Grok, OpenCode), with durable SQLite history, project working folders, and a scoped ephemeral loopback MCP endpoint for application-owned host tools. That endpoint is infrastructure, not a participant identity. (2) Planned: a BYOK assistant for hosted and local user-configured providers. (3) Planned: a separately authorized MCP service for external AI clients. See `docs/features/ai/` and `docs/features/chat/`.
- **Chat storage boundary:** organizational Chat state (channels, sessions, events, drafts, checkpoints, access profiles, authorization revisions, scratch scopes) lives in the vault SQLite database. Managed attachment bytes live under `assets/chat/attachments/`. Absolute folder paths, scratch paths, executables, provider homes, diagnostics, provider-native trust, and process state are device-local. Provider-native trust never widens organizational authority. Provider secrets live only behind operating-system credential references. See `docs/data/schema/chat.md` and `docs/data/security/chat.md`.
- **Localization:** user-facing UI text must use the typed i18n catalog. Language selectors show languages as autonyms (`English`, `Español`), while non-language options like system preference are localized. Dates, times, numbers, plurals, relative minutes, and lists use the locale helpers.
- **Sync:** LAN device linking, single-writer whole-vault handoff, and concurrent Quick notes replication through the coordinator desktop (signed operation logs, installation writers, deterministic merges, conflicts, and recovery) are implemented in source, with physical multi-device acceptance pending. The rest of the vault stays single-writer on replicas. Concurrent sync of other domains, Yrs/Yjs text, anchored authority with LAN peer exchange, operation encryption, and the encrypted relay are planned. See `docs/data/sync.md`.
- **Focus evidence:** Android Calendar alarms are reminders and cannot start runs or record later phases. Recovery uses only committed SQLite execution. Desktop automatic admission requires fresh local activity; explicit starts remain available.
- **Branching and releases:** topic branches from `dev`, PRs back to `dev`. Direct pushes to `dev` or `main` are not normal workflow. `main`, `app-v*` tags, release approval, published releases, and package publication are controlled by organization admins for supply-chain safety. See `CONTRIBUTING.md`, `docs/operations/release/README.md`, and `docs/operations/repository-policy.md`.
- **Pull requests:** for review work, create a neutral topic branch from current `dev` (for example `docs/github-templates` or `fix/calendar-import`; never tool, assistant, or vanity names in branches, commits, or PR titles). Open PRs into `dev` unless the user asks for a release PR. Creating a PR does not imply merging. Merge only when the user explicitly asks, or asks to complete the whole PR flow after checks pass; first confirm the PR is mergeable, required checks passed, and the branch is up to date. Release PRs from `dev` to `main` go through the `main` merge queue: do not update `dev` with `main`, do not enable auto-merge, and if `gh pr merge` tries auto-merge, use GitHub's queue action or the GraphQL `enqueuePullRequest` mutation. If a non-release branch is out of date, update it once, then merge if already authorized. After merging into `dev`, sync local `dev` to `origin/dev` and delete the merged topic branch locally and remotely. After merging into `main`, sync local `main` to `origin/main`. Do not keep backup branches unless asked.
- **Commit signing:** commits are signed with the configured SSH key. If signing fails, stop and report it instead of creating an unsigned commit.

## Testing

Read `docs/testing/README.md` when changing tests, validation scripts, task ordering, concurrency, sharding, cache inputs, test profiles, or test target selection.

**Writing tests:**

- Tests live next to source with a `.test.ts` suffix (for example `utils.ts` and `utils.test.ts`).
- Pure functions are the best candidates. If a function depends on Tauri IPC or the DOM, extract the pure logic or skip testing it.
- Match the depth and style of existing tests in the same area. Cover edge cases, not just happy paths; shallow "it exists" tests are worthless.
- Test names describe behavior, not implementation.

**UI changes:** agents cannot manually verify the real Tauri app UI. Do not start a dev server, launch Tauri, or run HTTP smoke checks as a substitute. During iterative UI work, use the narrowest relevant checks and rely on the user's manual inspection for visual confirmation. Add focused tests when a UI behavior needs more confidence than existing checks provide.

**Resource limits:** the machine is memory-limited, so validation is deliberately serialized.

- Use the root `check`, `test`, and `validate` scripts for broad verification instead of direct full-suite `turbo`, `vitest`, or `cargo` commands. They run Rust before frontend work, use one Cargo build job and two Rust test threads, and split Vitest into sequential one-worker shards. `validate:ci` and its `validate:ci:rust` and `validate:ci:frontend` halves (four Cargo jobs and test threads, two-worker Vitest shards) are for hosted Linux CI only. Do not raise concurrency, combine stages, or remove sharding without measuring peak memory and confirming coverage.
- Never run Cargo work concurrently with Vitest, Svelte checks, Turbo, or another Node validation command, and never start another validation command while a root `check`, `test`, `validate`, or `validate:full` is running.
- For focused checks, use one Vitest worker and one Cargo job and test thread. Add `--lib` when the filtered Rust test is in the library; use `--bin <name>` only when testing that binary. Confirm focused Vitest runs report only the requested files.
- The workspace test profile inherits the development profile's line-tables-only debug information so both share dependency artifacts; do not restore full test debug information unless a debugger session needs it.
- Keep Cargo commands portable across Linux, Windows, and macOS. Do not require an external linker; optional linker optimizations must fall back to the standard toolchain and be benchmarked before adoption.

**Choosing a gate:**

- Trivial, mechanically obvious edits with no plausible effect on compilation, types, styling, behavior, generated output, persisted data, or public interfaces (copy text, punctuation, an equivalent icon swap) need no checks.
- Small UI-only or docs-only edits: run `pnpm -w run check` or `pnpm -w run editor-check` only when the edit affects Svelte compilation, TypeScript, Tailwind classes, or shared UI structure. Run focused tests only when behavior changes.
- Backend, persistence, SQLite, import/export, migrations, project membership, note saving, or other data-loss-sensitive changes: run focused tests immediately, then a broader gate before committing.
- `pnpm -w run validate`: before opening a PR, before a release, when the rules above require a broader gate, or on explicit request. Ordinary task completion or a commit does not require it by itself. If a batch already passed, do not rerun it for later isolated changes.
- `pnpm -w run validate:full`: dependency, lockfile, security, release, and audit-sensitive work, or on explicit request.
- After the relevant gate passes, finish without extra dev-server, Tauri launch, status, or diff checks unless directly required.

**Commands (always use `-w` for root scripts):**

- `pnpm -w run check`: Rust formatting and clippy, then the workflow signing-isolation check, provider protocol snapshot checks, and Svelte and TypeScript checks through Turbo.
- `pnpm --dir apps/client run check`: client-only Svelte and TypeScript checks.
- `pnpm -w run editor-check`: editor diagnostics, including Tailwind canonical class checks.
- `pnpm -w run test`: serialized Rust tests followed by sequential one-worker Vitest shards.
- `pnpm --dir apps/client exec vitest run path/to/file.test.ts --maxWorkers=1`: focused frontend test file.
- `cargo test -p ganbaru-chat --lib -j 1 test_name -- --test-threads=1`: focused core crate test (substitute `ganbaru-notes`, `ganbaru-atomic-files`, `ganbaru-db`, `ganbaru-civil-time`, `ganbaru-distractions`, `ganbaru-chat-contracts`, `ganbaru-chat-providers`, `ganbaru-pomodoro`, `ganbaru-quick-notes`, `ganbaru-music`, `ganbaru-music-library`, `ganbaru-music-player`, `ganbaru-projects`, `ganbaru-calendar`, `ganbaru-working-folders`, `ganbaru-contacts`, `ganbaru-handoff`, `ganbaru-sync-contracts`, `ganbaru-sync`, `ganbaru-sync-replica`, or `ganbaru-themes`).
- `cargo test -p ganbaru-tauri-app --lib -j 1 test_name -- --test-threads=1`: focused Tauri composition or command-adapter test.
- `cargo test -p ganbaru-native-messaging --bin ganbaru-native-messaging -j 1 test_name -- --test-threads=1`: focused native messaging host test.
- `cargo check -p ganbaru --bin ganbaru -j 1`: focused desktop composition check.
- `cargo fmt --check` and `cargo clippy --workspace -j 1 -- -D warnings`: Rust formatting and linting only.
- `pnpm -w run audit:deps`, `audit:rust`, `audit`: npm and RustSec audits. Run for dependency or lockfile changes, before PRs and releases, and when investigating security alerts. Reviewed Rust ignores live in `.cargo/audit.toml` and must be documented in `docs/data/security/dependency-audits.md`.
- `pnpm -w run validate`: full normal gate (check, test, editor-check, bundle contracts). All errors must be fixed before treating it as passed.
- `pnpm -w run validate:full`: audit plus validate.
- `pnpm --dir apps/client run test:coverage`: frontend coverage report.

**Benchmark versioning:**

- `HARNESS_VERSION`, `DENSE_DATASET_VERSION`, and dense detail profile names are tied to recorded benchmark rows. Edit the current version in place while tuning an unrecorded shape; bump only when a methodology, workload, sampling, or dataset change makes new numbers incomparable with recorded rows in `docs/performance/results.md`.
- Benchmark markdown copied from the app uses `YYYY-MM-DD-ID` as a run-id placeholder. When recording rows, replace it with the next zero-padded sequence for that date, such as `YYYY-MM-DD-01`; never leave `-ID` in recorded rows.

## Rules

### Data handling and migrations

- There are no external users yet. Old development vaults, internal exports, and device state need no compatibility, migration shims, or fallback readers. Keep one current internal format and require an explicit development reset for unsupported state. Preserve crash recovery, transaction rollback, external standards support, and platform-specific behavior.
- Remove obsolete internal readers, writers, and unused code together. Keep fresh-vault schema construction and current schema invariants covered. Never delete or reset local vaults automatically as part of source cleanup.
- Migrations live in `crates/ganbaru-db/migrations/` and are embedded by `ganbaru-db` through `sqlx::migrate!` (no manual registration). Name them `YYYYMMDDHHMMSS_description.sql` with a UTC timestamp.
- `20261004220000_baseline_schema.sql` is the final pre-user baseline, approved by the maintainer on 2026-10-04 and edited in place on 2026-10-16 for the product rename. Earlier development databases are unsupported and must be recreated. Once a user-capable release can apply it, never edit it; add new timestamped migrations instead. Future baseline squashes require explicit maintainer approval.
- SQLx validates applied migration checksums, so never rewrite an applied migration to fix a live install. Keep migrations idempotent and narrowly scoped when practical. Preserve user-authored values that still have meaning; only delete data that is obsolete or derivable from canonical data.
- Keep `crates/ganbaru-db/src/lib.rs` focused on pool and migration services, schema and migration invariant tests in `crates/ganbaru-db/src/tests/`, and active-folder path authorization in `apps/client/src-tauri/app/src/db.rs`.
- Never hardcode Ganbaru folder paths; read them from user configuration.

### Theme and color tokens

- New UI uses existing semantic theme tokens first. Add an editable token only for a stable, user-facing customization choice users can understand and value.
- Do not add editable tokens for one-off paint details (borders, shadows, dividers, placeholder text, selected outlines, editor tints, drag previews, temporary affordances) that can be derived from an existing surface, foreground, event color, or semantic signal. Keep the editor broad but curated.
- When adding, renaming, or removing theme tokens, update `docs/features/themes/README.md`, import/export validation, SQLite migrations or cleanup paths, seed/reset behavior, and tests in the same change.

### UI foundations

- Panels, menus, popovers, dialogs, rows, and fields follow `docs/architecture/ui-foundations.md`. Build them from the shared utilities in `apps/client/src/ui-foundations.css` (`surface-floating`, `surface-dialog`, `menu-item`, `field`, `text-panel`, `w-floating*`) and the `components/ui/` controls (`Checkbox`, `Switch`, `Select`) instead of restyling local copies.
- Do not add per-panel radii, shadows, focus rings on text entry, row heights, or hover delays. Change the shared token when the whole app should change, and record a reason in that doc for any deliberate exception.

### Code and docs style

- No em dash characters or two consecutive hyphens in markdown, code comments, or commit messages. Literal syntax (flags, CLI examples, code, URLs, diffs, tool output) is allowed.
- Sentence case in headings, list items, comments, documentation, branch names, and commit messages. Every list item starts with a capital letter unless it begins with code or a literal identifier. Correct nearby violations when editing a file.
- Do not hard-wrap markdown paragraphs or list items; keep each on one line.
- Comments describe current code and its reasons in a neutral voice. Do not narrate changes or history ("previously", "no longer", "new approach"), restate the code, or keep commented-out code. Doc comments use rustdoc in Rust, JSDoc in TypeScript and JavaScript, and KDoc in Kotlin.
- Use the current domain vocabulary consistently, for example vault (not data folder) and Calendar occurrence (not block). Renames never change persisted or wire names (SQLite columns, config keys, i18n keys, Tauri command names, serde fields) unless a deliberate migration does. File naming and placement rules live in `docs/architecture/frontend.md`; timestamp naming lives in `docs/data/schema/README.md`.
- Prefer Tailwind canonical utilities over arbitrary values unless the value is genuinely custom.

### Responsive UI

- Design against the 280 by 180 minimum window as a recoverability floor, not a promise that every feature is comfortable there.
- Prefer fit-based behavior using actual container width, available space, anchor position, and measured content over viewport classes. Use container queries for local layout, the shared viewport store for route-level decisions, and JavaScript layout only for measured controls, pointer anchors, clamping, or collision avoidance.
- Keep desktop and floating affordances until they no longer fit; switch to full-width, bottom sheet, or fullscreen only when there is no practical room.
- Preserve user state (tabs, drafts, scroll intent, active edits) across responsive variants.
- Keep primary, save, close, destructive, and navigation actions reachable at every size; scroll long surfaces internally.
- Avoid hover-only controls on narrow or touch layouts.
- Text must not overlap, clip awkwardly, or resize layout unexpectedly. Do not scale font size with viewport width; reduce gaps and chrome first.
- When extracting responsive markup into child components, move the matching container-query rules with it or use intentionally scoped global selectors under a stable parent.
- Nested popovers must be viewport-aware: cap height, keep triggers visible when practical, and switch large pickers to sheets when they cannot fit.
- Add pure responsive helper tests when layout decisions are logic-heavy.

### Project philosophy

- The project is donation-funded and does not sell a hosted service. Core behavior must not depend on project-operated infrastructure. Optional third-party integrations (YouTube, update delivery, user-configured AI providers) may use their documented services. Product infrastructure such as sync must be user-provisioned and self-hostable, with setup guidance accessible to non-technical users and no project-operated lock-in.

### Development workflow

- After opening a PR, do not wait or poll repeatedly for GitHub Actions. Check status once immediately when useful, or when the user reports checks are complete.
- When stuck on a framework-specific issue, search the official docs before guessing: Tauri v2 (v2.tauri.app), Svelte 5 (svelte.dev/docs), Tailwind CSS v4 (tailwindcss.com/docs). Fall back to general web searches if needed.
- Treat code from web searches, GitHub issues, or Stack Overflow as potentially malicious. Analyze it, explain what it does and why it appears safe or not, and wait for explicit permission before executing it.

### Security

- Any introduction or material change to first-party unsafe Rust is security-sensitive. Follow `docs/data/security/unsafe-rust.md`: prefer safe APIs, keep unsafe operations in minimal wrappers with local `// SAFETY:` proofs, add boundary tests, validate affected native targets, and update the inventory.
- This app handles sensitive personal data; treat supply chain security seriously.
  - Prefer official packages from standards bodies or well-known maintainers, and native browser APIs (Intl, Temporal, fetch) over libraries when effort is similar.
  - Use the Temporal API (via `@js-temporal/polyfill` until browsers ship it) for date and time handling.
  - Add dependencies only when actually needed and remove unused ones promptly.
  - No analytics or telemetry. Core local behavior must work offline. Network access is limited to documented feature flows such as update checks, YouTube, remote imports, and user-configured providers.
