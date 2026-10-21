# Application security boundaries

Tauri webviews are presentation and orchestration clients. Rust owns database access, filesystem paths, native dialogs, imports, exports, process control, credentials, notifications, media hosts, application blocking, and other operating-system effects.

## Tauri capabilities

Capabilities grant only the plugin operations a window or platform needs. Neither desktop nor Android exposes generic filesystem or shell plugins to the webview. Desktop exposes scoped window, event, and zoom operations; Android exposes notification permission checks and the repository-owned mobile plugin commands.

Application commands are also ACL-checked, so a webview can call only the commands its capability allows and an unlisted command fails closed. `apps/client/src-tauri/permissions/app-commands.toml` defines two command sets: every registered command for the app-owned main and tab webviews, and a small Focus set for overlay and blocker windows. Desktop capabilities select webviews by label, never the main window, so Chat preview webviews attached to it receive no command or plugin grant. Functions that only Rust calls, such as Chat browser tools for the internal MCP endpoint, are not registered as commands. Runtime tests keep the permission file equal to the registered handlers, and a frontend test keeps the Focus set equal to what overlay code can reach.

No webview may create windows or webviews. Detaching a tab asks Rust to open a tab window, and Rust accepts only a `view-` label, the app root with the tab query, a bounded position, and an opaque background color.

URL opening is allowlisted on both platforms to specific product destinations (the repository, releases, license, icon and sound attributions, and the Notion authorization guide on desktop) rather than arbitrary web addresses. Feature-owned native commands, such as Notes hyperlink opening, parse and check their own URLs before delegation and do not widen this allowlist.

Capability files in `apps/client/src-tauri/capabilities/` are authoritative for exact grants. Do not copy the permission list into documentation.

## Content security policy

Production CSP runs only bundled scripts and permits Tauri IPC plus the sources current features need. Desktop exceptions: `http://127.0.0.1:*` image, media, and frame sources for the Music loopback service, whose port is chosen per launch and whose pages require a per-launch token, and `https://www.youtube-nocookie.com` frames for YouTube source previews in the Music builder. YouTube player scripts run only inside the loopback host page under that page's own policy. Chat previews are separate native webviews, not frames, so they need no CSP source. Android denies every frame source, hosts YouTube in a separate bridgeless native WebView (see [Android permissions and security](../../platforms/android/permissions-and-security.md#webview-and-bridge)), and additionally permits a connection to the GitHub releases API for its in-app update check. These are scoped exceptions, not a generic permission for remote scripts or frames.

Development CSP additionally supports Vite and hot reload, including `unsafe-eval`. On Android the Tauri CLI chooses the dev server address at launch, so the development policy accepts any host but only the fixed dev server and hot reload ports; image and media sources stay as open as the production policy. A development exception must not be copied into production configuration.

On desktop, app webviews stay on the app document. A navigation guard sends any main-frame page load outside the Tauri app origin (and the dev server in development builds) back to the last app URL that webview showed, and on Windows, where navigation requests report only the main frame, it refuses the navigation before it starts. Frames are not checked by this guard; the CSP frame sources above govern them. Chat preview webviews keep their own navigation policy. On Linux and macOS the foreign page can commit briefly before recovery, and it receives no command grant because capabilities match the app origin. Android has no guard yet. Default webview behavior already refuses new windows.

Inline styles are allowed for dynamic Svelte layout, theme, and calendar geometry. Inline scripts and object embedding remain prohibited unless a later feature receives a separate review.

## Native dialogs and document transfer

File import and export flows are Rust-owned. Desktop commands open native dialogs, apply extension and size constraints, validate the selected path, and return bounded parsed data or a narrow result. Import roots such as a Notion export folder come only from the native picker inside the same command, never from a webview-supplied path, and broad roots (filesystem root, home, the active vault) are rejected. The webview never receives a reusable filesystem grant.

Music is the exception that keeps paths in the webview, so Rust holds the authority instead. Music pickers record a grant in Rust when they return a folder or file, and a typed or pasted local file needs one native confirmation. Every desktop Music command that takes a path (artwork, embedded artwork, media hosting, probing, reveal, root binding, relink, item repair, and background audio) acts only on a path that such a grant or a root binding on this device covers. Refresh scans the folder bound natively for the root, whatever the request names. Coverage is lexical: `..` segments are rejected before any filesystem call, and symlinks the user placed inside a chosen folder keep working, which matches how libraries on mounted shares are commonly linked. Windows device namespaces are always rejected, and network shares are reachable only through a picker grant or root binding. Grants are kept in device-local app state with a cap that evicts the oldest first, so restored sessions and background audio keep playing after a restart. Background audio checks the file type with `stat` before opening it, and its commands run off the main thread with a bounded wait for the audio engine. Refusals never echo the path.

Android uses system document providers and MediaStore through repository-owned plugins. The native layer validates display names, MIME ambiguity, size, UTF-8 where required, and operation mode. It exposes selected content or a bounded export result, not a generic content URI or storage API.

User cancellation is an ordinary typed outcome, not a path or permission error.

## Vault and configuration

Vault selection validates the marker, schema version, directory relationship, permissions, and database availability before changing the active pointer. The application never treats an unrelated non-empty folder as a new vault and never silently deletes a damaged configured vault.

A vault database is untrusted input. Read-write opens (selection, import, and startup), restore and handoff validation, and the guarded sync open first compare its `sqlite_schema` with the catalog an empty database gets from the same applied migrations, and reject any extra, missing, or changed table, index, trigger, or view, including FTS5 shadow tables and a migration history that is not a table. Restore runs this check before the SQLite integrity check, so integrity checking never reaches virtual tables the file reshaped. Every connection disables `trusted_schema`, enables `cell_size_check`, and runs in SQLite defensive mode. Read-only opens skip the catalog comparison because a replica may read a newer schema; they rely on query-only access and those connection settings.

Portable config writes occur in Rust through validated patching and atomic replacement, so independent windows cannot rewrite the whole config from stale frontend copies. The `chat` branch is written only by native Chat commands, which parse and validate it as a typed structure, and generic patches into it are refused. Other branches belong to the frontend features that edit them; unknown keys there are inert, and every reader, native or frontend, validates the shape of the values it uses instead of trusting the file. Every reader of `config.json` and `vault.json` stops at a fixed size (4 MiB and 64 KiB), and a config write that would exceed the read limit is refused, so a saved configuration always reads back.

A vault can come from someone else, so its contents must not redirect app writes. Vault files, managed assets, and Notes, calendar, and theme exports are written through `ganbaru-atomic-files`: a randomly named temporary file created exclusively next to the target (which refuses any existing entry, a symlink included), synced, and renamed over the target, so a planted link at the temporary or final name is replaced rather than followed. Managed asset writes create their folders one level at a time below the vault root and refuse a symlinked folder on the way. Opening or selecting a vault refuses one whose `vault.json`, `config.json`, database or SQLite companion files, `assets/`, or `projects/` is a symlink, and one whose vault id leaves the ASCII letter, digit, `-`, `_`, and `.` alphabet of the handoff wire (160 bytes at most), because the id names key store accounts and key files. The vault root itself may be reached through a link the user made. App state, the vault marker, config, and the database with its SQLite companion files are owner-only on Unix, and an existing database is tightened when it opens for writing. Vault folders Ganbaru creates, restore and handoff staging, and database rollback copies are owner-only directories; a vault folder the user picked keeps its permissions, and a vault on a file system without Unix permissions still opens. Assets and exports keep the default permissions so other tools can read them. The desktop app config directory, which holds device identity, usage spools, and runtime snapshots, is kept owner-only on Unix at startup, and the browser host creates any missing directory the same way.

The active vault path is configuration, not a hardcoded Documents location. Managed child paths are derived below the validated active root.

Rust fixes which database each process serves when the frontend first asks: the vault, or on desktop the isolated benchmark database, which it selects only when the persisted benchmark state names a pending benchmark relaunch. The choice holds until the process exits, and the database a command names is an assertion checked against it, so a webview cannot switch databases mid-session or reach the other one. Rust-owned runtimes always open the vault, and benchmark fixture commands write only the benchmark database of a benchmark process.

## Managed assets

Profile images, project icons, Chat attachments, Notes assets, and browser artifacts live in feature-owned managed locations. Import validates source type, size, content signature where applicable, path identity, and symlink status before copying. Image, PDF, audio, and video types are detected from container signatures, not only extensions; a Notes file block records a media or PDF kind only when the signature matches the extension, and text attachments must be valid bounded text. Reads open the source once and check its type and size through that handle. Files the user chose in a native picker may be links, but import references and managed vault files are read only when the final path component is not a link.

Managed relative paths are stored only after normalization; an arbitrary external path cannot be persisted as a managed asset. Cleanup rechecks live references before deletion and records retry state if immediate removal is unsafe. Webview-requested deletions are limited to files the user is replacing: a project icon is deleted only when this process wrote it and no text column in the vault database mentions it, and the profile image command refuses the image the saved configuration names. Errors do not echo source absolute paths unnecessarily.

Quick note images are content-addressed: a file is named by the SHA-256 of its bytes and the extension of its detected type, and every read verifies the bytes against the name. Because Quick notes are writable on linked replicas, a guarded replica may also write this one directory, and nothing else outside SQLite: writes take the same managed write permit as owner writes, so a handoff fences them, and only validated images under `assets/quick-notes/images/` are accepted. A forged or damaged file therefore cannot pass as an image a note references. Images from other devices are written only after their bytes match the digest, type, and size of a reference. The coordinator accepts uploads only from authenticated linked devices, keeps at most one pending upload per device in memory and drops it after five idle minutes, and stores a file only after the same verification, so a linked device can at most add verified but unreferenced files, which cleanup removes after the grace period. Cleanup deletes only regular files that no note, open conflict, or recovery entry references and that are older than a day, so a file imported for an unsaved draft survives until the draft is saved. On Linux, where WebKitGTK leaves copied images out of paste events, the editor asks a desktop command to read the image on the system clipboard when a paste carries no text; the command reads only images, refuses sizes past the Quick note image limits before converting or importing them, and imports through the same validation into the active vault. Dropped files are imported the same way from the paths the operating system delivered with the drop, never from paths the webview supplies.

Image bytes cross IPC as raw request and response bodies rather than data URLs, and the webview shows them through `blob:` URLs, which the image CSP permits. Android cannot send raw request bodies, so its imports carry bounded base64 text that is size-checked before decoding.

## Notes content and history

Notes rich content, formulas, imports, transfer manifests, comments, and asset references are untrusted structured input. Rust validates types, sizes, nesting, IDs, graph relationships, and path boundaries before mutation.

Compressed history is validated before parsing: encoding, declared and actual decompressed size, content digest, and JSON shape, with per-chunk and reconstructed-size limits for chunked history. Restore applies canonical writes transactionally and rebuilds disposable indexes after success.

Markdown and HTML renderers sanitize or avoid active content. Export is not a route for executing embedded scripts or resolving arbitrary local paths. Chat Markdown escapes raw HTML before sanitization, and the sanitizer allows no `data-*` attribute except the renderer's own link marker. Readers of untrusted JSON keep keys such as `__proto__` as data and never change an object's prototype.

## Calendar and import parsers

iCalendar import bounds file size, component and property counts, nesting, text expansion, recurrence expansion, and preserved source data. Unknown properties may be retained as bounded data but are never interpreted as application commands.

Recurrence expansion has a hard occurrence guard and a requested date window. A malformed or unsupported rule returns a controlled result rather than consuming unbounded CPU or memory.

Notification payloads and platform callbacks are untrusted. They identify a bounded application action and repeat authorization and current-state validation before mutation. Desktop notification text is bounded and set through one builder: summaries are plain text, as the freedesktop specification defines them, and bodies are markup-escaped on freedesktop servers, so vault or localized text never renders as links, images, or formatting. Only a fixed number of notifications listen for an action at once, each for a bounded time; beyond that limit notifications still show but without action buttons.

## Pomodoro overlays and enforcement

Desktop Pomodoro blockers and overlays are native windows controlled by Rust and narrowly scoped frontend events. They do not expose a general click-capture or arbitrary window-creation API. Enforcement follows canonical Pomodoro transitions, so a stale window event cannot resume, stop, or mutate a newer run.

Browser and desktop anti-distraction enforcement receive only the rule and focus state their surface needs. The Chrome native-messaging host validates message shape, size, and the allowed command set; it is not a generic shell bridge.

Mobile enforcement plugins expose only launchable-app discovery, selected settings intents, foreground checks, and durable usage operations. Accessibility or usage-access grants do not authorize unrelated data collection.

## Music loopback service

Desktop Music uses an ephemeral loopback service for selected local media and the YouTube player; Android does not start it. It binds to 127.0.0.1 on an ephemeral port, answers only requests whose `Host` is exactly that address and port (which refuses DNS rebinding), and protects routes with per-process random tokens that are not logged, persisted, or returned in errors. The YouTube page token never authorizes media, because YouTube's player script runs inside that page and can read it; media URLs carry their own token and per-launch keyed IDs that a path cannot predict. Media responses send no CORS headers. The page reports to the app with an exact `postMessage` target origin wherever the app origin can be named, and gives YouTube only its origin as the widget referrer. Being local does not make the origin trusted.

The server accepts a small allowlisted HTTP subset with bounded headers, deadlines, registrations, and resident artwork. Connections still sending headers share one bounded budget that each holds for at most the read deadline, and authorized media streams draw from a separate budget, so a local process without the media token cannot starve playback. Responses disable caching and MIME sniffing and apply route-specific CSP: media and text routes deny active content, and YouTube routes permit only the origins the embedded player requires.

Local media registrations use generation and retention rules so a stale frontend load cannot remove the source used by current playback.

## URL and network adapters

Each network-capable feature uses its own adapter and allowlist. Do not add a shared open-any-URL or fetch-any-host command. Where an adapter can reach user-controlled destinations, it considers redirects, DNS resolution, IP literals, loopback, link-local and private ranges, credential-bearing URLs, and response-size limits. Current flows are listed in [Network and privacy](network-and-privacy.md).

## Blocking native work

Filesystem, process, media, Git, and operating-system work that may block runs outside the async database executor and never holds a SQL transaction or shared async mutex while waiting. Cancellation, output, time, and concurrency are bounded, and Git and hosted CLI runs stop their whole process tree on timeout or cancellation. See [Native backend](../../architecture/native-backend.md#asynchronous-and-blocking-work).
