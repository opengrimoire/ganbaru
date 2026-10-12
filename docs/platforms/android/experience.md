# Android experience

## Adaptive shell

Phones use a compact global top bar for primary destinations and utility surfaces. Larger windows can use a navigation rail. Calendar, Projects, Notes, and Chat are primary destinations. Pomodoro, Quick notes, Music, and Settings open as contextual surfaces without adding another permanent bar.

The shell uses touch-sized controls, visible selection, localized accessible names, system safe areas, and the current theme. It does not spend scarce phone width repeating the active destination label.

Startup shows a single loading indicator until the database, Calendar, Pomodoro recovery, and the Pomodoro and linked-device controls are ready. Once Calendar paints, the remaining destinations are prepared in prioritized background batches rather than on first interaction, so opening them later rarely waits. Heavy detail workflows stay lazy only where desktop also defers them.

## Hierarchical navigation

Projects, Notes, and Chat share one compact identity row. On phones, selecting a breadcrumb level opens a full-height touch selector for that level instead of reproducing adjacent desktop sidebars. Only one hierarchy level is visible at a time, and Back moves toward the group root before dismissing the selector. Opening navigation or switching a channel does not summon the keyboard until the user selects Search or the composer.

## Feature surfaces

- **Calendar** starts in day view with direct touch navigation and editing. Gestures do not conflict with event drag or resize, and active-event protections match desktop.
- **Projects** starts in List and reuses the canonical List, Dashboard, Kanban, Calendar, and Gantt views. Wide views pan natively in both axes. Selection and task-detail controls stay visible on coarse pointers instead of depending on hover. Filters, sorting, columns, grouping, and settings open in touch-sized sheets.
- **Notes** reuses the page, block, database, history, and navigation contracts. Working-folder Markdown controls are absent, and managed images and files use bounded document-input flows.
- **Chat** reuses the responsive canonical workspace. Channel navigation replaces the conversation while open instead of covering it with a desktop overlay. Provider setup, terminal, files, Git, local review workspace, and execution diagnostics are absent.
- **Pomodoro, Quick notes, and the compact Music player** float above the active destination within the visible safe viewport. The Music library builder expands to a full-screen workspace.
- **Settings** keeps the desktop category structure where a shared preference or Android equivalent exists and omits unsupported controls, including keyboard shortcuts. Anti-distraction settings include shared usage limits, Android selected-app rules and access status, and read-only browser and desktop configuration from the portable vault.

## Linking with a desktop

The linking protocol, ownership rules, and data protections are owned by [device linking and synchronization](../../data/sync.md). Android adds these interaction rules:

- First use starts with a localized welcome screen (license summary, source link, full AGPL 3.0 text, and acknowledgments), then creates or restores the private vault, completes the Android access review, and shows a full-screen desktop-linking scanner. A missing or invalid configured vault leads to vault recovery instead.
- The scanner uses the rear camera, requests camera access in context, and decodes the desktop's QR invitation, which carries the private-LAN endpoint and pinned coordinator identity. The user never types an address or configures a server.
- An untouched first-run vault is replaced directly by the initial read-only desktop snapshot. **Not now** finishes onboarding with an independent vault and permanently disables that shortcut. Closing the scanner without choosing either leaves onboarding unfinished.
- A linked-device control placed after Pomodoro shows the writable or read-only role and opens transfer, refresh, unlink, and recovery actions. With no relationship it opens the same scanner. Data settings is the fallback management surface.
- When the phone opens or becomes read-only, a focused prompt names the current main device and offers `Use on this device` or `Continue in read-only`. `Use on this device` is the only normal action that requests write ownership. A read-only vault remains browsable through the ordinary shell.
- Emergency local-copy recovery appears only in Data settings, after membership is lost while the local copy is read-only.

Scheduled Calendar alarms and the last accepted distraction rules keep working while the phone is offline or backgrounded. Desktop Calendar changes reach Android notification scheduling only after a successful read-only refresh.

## Insets, keyboard, and input

A native bridge publishes system-bar and display-cutout insets, with CSS safe-area values as a fallback. Layout reacts to orientation, navigation mode, cutouts, and window changes without hardcoded device dimensions. Controls stay inside the visible viewport.

Visual viewport tracking keeps focused inputs and editor actions visible while the keyboard opens, resizes, or rotates. Opening navigation does not focus Search automatically, and dismissing the keyboard does not leave the current feature.

## Android Back

A serialized frontend Back controller intercepts only when the UI has consumable state: dialogs, menus, pickers, full-screen builders, navigation levels, editor layers, task details, Notes contextual pages, and utility sheets. The topmost layer closes first. At a destination root the listener yields, so Back follows native Android behavior. Two layers cannot consume the same press, and a stale closed layer cannot trap the app.

Predictive Back still needs validation on newer devices; the state model must stay compatible without inventing a separate navigation history.

## Accessibility

Touch controls expose localized names, selected and expanded state, and sufficient target size. Keyboard and switch-access users can reach navigation, overlays, hierarchy selectors, editing, Save, Cancel, and recovery actions. Motion is restrained and respects user preferences. Status is never communicated by color or animation alone.
