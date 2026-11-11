# AI provider runtimes

Status: Implemented on desktop for Codex, Claude, Cursor, Grok, and OpenCode. Adapters live in `crates/ganbaru-chat-providers/`.

Each adapter normalizes provider-native events into stable Ganbaru contracts while keeping enough native detail for inspection and recovery.

## Shared contract

Each adapter supports exactly the capabilities its metadata advertises; anything else stays absent or disabled. Ganbaru never simulates approval, continuation, cancellation, interactive input, model selection, or reasoning controls that the provider cannot truthfully perform.

All transports apply bounded parsing, payload limits, timeouts, cancellation, process-tree cleanup, redaction, and stable error categories. Short probes (version, account, and catalog queries) read each output stream only up to a fixed limit and stop the whole process tree once a stream exceeds it or the probe times out. Provider output is untrusted input: unknown events may be kept for diagnostics but never become typed application state without validation.

Usage reports carry provider facts only. Context fill is the size of the newest root model request measured against the main model's context window, turn and thread counts are the totals the provider reports, and cache writes appear only when the provider sends them. Rate limit status carries the windows and reset times the provider reports; a provider that reports none shows none.

Users may set any launch arguments, environment variables, and provider settings their CLI accepts. Values on a per-family allowlist (formatting and locale variables, telemetry and timeout switches, inherited API key variables, and a few cosmetic flags) apply immediately. Anything else, including values typed in settings, applies after one native confirmation on this device, and arguments that duplicate options Ganbaru manages (protocol mode, output format, session selection and continuation, permission and approval transport) are always rejected. Claude teammate runs accept no launch arguments at all, so a provider instance with launch arguments cannot run teammates. Executables and provider homes chosen with the native picker, or the family default command, apply without asking. Default commands and Settings discovery search only absolute `PATH` entries, the usual user install folders, and Homebrew on the platform that has it, and skip any program that every local account can write or that sits in a folder every account can write, so a planted copy never becomes a provider silently. See [Chat security](../../data/security/chat.md#provider-configuration-and-secrets).

## Discovery and model catalogs

Native discovery owns provider readiness, model identities, and capability options, including cached catalogs. The frontend validates the catalog and never infers capabilities from model IDs or labels; distinct routing aliases stay selectable even when they share a display name.

Discovery is lazy. App startup and vault preparation load cached Chat history and settings without launching provider processes; opening Chat or Chat settings starts automatic discovery once per vault, and explicit refresh is always available. Probes, catalog queries, and thread lifecycle calls outside a session run in a fresh empty directory that only the user can access, so project configuration in the folder the app was launched from never loads. A failed probe leaves local history usable, and late results never overwrite settings from another vault or a newer load.

Rationale: provider probes spawn external processes, so they must not slow startup or block reading local history.

## Codex

Codex uses its native app-server protocol. Ganbaru owns the process, thread association, event ingestion, interaction requests, cancellation, and workspace authority. The app-server thread identity is an execution detail and never replaces the Chat conversation or agent identity. Provider-supported model, reasoning, approval, and sandbox choices are recorded with the run. Codex rate limit updates are sparse, so Ganbaru merges them per metered limit and reports every known window together.

## Claude

Claude uses its native bidirectional stream-JSON protocol. Ganbaru owns the process and translates streamed messages, tool activity, permission requests, questions, and completion into canonical events. Session identity stays provider-local, and resuming a session must keep its authorized project and execution target; it can never silently resume in a different folder. Subagent frames stay out of the root stream and never move the resume cursor. Teammate instructions join the system prompt once per session, inline or through an owner-only file when they exceed 16 KiB, and a turn whose instructions differ from the session's fails with a retryable conflict instead of changing them silently. Effort and Fast mode change in the running session without a restart. Host proxy and CA certificate variables reach the CLI. Claude reports each rate limit type separately, so Ganbaru merges them per type and a request stays limited until every rejected type resets. Rate limits and root API retries become typed notices; the timeline does not render them yet.

## Cursor and Grok

Cursor and Grok use the official Agent Client Protocol (ACP v1). They share one bounded ACP transport but keep provider-specific executable discovery, credentials, model handling, interactions, continuation, and diagnostics. Loading a native session for resume suppresses the history the agent replays, because the provider already restores its own context and the Ganbaru timeline is the history the user sees; permission requests that arrive during the load are cancelled.

## OpenCode

OpenCode runs as a local server reached through its HTTP and event-stream interfaces. Ganbaru reconciles streamed and fetched history, deduplicates events, keeps events whose own or nested session ID names another session (such as a subagent) out of the thread, reconnects within bounds, and keeps cancellation and session ownership explicit. Ganbaru supports OpenCode from 1.14.19 and below 2.0, and refuses 2.x releases, prereleases included, with an unsupported-version error until a runtime for its protocol exists.

## Compatibility

Executable version floors and dependency versions are implementation facts, not product behavior. Compatibility fixtures and focused integration tests define the supported protocol surface. When a provider changes its protocol incompatibly, the adapter and its fixtures are updated without weakening the shared authorization boundary.

Dependency choices and rejected alternatives are recorded in [Chat dependency decisions](../../architecture/decisions/chat-dependencies.md).
