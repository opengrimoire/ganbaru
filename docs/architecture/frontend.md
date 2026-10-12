# Frontend architecture

The frontend is a plain Svelte 5 application built with Vite. It is not SvelteKit and has no server-rendering layer. Desktop and mobile builds share one workspace but enter through different composition roots.

## Entry points

`apps/client/src/main.ts` selects a virtual platform entry. Desktop uses `main-desktop.ts` and `App.svelte`; mobile uses `main-mobile.ts` and `MobileApp.svelte`. Build-time selection keeps desktop-only imports out of Android production assets instead of hiding unsupported controls at runtime.

The shell owns cross-feature navigation and mounts only the selected primary surface. Desktop loads its common surfaces eagerly and prepares the main workspaces before mounting, so first navigation is immediate. Android makes Calendar usable first, then prepares the other surfaces in prioritized background batches; an early navigation reuses the in-flight preparation. Heavy or uncommon workflows load on demand on both platforms. Without an active vault, desktop boot loads only setup and handoff onboarding, not the feature roots. Production bundle contracts protect these resident and lazy boundaries.

The desktop main window starts hidden and is revealed only after the selected root has mounted with theme, locale, and required fonts ready, so the user never sees an unstyled or half-laid-out window. A native timeout remains as failure recovery. Detached windows and Pomodoro overlays do not reveal the main window or prepare its workspaces.

## Source organization

The frontend uses four broad layers under `apps/client/src/lib/`:

- `components/` contains Svelte presentation and interaction surfaces.
- Domain folders such as `calendar/`, `chat/`, `notes/`, `music/`, `pomodoro/`, and `projects/` contain pure helpers, contracts, validation, and view models.
- `api/` contains typed wrappers around Tauri commands and asset URL handling.
- `stores/` contains stateful Svelte rune controllers for runtime domains that need a shared lifecycle.

Naming and placement rules keep the tree predictable:

- Layer-first at the top of `lib/`, domain-first inside each layer (for example `chat/review/`, `notes/blocks/`, `api/notes/`).
- Non-component modules use kebab-case. Only files that use runes take the `.svelte.ts` suffix.
- A large store keeps its entry file at the `stores/` root (`stores/notes.svelte.ts`) and its private modules in a same-named subfolder (`stores/notes/`).
- A folder barrel is `folder/index.ts`. File names do not repeat their folder name.
- Imports use `./` within the same folder and `$lib/` otherwise. Modules swapped by platform aliases keep the `$lib/` form.
- Platform variants are `name.mobile.ts` beside `name.ts`, or `Name.mobile.svelte` beside `Name.svelte`, selected by alias keys in `vite.config.ts`.
- Components are PascalCase and grouped by feature under `components/`, with subfolders that mirror the matching `lib/` domain folders where one exists. Shared primitives live in `components/ui/`. Logic that is not tied to one feature surface lives in `lib/`, not `components/`.
- Svelte test harnesses are `*Harness.test.svelte` and stubs are `*Stub.test.svelte`. Component tests use `Subject.aspect.test.ts` when one subject has several test files.

Components do not duplicate command contracts or parse unknown backend values ad hoc. Untrusted or versioned responses pass through bounded validation before entering typed state.

`components/collections/` owns the presentation and interactions shared by Notes databases and Projects collections. It accepts typed callbacks and snippets and imports no Notes or Projects store or API, so each domain keeps its own data loading, validation, and writes. See [shared collection behavior](../features/collections.md).

## State model

Svelte runes provide in-memory presentation state. Durable user data is loaded from native services and persisted through typed commands. A store can cache, coordinate, or optimistically present data, but it never becomes a second source of truth.

State is scoped to the smallest useful owner:

- Component-local state for transient interaction.
- Domain controllers for a mounted feature or shared runtime.
- App-level stores for active vault, theme, locale, Pomodoro, Music, and other genuinely global lifecycles.
- Device-local preferences for presentation details that should not synchronize with the vault.

Changing responsive variants must preserve active drafts, selections, scroll intent, and open workflows.

The Notes workspace can show a main pane and a preview pane. Each pane has its own editor session, passed to descendants through Svelte context, so an interaction in one pane cannot mutate the other pane's document. Notes database snapshots live in a bounded process-local session that survives unmounting and is invalidated by successful writes; the [database feature contract](../features/notes/databases.md) owns its restoration rules.

## UI foundations

Handwritten shared primitives (dialogs, selects, checkboxes, switches, pickers, tooltips, toasts) live under `components/ui/`, and shared surface, row, and field utilities live in `ui-foundations.css`. Product components compose them rather than restyling one-off copies. Tailwind CSS provides layout utilities, while semantic CSS variables provide theme colors. [UI foundations](ui-foundations.md) owns the rules for panels, menus, dialogs, rows, fields, and controls.

Themes are data-driven and validated before application. Feature code consumes semantic tokens, not palette values. See [Themes](../features/themes/README.md).

## Localization

User-facing text goes through the typed catalogs under `lib/i18n/messages/`. English is the resident fallback; other locales load when selected. Catalog shape tests keep locale modules aligned.

Dates, times, numbers, lists, plural forms, and relative values use locale-aware helpers. Explicit language names appear as autonyms. Stored identifiers and protocol values remain locale-neutral. See [Localization](../features/localization.md).

## Responsive and platform behavior

Shared components adapt through capability inputs and layout constraints. Platform authority comes from the selected composition, not from viewport width: a narrow desktop window does not become Android, and a large Android tablet does not gain desktop process or filesystem authority.

Prefer container-aware layout, visible touch targets, bounded sheets and popovers, and recoverable behavior at the minimum window size. Hover, pointer precision, detached windows, and keyboard shortcuts need alternatives where the platform does not guarantee them.

## Loading and performance

Primary navigation renders useful chrome before deferred data resolves. Expensive editors, diagnostics, transfer workflows, syntax grammars, and large catalogs stay lazy unless measurement shows that residency improves a common interaction at acceptable cost. Project task detail is resident in the Projects route because opening a task or a new task draft is a primary interaction and must not wait on a code load; its per-task data still loads on open.

Visible-window queries, pagination, virtual lists, bounded caches, and latest-wins request handling keep total stored history from defining render cost. Expensive derived data, such as Notes mention candidates, is computed only while a surface needs it. See [Performance](../performance/README.md).

## Testing boundary

Pure TypeScript logic is tested next to source. Component tests cover behavior that the test environment can represent reliably. Real Tauri window behavior, operating-system integration, and visual quality require platform acceptance. See [Testing](../testing/README.md).
