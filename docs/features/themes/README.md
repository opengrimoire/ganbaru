# Themes

Themes define Ganbaru's application shell, Calendar surfaces, event palette, and contrast behavior through a validated registry. A theme is not a light/dark boolean.

## Current scope

| Capability | Status |
| --- | --- |
| Immutable built-in light and dark themes | Implemented |
| User themes with full resolved snapshots and source palettes | Implemented |
| Import, export, duplicate, edit, seed reset, and engine rebake | Implemented |
| Stable 32-slot event palette | Implemented |
| Live preview, token isolation, contrast warnings, and responsive editor | Implemented |
| Font family and scale preferences | Implemented |
| Replication of user themes between linked devices, with conflict choice | Implemented |

## Principles

- Stored event colors use stable slot identities, not copied hex values.
- Paint uses stored resolved snapshots, so engine upgrades never silently change a user theme.
- Source edits derive coherent token families without erasing isolated (user-pinned) values.
- Effective canvas luminance, not the theme's name or label, decides runtime light or dark treatment.
- Contrast feedback is visible and honest; the app never certifies an inaccessible palette.
- Imported themes cannot shadow built-in identities.
- Save and Cancel have complete snapshot semantics.

## Theme model

A theme contains:

- Stable identity and display name.
- Resolved app and Calendar token snapshots.
- Source colors used by the [color engine](color-engine.md).
- Per-token isolation flags.
- A 32-slot event palette plus fallback slot and blend canvas.
- Calendar default basis.
- Derivation engine version.

## Token catalog

The editable catalog is defined by `APP_TOKEN_KEYS` and `CALENDAR_TOKEN_KEYS` in `apps/client/src/lib/themes/definitions.ts`, currently 35 app tokens and 7 Calendar tokens. Token identities are the compatibility contract; the counts are only a current fact.

User themes store every editable token as a resolved hex snapshot. Runtime-only implementation colors (hover tints, borders, and similar paint details) are derived from stored values and are never exported or shown as editable rows. Add a new editable token only for a stable, user-meaningful color decision; see the theme token rules in `AGENTS.md`.

Adding, renaming, or removing an editable token requires, in the same change:

- A value for each built-in theme.
- Upgrade behavior for stored user themes and their seeds, with an explicit drop or mapping rule so dead values do not linger.
- Import and export validation.
- Editor placement.
- SQLite migration or cleanup.
- Tests, and an update to this section.

## Event palette

Every theme owns exactly 32 event-color slots with stable numeric identity. Calendar events and Quick notes store the slot index, so switching themes recolors them without rewriting records. Unknown slot values fall back to the theme's fallback slot. Reordering visible color controls never renumbers persisted slots. The blend canvas controls dimmed event variants and normally follows the Calendar canvas unless isolated.

## Persistence

User themes and their seed snapshots are normalized SQLite records. Built-in light and dark themes are code-pinned, never stored as rows, and their reserved IDs cannot be inserted by import or persistence. At startup, a device without a saved active theme, or whose saved theme no longer exists, uses the quick-toggle theme matching the system color scheme, without saving it. Deleting or discarding the active theme falls back to the built-in dark theme on the deleting device. Native window and WebView backgrounds follow the active theme surface so no white frame shows before the page paints; Android keeps the last surface for the next launch and, on Android 12 and later, the theme mode for the system launch splash.

The quick-toggle light and dark choices are portable preferences that follow the vault. The active theme ID, font family, and font scale are device preferences, so switching to dark mode on one device never repaints another. Theme content never lives in preferences. Platform bootstraps load configuration and hydrate user themes before mounting the app shell.

A user theme keeps a seed snapshot for `Reset all to seed`. The seed is fixed when the theme is created: duplicating creates a new independent seed, import uses the imported content as its seed, and saving, rebaking, or resetting never changes it. Seeds are not exported.

## Linked devices

User themes replicate between linked devices; built-in themes never do. User theme IDs are random UUIDs, so themes created on different devices never collide. Name and icon edits resolve to the latest edit. A theme's content (tokens, event palette, blend canvas, Calendar defaults, and engine version) merges as one unit: when two devices save the same theme concurrently, both versions are kept, the list marks the theme, and its editor shows each version with its device and time so the person picks one, which applies on every device. To keep both, duplicate the theme before picking. A theme deleted on one device while another edited it becomes a recovery entry in the sync settings, and restoring it creates a new theme from the latest edit, which is also its seed. A dismissed engine upgrade stays dismissed on every device. When a linked device deletes the active theme, the device falls back as it does at startup. An open editor keeps its buffer while synced changes arrive and reloads the theme when the session ends. See [Sync](../../data/sync.md).

## Import and export

Export is deterministic, versioned JSON (`schemaVersion: 1`) with identity, snapshots, sources, isolation, event palette, Calendar defaults, blend canvas, and engine version, without device-local selection preferences.

Import validates the whole object before registering it and never partially applies rows. Missing required values, invalid colors, malformed slots, or an unsupported schema version fail with a list of all problems. Unknown token keys are dropped so a stale token name does not block an otherwise valid theme. An imported theme always gets a fresh ID, and a display name that is already taken gets a numeric suffix, so importing the same file twice keeps both copies. Themes from an older engine paint from their snapshots and are offered a rebake.

## Typography and density

Font family and font scale are Appearance preferences, not theme tokens. They apply alongside the active theme but keep their own device preference identity, so changing themes does not change reading comfort.

Layout density is not a theme dimension. A future density system needs explicit component and accessibility contracts rather than arbitrary spacing tokens in imported color themes.

## Documentation map

- [Color engine](color-engine.md)
- [Theme authoring](authoring.md)
- [Calendar event colors](../calendar/README.md)
- [Data architecture](../../data/architecture.md)
