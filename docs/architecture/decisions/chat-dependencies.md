# Chat dependency decisions

This record explains dependency boundaries that materially affect Chat security, compatibility, or maintenance. Exact resolved versions remain authoritative in manifests and lockfiles.

## Selected dependencies

| Concern | Decision | Rationale |
| --- | --- | --- |
| Async child processes | Tokio's narrow process and I/O features | Integrates with the Rust runtime without a second executor. |
| Process-tree cleanup | Platform-specific Rust implementation | Cleanup must cover descendants and application exit, not only a direct child. |
| Native credentials | Platform credential stores behind an adapter | Provider secrets do not belong in vault files or SQLite. |
| Pseudoterminals | `portable-pty` behind a bounded service | A mature cross-platform PTY boundary is safer than ad hoc terminal emulation in Rust. |
| Markdown | `marked` plus DOMPurify | Parsing and sanitization remain separate and untrusted HTML never bypasses sanitization. |
| Diffs | `@pierre/diffs` vanilla API | Supports large review surfaces without coupling the app to a framework adapter. |
| Workspace observation | `notify` behind bounded reconciliation | Native observation improves freshness, while authoritative reads preserve correctness. |
| Terminal rendering | xterm packages | Terminal escape handling belongs in a maintained terminal renderer. |
| File editor | CodeMirror 6 and lazy language support | Provides revision-safe editing and broad syntax support without loading all grammars at startup. |
| ACP types | Official `agent-client-protocol` crate | Cursor and Grok share negotiated protocol types while provider extensions remain validated. |
| Internal MCP | Official `rmcp` crate | The ephemeral host-tool endpoint uses maintained protocol types without becoming a permanent server. |

## Provider transport decisions

Codex and Claude use their native supported transports. Cursor and Grok use the official ACP types with ACP v1 negotiation rather than a handwritten protocol implementation.

OpenCode event streams use the existing HTTP stack and a small repository-owned bounded Server-Sent Events decoder. A larger event-stream dependency is unnecessary for the required grammar, reconnection, deduplication, and history reconciliation.

## Browser capture

Browser preview capture uses target-specific bindings already available in the desktop stack. It captures only the isolated preview webview. General desktop-capture dependencies and screen-wide authority are intentionally excluded.

Recordings use bounded image frames and a versioned manifest. A video codec or media subprocess is not added solely for preview recording.

## Credentials

Linux uses the desktop secret-service boundary, Windows uses Credential Manager, and the macOS adapter uses the Keychain through the credential abstraction. The macOS path still requires physical-platform acceptance before credentials can be considered release-ready.

No fallback stores secrets in the vault, configuration JSON, environment snapshots, logs, or provider diagnostics.

## Dependency review rules

New dependencies require a concrete capability that cannot be implemented safely with the existing stack. Review includes maintenance, provenance, advisory history, transitive scope, runtime authority, artifact impact, and platform support.

Historical audit results and package versions do not belong in this decision record. Current security results come from the repository validation and audit commands.
