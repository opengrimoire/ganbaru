# Android experience

## Adaptive shell

Phones use a labeled bottom bar within thumb reach for the open [workspace tabs](../../features/workspace-tabs.md), and a slim top bar. Its leading side holds the active destination's context switcher, or, where there is no context to switch (Calendar), the destination icon and name in the same pill without the open affordance; its trailing side holds the utilities: a Pomodoro ring that adds the remaining time while a session runs, then linked devices, Quick notes, Music, and Settings. The bottom bar hides while the keyboard is open so editing keeps the space. Larger windows use a navigation rail instead of the bottom bar. Both bars share the content surface color, so a screen reads as one page rather than stacked bands.

The shell uses touch-sized controls, visible selection, localized accessible names, system safe areas, and the current theme. It does not spend scarce phone width repeating the active destination label.

Loading indicators sit on the content surface color. Startup shows a single loading indicator until the database, Calendar, Pomodoro recovery, and the Pomodoro and linked-device controls are ready. Once Calendar paints, the remaining destinations are prepared in prioritized background batches rather than on first interaction, so opening them later rarely waits. Heavy detail workflows stay lazy only where desktop also defers them.

## Hierarchical navigation

Projects, Notes, and Chat share one context model. On phones, the top bar shows a single switcher with the current project, its icon, and an open affordance, because a full group, project, and note path cannot stay legible at phone width. Selecting it opens a full-height touch selector at the current level: the open note's siblings in Notes, the project's channels in Chat, or the group's projects otherwise. The Chat action row therefore does not repeat the channel name. Only one hierarchy level is visible at a time, and Back moves toward the group root before dismissing the selector. The row below the top bar holds only actions for the current view, such as the sidebar entry, Back from an open note, the open database or Chat channel, and page, view, or project settings actions, and it collapses when it has none. Creation lives in the sidebars and views rather than this row, except that the Projects view switcher, which names the open view and lists the project's views like the Calendar span button, ends its menu with an Add view row that expands the view types in place, because switching views is frequent and otherwise needs the sidebar. A project's Notes and Chat views open inside the Projects tab as the phone Notes and Chat workspaces scoped to that project, with the same switcher in their action row to return to another view. All action rows share the top bar icon columns, so their buttons line up under the top bar buttons from either edge. Projects, Notes, and Chat open their sidebar from a sidebar button in the action row as a floating panel shaped like the selector, with a title, close, search, and touch rows over the current view; the backdrop and Android Back close it. Holding a sidebar row still lifts it for the same drag reordering as desktop, while a swipe scrolls. Projects also rename, reorder, and remove views there. With no note open, Notes shows the same explorer rows in place as its landing level. Opening navigation or switching a channel does not summon the keyboard until the user selects Search or the composer.

## Feature surfaces

- **Calendar** starts in day view with direct touch navigation and editing. Gestures do not conflict with event drag or resize, and active-event protections match desktop.
- **Projects** starts in List and reuses the canonical List, Dashboard, Kanban, Calendar, and Gantt views. Wide views pan natively in both axes. Selection and task-detail controls stay visible on coarse pointers instead of depending on hover. Filters, sorting, columns, grouping, and settings open in touch-sized sheets.
- **Notes** reuses the page, block, database, history, and navigation contracts. Working-folder Markdown controls are absent, and managed images and files use bounded document-input flows.
- **Chat** reuses the responsive canonical workspace. Channel navigation replaces the conversation while open instead of covering it with a desktop overlay. Provider setup, terminal, files, Git, local review workspace, and execution diagnostics are absent. Message actions (reactions, copy, and more) appear when the user holds a message still, since touch has no hover, and a tap elsewhere, a scroll, or Back hides them. Holding a message therefore does not start a native text selection; Copy copies the whole message.
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

The app draws edge to edge, so the native shell resizes the WebView above the soft keyboard itself and reports that the keyboard is open. The page therefore lays out in the space the keyboard leaves, the way native apps do: bottom-anchored controls such as the Chat composer sit directly above the keyboard, the bottom tab bar hides, and the focused field and its caret stay visible while the keyboard opens, resizes, or rotates. On Android 11 and later the page resizes once the keyboard finishes sliding in, so no empty band shows above the moving keyboard. Dismissing the keyboard does not leave the current feature.

The keyboard opens only when the user asks to type: by selecting a field, or through an action whose purpose is typing, such as creating a page, event, or item, renaming, or Search. Switching destinations, opening a page, note, channel, or task, restoring a previous caret, and loading data never focus a text field on a phone.

## Android Back

A serialized frontend Back controller intercepts only when the UI has consumable state: dialogs, menus, pickers, full-screen builders, navigation levels, editor layers, task details, Notes contextual pages, and utility sheets. The topmost layer closes first. With no layer open, Back returns to the first workspace tab, and on that tab the listener yields, so Back follows native Android behavior. Two layers cannot consume the same press, and a stale closed layer cannot trap the app.

Predictive Back still needs validation on newer devices; the state model must stay compatible without inventing a separate navigation history.

## Accessibility

Touch controls expose localized names, selected and expanded state, and sufficient target size. Keyboard and switch-access users can reach navigation, overlays, hierarchy selectors, editing, Save, Cancel, and recovery actions. Motion is restrained and respects user preferences. Status is never communicated by color or animation alone.
