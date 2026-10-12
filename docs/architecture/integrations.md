# Integration boundaries

Ganbaru AI integrates with local tools and operating-system services while remaining useful offline. Every integration is optional, capability-scoped, and subordinate to canonical application authority.

## Native coding-agent providers

Desktop Chat supports Rust-owned native transports for Codex, Claude Code, Cursor, Grok, and OpenCode. Ganbaru preserves provider-native interaction concepts while normalizing durable events and organizational history.

Provider sessions do not own channels, agent identity, project commitments, or access policy. Each organizational run resolves one execution target and one immutable authorization revision before dispatch. Provider-specific safety is enforced only where the adapter can verify it.

See [AI integration](../features/ai/README.md), [Chat](../features/chat/README.md), and [Chat access control](../data/access-control.md).

## Internal host tools and external MCP

An active native Chat run can receive an ephemeral loopback MCP endpoint for narrowly scoped Ganbaru-owned host tools. It is application infrastructure, not a general local API and not a participant identity.

A separately authorized MCP service for external clients is planned. It must not reuse possession of the internal endpoint or provider-native trust as authorization.

## Browser extension

The Chromium Manifest V3 extension communicates with a separate local native messaging host. It receives bounded blocker state, reports redacted events, and opens an extension-owned block page. The extension does not turn the browser into a general data bridge.

Firefox support is planned behind the same product-level rule semantics. See [Distraction blocker](../features/distractions/README.md).

## Media

Desktop local audio uses the Rust media service with Rodio and selected Symphonia decoders. Local video uses the platform WebView media stack through a token-gated loopback file host. Android local audio uses a Media3 session service and selected document-tree grants.

YouTube uses the official IFrame Player API. The [YouTube Required Minimum Functionality](https://developers.google.com/youtube/terms/required-minimum-functionality) rules require the embedded player to remain unobscured and retain its required interactions. Current input-locking behavior requires redesign before it can be described as compliant. Ganbaru does not scrape or proxy YouTube streams.

Spotify is not an active integration. Music files remain where the user stores them; the vault stores library metadata, playlist definitions, assignments, and playback state.

See [Music](../features/music/README.md).

## Android services

Android adapters cover document selection and portable backup, calendar and Pomodoro notifications, background local audio, selected-app usage awareness, and explicitly consented blocking. They use operating-system settings and permissions rather than desktop authority translated into mobile commands.

Device-local schedules, journals, and selected-document references reconcile into canonical vault records. They do not become independent sources of truth. See [Android](../platforms/android/README.md).

## Network defaults

Normal productivity features work without network access. Network use is feature-specific and visible, such as an explicitly requested remote Notes import, YouTube playback, update checks, or a configured provider.

Loopback servers use random bearer tokens, bounded requests, narrow registries, and process-lifetime scope. A loopback address is not sufficient authentication by itself.

## Device linking, sync, and collaboration

Private-LAN device linking with single-writer whole-vault handoff is implemented between one person's desktop and Android devices. It uses pinned, paired transport and moves one validated complete vault; it is not concurrent sync.

Typed domain operations, Yrs/Yjs collaborative text, and an optional user-hosted Rust relay for encrypted records are planned, followed by multi-person collaboration. They are not current runtime dependencies. The future transport must preserve local canonical storage and enforce resource visibility for derived data as well as direct reads.

See [Sync and collaboration](../data/sync.md).

## Planned BYOK assistants

A general BYOK assistant path is planned for hosted and local models. It will use typed Ganbaru operations and explicit data scopes. It will not inherit arbitrary shell or filesystem authority from native coding-agent Chat.
