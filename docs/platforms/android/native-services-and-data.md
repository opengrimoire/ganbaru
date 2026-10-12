# Android native services and data

## Application-private vault

Android creates the active vault in application-private storage. The user cannot choose another live-vault path because Android document providers do not offer a safe writable folder with desktop semantics. The displayed data location is informational; reveal-in-file-manager and live-folder switching are absent.

First use states clearly that uninstall can remove this data and offers `Start from zero`, import of a complete Ganbaru AI folder, or restore of a portable Android backup.

## Folder import

Import uses the system directory-tree picker and streams the selected folder into private staging. It validates the vault marker, paths, depth, entry count, expanded size, and database before atomic activation. The external folder never becomes the live vault and is never edited in place.

## Portable backup and restore

Backup writes a consistent SQLite snapshot and bounded archive to Downloads without passing file bytes through JavaScript. Live WAL and SHM files are excluded.

Restore uses the system document picker, extracts into private staging with path and size limits, validates the vault and SQLite integrity, closes active database access, and atomically activates the restored folder with rollback on failure.

The portable format is the recovery path after reinstall. Android system backup and device transfer exclude the unencrypted live vault until a deliberate encrypted design exists.

## Linked desktop handoff

The handoff protocol is owned by [device linking and synchronization](../../data/sync.md). On Android, device identity, the pinned coordinator fingerprint, ownership generation, transfer state, and staging live in application-private platform state and are never part of the transferred vault. Incoming vaults use the same archive validator and atomic activation path as portable restore. Before the first ownership activation replaces an independent vault, Android exports it to Downloads and also keeps a private rollback copy; backup failure aborts the transfer.

After activation, startup reconciles Calendar notifications and anti-distraction projections. Device-local alarms, native enforcement, Media3 state, permissions, and document-tree grants are not replaced by the incoming vault.

## Lifecycle and process death

Visibility and page lifecycle hooks flush configuration, Notes, and Quick notes as a best effort. Canonical writes remain responsible for correctness because Android can kill the process without a final callback.

Startup validates the active vault and reconciles domain state. Malformed, duplicated, expired, or unsupported persisted state closes with a typed reason instead of becoming active silently.

Activity removal does not end Pomodoro, stop Media3 playback, or disable selected-app enforcement. Their native services own those lifecycles explicitly.

## Calendar notifications

Calendar keeps a bounded rolling native projection derived from SQLite events, reminders, recurrence, exclusions, and overrides. Startup, event mutation, Activity resume, reboot, package replacement, and clock or timezone changes reconcile it. The projection is rebuildable device state, not a second event database. Each delivery uses a stable domain identifier and the Calendar notification channel, and taps are validated before opening Calendar.

When permission, channel settings, or exact-alarm access block delivery, the event panel explains the problem and opens the matching system setting. The app never bypasses silent mode, Do Not Disturb, or channel controls with media playback.

## Pomodoro service and alarms

The execution model is owned by [Pomodoro](../../features/pomodoro/README.md) and [Focus authority](../../algorithms/pomodoro/focus-authority.md). Android-specific rules:

- An explicitly started run owns a special-use foreground service with a silent ongoing progress notification.
- Native state describes only the currently accepted phase. Its alarm delivers a boundary reminder and ends the projection; it never advances to later phases while the WebView is absent.
- The Rust Focus owner publishes and revokes phase projections. The WebView cannot publish or cancel them and supplies only localized copy, which is never recovery evidence.
- Opening the app runs `ganbaru-pomodoro` recovery against committed SQLite state. Notification projections cannot create a run or phases.
- A device-local delivery receipt suppresses repeated alerts. It is presentation state, never execution evidence.
- Calendar commitment reminders are separate. A due event while the Activity is absent creates no run, focus minutes, or phase-dependent blocking; the user starts it from the explicit **Start scheduled session** action.

Manufacturer task cleaners can still override standard behavior. The app offers truthful autostart and battery-setting guidance without claiming it can grant those controls.

## Music service

Local audio uses a Media3 session service and ExoPlayer, which own background playback, audio focus, the media notification, and lock-screen, headset, and system controls. Sources use a retained document-tree permission and stable tree-relative identities. Scanning and metadata work stay off the main thread and within explicit bounds. Library, playlist, assignment, and playback state remain canonical in SQLite.

## Anti-distraction guardian

Selected-app rules run in a private `:guardian` process that holds alarm handling, ongoing notification coordination, Usage Access reads, the opt-in Accessibility Service, validated rule projections, and a bounded native journal. Only the guardian opens its private preferences and journal database, and non-exported providers validate rule projections and journal imports.

The current owner writes normalized usage and block history to canonical SQLite. A non-owner keeps usage in the private journal and exchanges stable sample IDs through the authenticated coordinator, as described in [device linking](../../data/sync.md). User configuration remains in the active vault `config.json`.

One serialized Rust publisher synchronizes the journal and derives budgets from persisted configuration. The guardian captures its counter baseline and pending journal under one lock so usage is never counted twice. The frontend supplies only localized copy and reads cached budgets; it has no accounting or enforcement authority. Configuration, vault, handoff, and failed-publication boundaries revoke obsolete rules. If the application process exits, the guardian keeps enforcing its last accepted local policy.

## Managed files

Profile images, project icons, custom emoji, Notes icons, covers, attachments, and CSV input use system document selection and bounded managed-asset writes. Declared and sniffed MIME type, size, dimensions where relevant, digest, and destination are validated before canonical references are stored. Remote Notes images are not rendered directly under the current content policy; a future ingestion flow can download and validate them into managed storage.

## Deferrable work

WorkManager is appropriate for deferrable maintenance and future sync. It must not own Pomodoro deadlines, Calendar reminders, active Music playback, or real-time selected-app enforcement.
