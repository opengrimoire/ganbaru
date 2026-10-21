# AI provider runtimes

Status: Implemented on desktop for Codex, Claude, Cursor, Grok, and OpenCode. Adapters live in `crates/ganbaru-chat-providers/`.

Each adapter normalizes provider-native events into stable Ganbaru contracts while keeping enough native detail for inspection and recovery.

## Shared contract

Each adapter supports exactly the capabilities its metadata advertises; anything else stays absent or disabled. Ganbaru never simulates approval, continuation, cancellation, interactive input, model selection, or reasoning controls that the provider cannot truthfully perform.

All transports apply bounded parsing, payload limits, timeouts, cancellation, process-tree cleanup, redaction, and stable error categories. Short probes (version, account, and catalog queries) read each output stream only up to a fixed limit and stop the whole process tree once a stream exceeds it or the probe times out. Provider output is untrusted input: unknown events may be kept for diagnostics but never become typed application state without validation.

Users may set any launch arguments, environment variables, and provider settings their CLI accepts. Values on a per-family allowlist (formatting and locale variables, telemetry and timeout switches, inherited API key variables, and a few cosmetic flags) apply immediately. Anything else, including values typed in settings, applies after one native confirmation on this device, and arguments that duplicate options Ganbaru manages (protocol mode, output format, session continuation, permission and approval transport) are always rejected. Executables and provider homes chosen with the native picker, or the family default command, apply without asking. Default commands and Settings discovery search only absolute `PATH` entries, the usual user install folders, and Homebrew on the platform that has it, and skip any program that every local account can write or that sits in a folder every account can write, so a planted copy never becomes a provider silently. See [Chat security](../../data/security/chat.md#provider-configuration-and-secrets).

## Discovery and model catalogs

Native discovery owns provider readiness, model identities, and capability options, including cached catalogs. The frontend validates the catalog and never infers capabilities from model IDs or labels; distinct routing aliases stay selectable even when they share a display name.

Discovery is lazy. App startup and vault preparation load cached Chat history and settings without launching provider processes; opening Chat or Chat settings starts automatic discovery once per vault, and explicit refresh is always available. Probes, catalog queries, and thread lifecycle calls outside a session run in a fresh empty directory that only the user can access, so project configuration in the folder the app was launched from never loads. A failed probe leaves local history usable, and late results never overwrite settings from another vault or a newer load.

Rationale: provider probes spawn external processes, so they must not slow startup or block reading local history.

## Codex

Codex uses its native app-server protocol. Ganbaru owns the process, thread association, event ingestion, interaction requests, cancellation, and workspace authority. The app-server thread identity is an execution detail and never replaces the Chat conversation or agent identity. Provider-supported model, reasoning, approval, and sandbox choices are recorded with the run.

## Claude

Claude uses its native bidirectional stream-JSON protocol. Ganbaru owns the process and translates streamed messages, tool activity, permission requests, questions, and completion into canonical events. Session identity stays provider-local, and resuming a session must keep its authorized project and execution target; it can never silently resume in a different folder.

## Cursor and Grok

Cursor and Grok use the official Agent Client Protocol (ACP v1). They share one bounded ACP transport but keep provider-specific executable discovery, credentials, model handling, interactions, continuation, and diagnostics.

## OpenCode

OpenCode runs as a local server reached through its HTTP and event-stream interfaces. Ganbaru reconciles streamed and fetched history, deduplicates events, keeps events whose own or nested session ID names another session (such as a subagent) out of the thread, reconnects within bounds, and keeps cancellation and session ownership explicit.

## Compatibility

Executable version floors and dependency versions are implementation facts, not product behavior. Compatibility fixtures and focused integration tests define the supported protocol surface. When a provider changes its protocol incompatibly, the adapter and its fixtures are updated without weakening the shared authorization boundary.

Dependency choices and rejected alternatives are recorded in [Chat dependency decisions](../../architecture/decisions/chat-dependencies.md).
