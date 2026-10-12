# Security

Ganbaru AI stores calendar, work, Notes, browsing-control, focus, and AI conversation data. Security is a constraint on every storage and execution decision, not a feature that can be added after those decisions are made.

## Threat model

The application must limit:

- Compromised or malicious dependencies during development, build, install, and runtime.
- Untrusted provider processes, provider protocol events, repository content, Markdown, terminal output, URLs, media metadata, imports, and native callbacks.
- A webview or frontend defect attempting to exceed its narrow native commands.
- Path traversal, symlink escape, replaced external folders, stale Git identity, and unsafe archive or import expansion.
- Accidental disclosure through logs, diagnostics, search, summaries, exports, attachments, caches, and cross-channel references.
- Network observation of device linking, and server-side plaintext access when future remote synchronization is enabled.
- Release, updater, package-repository, and CI credential compromise.

The application can constrain native processes it starts and the data it deliberately sends to external services. It cannot stop same-user malware from reading an unencrypted vault through ordinary operating-system access. Local data is not application-encrypted; the operating-system account and disk encryption protect files at rest (see [Network and privacy](network-and-privacy.md#local-data-encryption)).

The application also does not defend against a user intentionally modifying their own database, an attacker with physical access to an unlocked or unencrypted device, or an AI provider retaining content according to its own policy after the user chose to send it.

## Trust boundaries

Rust application services are the policy boundary for filesystem, process, credential, database, import, export, update, network, and operating-system operations. The Svelte webview receives validated DTOs and scoped commands, not ambient shell or filesystem access.

Provider-native trust never widens organizational authority. Portable IDs do not authorize a device path. User-authored text, mentions, URLs, repository instructions, and provider events are data, not commands or grants.

The active vault is user-owned durable storage. The platform app config directory is trusted only for device-local bootstrap, binding, and pairing state. Secrets are resolved through native credential storage and are not copied into ordinary config, SQLite, diagnostics, or provider DTOs.

## Status

Local LAN device linking and single-writer whole-vault handoff are implemented in source: pinned certificates, mutual TLS, single-use invitations, and no relay or account. Concurrent operation sync, the end-to-end encrypted relay, and its key lifecycle remain planned. Both are specified in [Synchronization](../sync.md).

Remote synchronization, hosted BYOK chat, an external MCP service, and the `ganbaru-ai` CLI are not implemented. Their security requirements are normative design constraints, not claims about current traffic or capability. Documents must label current behavior and planned requirements separately.

## Required properties

Across domains:

- Deny unknown, stale, oversized, malformed, or unsupported input.
- Validate before mutation and again at the privileged boundary.
- Bound reads, writes, decompression, parsing, process output, queues, and worker counts.
- Remove secrets and unnecessary absolute paths from persisted errors and diagnostics.
- Use explicit allowlists for native commands, URL schemes, origins, file types, and tool methods.
- Preserve user-authored data when a safe repair or export path exists.
- Make cleanup retryable when immediate deletion is unsafe.
- Keep security-sensitive decisions in testable Rust services rather than presentation code.

These documents record rationale and policy. Exact capabilities, CSP directives, URL scopes, and dependency versions are authoritative in configuration and lockfiles and must be reviewed when they change.

## Security documents

- [Supply chain](supply-chain.md): dependencies, CI, releases, copied code, and contributor rules.
- [Application boundaries](application-boundaries.md): Tauri capabilities, CSP, native I/O, imports, assets, Notes, overlays, and loopback services.
- [Chat security](chat.md): provider processes, working folders, terminals, attachments, previews, checkpoints, and internal host tools.
- [Network and privacy](network-and-privacy.md): telemetry, current and planned network flows, local encryption, sync encryption, and data minimization.
- [Unsafe Rust](unsafe-rust.md): first-party unsafe-code policy and the reviewed native and FFI boundary inventory.
- [Dependency audits](dependency-audits.md): reviewed advisory exceptions and allowed warnings.
- [Chat access control](../access-control.md): the normative authorization specification.
