# Application security boundaries

Tauri webviews are presentation and orchestration clients. Rust owns database access, filesystem paths, native dialogs, imports, exports, process control, credentials, notifications, media hosts, application blocking, and other operating-system effects.

## Tauri capabilities

Capabilities grant only the plugin operations a window or platform needs. Neither desktop nor Android exposes generic filesystem or shell plugins to the webview. Desktop exposes scoped window, webview, event, zoom, and updater operations; Android exposes notification permission checks and the repository-owned mobile plugin commands.

URL opening is allowlisted on both platforms to specific product destinations (the repository, releases, license, icon and sound attributions, and the Notion authorization guide on desktop) rather than arbitrary web addresses. Feature-owned native commands, such as Notes hyperlink opening, parse and check their own URLs before delegation and do not widen this allowlist.

Capability files in `apps/client/src-tauri/capabilities/` are authoritative for exact grants. Do not copy the permission list into documentation.

## Content security policy

Production CSP permits bundled application assets, Tauri IPC, and the origins required by current features. Desktop permits YouTube player scripts and frames plus ephemeral loopback media, image, and frame sources for Music and Chat previews. Android omits YouTube and additionally permits a connection to the GitHub releases API for its in-app update check. These are scoped exceptions, not a generic permission for remote scripts or frames.

Development CSP additionally supports Vite and hot reload, including `unsafe-eval`. A development exception must not be copied into production configuration.

Inline styles are allowed for dynamic Svelte layout, theme, and calendar geometry. Inline scripts and object embedding remain prohibited unless a later feature receives a separate review.

## Native dialogs and document transfer

File import and export flows are Rust-owned. Desktop commands open native dialogs, apply extension and size constraints, validate the selected path, and return bounded parsed data or a narrow result. The webview never receives a reusable filesystem grant.

Android uses system document providers and MediaStore through repository-owned plugins. The native layer validates display names, MIME ambiguity, size, UTF-8 where required, and operation mode. It exposes selected content or a bounded export result, not a generic content URI or storage API.

User cancellation is an ordinary typed outcome, not a path or permission error.

## Vault and configuration

Vault selection validates the marker, schema version, directory relationship, permissions, and database availability before changing the active pointer. The application never treats an unrelated non-empty folder as a new vault and never silently deletes a damaged configured vault.

Portable config writes occur in Rust through validated patching and atomic replacement, so independent windows cannot rewrite the whole config from stale frontend copies. Unknown persistent values are rejected or dropped by explicit rules.

The active vault path is configuration, not a hardcoded Documents location. Managed child paths are derived below the validated active root.

## Managed assets

Profile images, project icons, Chat attachments, Notes assets, and browser artifacts live in feature-owned managed locations. Import validates source type, size, content signature where applicable, path identity, and symlink status before copying. Image types are detected from signatures, not only extensions, and text attachments must be valid bounded text.

Managed relative paths are stored only after normalization; an arbitrary external path cannot be persisted as a managed asset. Cleanup rechecks live references before deletion and records retry state if immediate removal is unsafe. Errors do not echo source absolute paths unnecessarily.

## Notes content and history

Notes rich content, formulas, imports, transfer manifests, comments, and asset references are untrusted structured input. Rust validates types, sizes, nesting, IDs, graph relationships, and path boundaries before mutation.

Compressed history is validated before parsing: encoding, declared and actual decompressed size, content digest, and JSON shape, with per-chunk and reconstructed-size limits for chunked history. Restore applies canonical writes transactionally and rebuilds disposable indexes after success.

Markdown and HTML renderers sanitize or avoid active content. Export is not a route for executing embedded scripts or resolving arbitrary local paths.

## Calendar and import parsers

iCalendar import bounds file size, component and property counts, nesting, text expansion, recurrence expansion, and preserved source data. Unknown properties may be retained as bounded data but are never interpreted as application commands.

Recurrence expansion has a hard occurrence guard and a requested date window. A malformed or unsupported rule returns a controlled result rather than consuming unbounded CPU or memory.

Notification payloads and platform callbacks are untrusted. They identify a bounded application action and repeat authorization and current-state validation before mutation.

## Pomodoro overlays and enforcement

Desktop Pomodoro blockers and overlays are native windows controlled by Rust and narrowly scoped frontend events. They do not expose a general click-capture or arbitrary window-creation API. Enforcement follows canonical Pomodoro transitions, so a stale window event cannot resume, stop, or mutate a newer run.

Browser and desktop anti-distraction enforcement receive only the rule and focus state their surface needs. The Chrome native-messaging host validates message shape, size, and the allowed command set; it is not a generic shell bridge.

Mobile enforcement plugins expose only launchable-app discovery, selected settings intents, foreground checks, and durable usage operations. Accessibility or usage-access grants do not authorize unrelated data collection.

## Music loopback service

Music uses an ephemeral loopback service for selected local media and the YouTube player. It binds to 127.0.0.1 on an ephemeral port and protects routes with a per-process random token that is not logged, persisted, or returned in errors. Being local does not make the origin trusted.

The server accepts a small allowlisted HTTP subset with bounded headers, deadlines, worker count, registrations, and resident artwork. Responses disable caching and MIME sniffing and apply route-specific CSP: media and text routes deny active content, and YouTube routes permit only the origins the embedded player requires.

Local media registrations use generation and retention rules so a stale frontend load cannot remove the source used by current playback.

## URL and network adapters

Each network-capable feature uses its own adapter and allowlist. Do not add a shared open-any-URL or fetch-any-host command. Where an adapter can reach user-controlled destinations, it considers redirects, DNS resolution, IP literals, loopback, link-local and private ranges, credential-bearing URLs, and response-size limits. Current flows are listed in [Network and privacy](network-and-privacy.md).

## Blocking native work

Filesystem, process, media, Git, and operating-system work that may block runs outside the async database executor and never holds a SQL transaction or shared async mutex while waiting. Cancellation, output, time, and concurrency are bounded. See [Native backend](../../architecture/native-backend.md#asynchronous-and-blocking-work).
