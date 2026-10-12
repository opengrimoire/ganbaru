# Chat security

The local coding-agent Chat starts native provider processes and operates on user-selected repositories. Provider CLIs, protocol events, model output, repository instructions, terminal output, URLs, Git metadata, and errors are untrusted. Rust is the policy boundary.

Organizational authorization is specified in [Chat access control](../access-control.md). This document covers how native boundaries enforce it.

## Provider lifecycle

Rust owns provider discovery, startup, transport, cancellation, event ingestion, and shutdown. The frontend selects a configured provider and sends typed commands. It cannot spawn an arbitrary executable, choose an arbitrary provider home, or connect directly to a provider's local server.

One driver generation owns an active thread. Events from a stopped, settled, or mismatched generation cannot enter the projection stream. Shutdown rejects new commands, flushes bounded pending events, and enforces deadlines even when a provider hangs.

Provider-native approval, trust, sandbox, or workspace state is defense in depth. It never widens folder, channel, runtime, or internal-tool authority granted by Ganbaru AI.

## Provider configuration and secrets

Portable provider configuration stores nonsecret values and opaque credential references. Executable paths, provider homes, discovery caches, and process state are device-local.

Secrets are resolved from the operating-system credential store immediately before the operation that needs them. Temporary provider environments are not persisted in diagnostics or DTOs. Replacing or removing a credential invalidates affected probe and model caches, and cleanup errors do not expose secret values or credential-store details.

The provider-file settings portal is a narrow exception for user configuration and instruction files. Rust resolves a fixed allowlist from provider family and instance. Reads and writes reject symlinks, invalid UTF-8, null bytes, traversal, oversized data, stale revisions, and unsupported files; saves use restrictive permissions and sibling temporary replacement. These files may contain secrets and are never copied into the vault, SQLite, diagnostics, or logs.

## Working-folder authorization

Portable working-folder identity contains no external absolute path. Managed folders resolve below the active vault. External folders resolve through a device-local binding scoped by vault, device, and logical folder ID.

Rust validates the opened directory's filesystem identity (device and inode on Unix, volume and file identity on Windows, with reparse-point escapes rejected). Git-sensitive operations separately validate the Git common-storage identity. Missing directories, stale paths, replacement, traversal, absolute injection, symlink escape, vault nesting, and unbound devices fail closed. Remote URLs, branch names, and mutable Git configuration do not participate in authority.

The same authorization service serves provider startup, terminals, attachments, Markdown operations, file tools, mentions, previews, watchers, Git, checkpoints, and restores. No adapter reimplements a weaker path check. Known residual races between identity checks and later opens are recorded in [Unsafe Rust](unsafe-rust.md).

## File reads and writes

Workspace operations accept validated relative paths below the authorized root. Listing is on demand and applies Git ignore rules plus bounded exclusions for common generated or secret-bearing directories.

Text preview rejects binary content, invalid UTF-8, symlinks, traversal, and oversized files. The webview receives bounded text and relative metadata only. Opening a file externally repeats authorization immediately before delegation.

Writes use expected-revision checks where user edits could race provider edits. Temporary replacement stays inside the authorized parent, and a tool cannot create a symlink or use a renamed ancestor to escape the root.

Provider-reported changed files are hints. Ingestion keeps only verified, deduplicated relative paths inside the root.

## Workspace observation

One native watcher serves the current authorized target, and switching targets stops the previous generation first. Output is coalesced into bounded invalidations without absolute paths; overflow emits one refresh-required invalidation. A watcher event never proves authorization or file existence, so every later read repeats validation.

## Terminals and processes

A terminal is bound to one authorized thread, execution target, runtime approval, and process generation. Rust constructs the working directory and environment; the frontend cannot supply a host path or inherit the full application environment.

Input, output, scrollback, event queues, and process lifetime are bounded. Terminal output is untrusted display data; escape sequences and link detection must not trigger an unreviewed native action. Stopping, revoking, changing target, or closing the owning scope terminates or quarantines the terminal, and a stale handle cannot attach to a newer run.

## Internal MCP host tools

Each native Chat run may receive an ephemeral loopback MCP endpoint with a small application-owned method allowlist. It is not a general localhost service and not an organizational participant.

Every request validates a run-scoped credential, method, bounded arguments, current run and thread, channel membership, access-profile revision, execution target, and revocation state. Results exclude credentials, external absolute paths, and unrelated vault content. The endpoint closes with the run, and replayed credentials or calls after interruption or revocation fail. A future external MCP service requires separate authorization and cannot reuse this endpoint.

## Event ingestion and diagnostics

Provider event size, nesting, sequence, identifiers, and payload shape are bounded before persistence. Unknown event kinds become typed unsupported data or safe diagnostics, never dynamically executed.

Diagnostics exclude secret-like fields, authorization material, unnecessary home paths, provider environments, and credentials. Persisted errors explain the application boundary without becoming a second transcript of sensitive provider output.

Projection changes commit before the frontend is notified, and notifications carry identity and invalidation data, not unbounded payloads.

## Attachments and context

Attachment import rejects symlink sources, unsupported signatures, invalid text, traversal, and oversized files. Bytes are hashed and written with restrictive permissions below the managed Chat asset root, and metadata stores the managed relative identity rather than the source path. Message and draft references own retention, and deferred cleanup rechecks references before removal.

Mentions, workspace files, Notes, tasks, channel messages, browser artifacts, and other context sources are resolved under current authority. Natural-language instructions or provider requests cannot introduce a hidden source. Audience restrictions still apply after content is summarized or transformed.

## Browser previews

Preview URLs must be HTTP or HTTPS with a host and no embedded credentials. Loopback targets (local development servers) open directly; any other destination requires explicit user confirmation. Navigation stays within the confirmed origin, and a loopback preview cannot navigate off loopback.

Webview identity, port, generation, navigation events, screenshots, recordings, and cleanup are tied to the owning thread and target, so a stale preview cannot emit current events. Browser artifacts are bounded managed assets with explicit references and cleanup. Cookies, local storage, and authenticated browsing are not imported into provider context.

## Checkpoints and restore

Checkpoint capture and restore verify folder and Git common-storage identity immediately before mutation, and validate dirty state, changed and untracked files, repository boundaries, and the expected checkpoint revision.

Restore is a scoped Git or filesystem operation, not a database rollback. It cannot change organizational messages, memberships, approvals, or another project's files. Previewing and applying a restore are separate authority checks. Native work runs outside database transactions, and retryable cleanup records prevent a failed step from appearing successful.

## Revocation

When membership, history, profile, target, runtime, scratch, or audience authority is reduced, new native actions fail immediately. A continuation that already materialized restricted context is interrupted and discarded or quarantined before reuse.

Provider stop, tool shutdown, terminal, preview, and scratch cleanup have bounded deadlines. Incomplete cleanup persists as a retryable record without weakening the revocation or exposing native paths.
