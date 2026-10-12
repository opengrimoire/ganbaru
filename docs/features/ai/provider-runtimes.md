# AI provider runtimes

Status: Implemented on desktop for Codex, Claude, Cursor, Grok, and OpenCode. Adapters live in `crates/ganbaru-chat-providers/`.

Each adapter normalizes provider-native events into stable Ganbaru AI contracts while keeping enough native detail for inspection and recovery.

## Shared contract

Each adapter supports exactly the capabilities its metadata advertises; anything else stays absent or disabled. Ganbaru AI never simulates approval, continuation, cancellation, interactive input, model selection, or reasoning controls that the provider cannot truthfully perform.

All transports apply bounded parsing, payload limits, timeouts, cancellation, process-tree cleanup, redaction, and stable error categories. Provider output is untrusted input: unknown events may be kept for diagnostics but never become typed application state without validation.

## Discovery and model catalogs

Native discovery owns provider readiness, model identities, and capability options, including cached catalogs. The frontend validates the catalog and never infers capabilities from model IDs or labels; distinct routing aliases stay selectable even when they share a display name.

Discovery is lazy. App startup and vault preparation load cached Chat history and settings without launching provider processes; opening Chat or Chat settings starts automatic discovery once per vault, and explicit refresh is always available. A failed probe leaves local history usable, and late results never overwrite settings from another vault or a newer load.

Rationale: provider probes spawn external processes, so they must not slow startup or block reading local history.

## Codex

Codex uses its native app-server protocol. Ganbaru AI owns the process, thread association, event ingestion, interaction requests, cancellation, and workspace authority. The app-server thread identity is an execution detail and never replaces the Chat conversation or agent identity. Provider-supported model, reasoning, approval, and sandbox choices are recorded with the run.

## Claude

Claude uses its native bidirectional stream-JSON protocol. Ganbaru AI owns the process and translates streamed messages, tool activity, permission requests, questions, and completion into canonical events. Session identity stays provider-local, and resuming a session must keep its authorized project and execution target; it can never silently resume in a different folder.

## Cursor and Grok

Cursor and Grok use the official Agent Client Protocol (ACP v1). They share one bounded ACP transport but keep provider-specific executable discovery, credentials, model handling, interactions, continuation, and diagnostics.

## OpenCode

OpenCode runs as a local server reached through its HTTP and event-stream interfaces. Ganbaru AI reconciles streamed and fetched history, deduplicates events, reconnects within bounds, and keeps cancellation and session ownership explicit.

## Compatibility

Executable version floors and dependency versions are implementation facts, not product behavior. Compatibility fixtures and focused integration tests define the supported protocol surface. When a provider changes its protocol incompatibly, the adapter and its fixtures are updated without weakening the shared authorization boundary.

Dependency choices and rejected alternatives are recorded in [Chat dependency decisions](../../architecture/decisions/chat-dependencies.md).
