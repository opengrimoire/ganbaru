# Chat security

The local coding-agent Chat starts native provider processes and operates on user-selected repositories. Provider CLIs, protocol events, model output, repository instructions, terminal output, URLs, Git metadata, and errors are untrusted. Rust is the policy boundary.

Organizational authorization is specified in [Chat access control](../access-control.md). This document covers how native boundaries enforce it.

## Provider lifecycle

Rust owns provider discovery, startup, transport, cancellation, event ingestion, and shutdown. The frontend selects a configured provider and sends typed commands. It cannot spawn an arbitrary executable, choose an arbitrary provider home, or connect directly to a provider's local server.

One driver generation owns an active thread. Events from a stopped, settled, or mismatched generation cannot enter the projection stream. Shutdown rejects new commands, flushes bounded pending events, and enforces deadlines even when a provider hangs.

Provider-native approval, trust, sandbox, or workspace state is defense in depth. It never widens folder, channel, runtime, or internal-tool authority granted by Ganbaru.

## Provider configuration and secrets

Portable provider configuration stores nonsecret values and opaque credential references. Executable paths, provider homes, discovery caches, and process state are device-local.

Portable runtime values that can run code, widen permissions, or redirect traffic (launch arguments, environment entries, and non-cosmetic provider settings) apply only after the user confirms them in a native dialog on this device. Rust classifies each value, stores the approved per-value SHA-256 digests in device-local state, and gates every provider launch, probe, model refresh, and provider-file access on them. A restore or handoff that carries the same values keeps working without a prompt; a changed or newly imported value is paused and listed in Settings until confirmed. Per-family allowlisted values and the family defaults apply silently. Typed executable and provider home paths are confirmed at save or test, while native picker selections are vouched for by short-lived in-memory receipts that are never persisted. Arguments that would duplicate managed transport options are always rejected.

Plaintext environment values stay valid, but a value whose name looks secret-like (a whole `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `AUTH`, or similar segment) shows a warning with a one-click move to the credential store, and runtime confirmations hide such values. Name matching cannot tell credentials from settings like `MAX_THINKING_TOKENS`, so it never rejects.

Rust derives each provider credential reference from the instance ID and environment variable name; the webview never names one. Before reading the store, materialization rejects any reference that is not the derived one for that instance and variable, so an edited or imported configuration cannot read another instance's secret. Source-control tokens use a separate credential-store service from provider secrets.

Secrets are resolved from the operating-system credential store immediately before the operation that needs them. Temporary provider environments are not persisted in diagnostics or DTOs. Replacing or removing a credential invalidates affected probe and model caches, and cleanup errors do not expose secret values or credential-store details.

The provider-file settings portal is a narrow exception for user configuration and instruction files. Rust resolves fixed file names from the family default home, the app process environment, or provider homes and environment values confirmed on this device; the portal refuses to open until the instance's runtime values are confirmed. A custom OpenCode configuration named by `OPENCODE_CONFIG` must be a JSON or JSONC file in an existing folder, and only the provider home may be created on save. The webview receives home-relative display paths. Reads and writes reject symlinks, invalid UTF-8, null bytes, traversal, oversized data, stale revisions, and unsupported files; saves use restrictive permissions and sibling temporary replacement. These files may contain secrets and are never copied into the vault, SQLite, diagnostics, or logs.

## Project provider configuration

Project provider configuration in a working folder can run commands as soon as a provider starts, so it follows the folder trust record described in [Git and hosted CLIs](#git-and-hosted-clis). Trusted folders load it exactly as the CLI does in the user's terminal; only folders without trust are restricted. Each provider family has one rule:

- Claude: the provider digest covers `.claude/settings.json`, `.claude/settings.local.json`, `.mcp.json`, and the `.claude/skills`, `.claude/agents`, and `.claude/commands` trees. A start in a folder whose current configuration no decision covers asks once and retries; an unattended assignment turn starts restricted instead. Restricted launches pass `--setting-sources user`, which with the pinned Claude version drops project settings, hooks (including skill hooks), project MCP servers, skills, commands, agents, and project `CLAUDE.md` while keeping the user's own settings, hooks, and MCP servers. A `--setting-sources` launch argument the user confirmed on this device overrides this. `--strict-mcp-config` and `disableAllHooks` are not used because they also drop the user's own servers and hooks.
- Codex: Codex loads project `.codex/config.toml` only for projects trusted in its own `CODEX_HOME`, which Ganbaru never writes, so no extra gate is needed.
- Cursor and Grok: project rules, MCP servers, and hooks under `.cursor/` may load in ACP mode, and whether Cursor's workspace trust applies there is unknown. This is a known residual risk until verified with the pinned version. Not verified on this device.
- OpenCode: `opencode.json` and `.opencode/` plugins load without a trust gate of its own. This is a known residual risk until a restriction is verified with the pinned version; `OPENCODE_DISABLE_PROJECT_CONFIG` is the candidate. Not verified on this device.

Isolated worktrees load project configuration only when the folder is trusted and every configuration file in the worktree matches the trusted folder's; otherwise they start restricted without asking. Edits a provider makes during a turn change the digest, so the next start asks again. Claude permission suggestions accepted with "Allow for session" stay in the session and are never written to the folder's settings files.

## Working-folder authorization

Portable working-folder identity contains no external absolute path. Managed folders resolve below the active vault. External folders resolve through a device-local binding scoped by vault, device, and logical folder ID.

Rust validates the opened directory's filesystem identity (device and inode on Unix, volume and file identity on Windows, with reparse-point escapes rejected). Git-sensitive operations separately validate the Git common-storage identity. Missing directories, stale paths, replacement, traversal, absolute injection, symlink escape, vault nesting, and unbound devices fail closed. Remote URLs, branch names, and mutable Git configuration do not participate in authority; how Ganbaru runs Git against that configuration is described in [Git and hosted CLIs](#git-and-hosted-clis).

The same authorization service serves provider startup, terminals, attachments, Markdown operations, file tools, mentions, previews, watchers, Git, checkpoints, and restores. No adapter reimplements a weaker path check. Known residual races between identity checks and later opens are recorded in [Unsafe Rust](unsafe-rust.md).

## File reads and writes

Workspace operations accept validated relative paths below the authorized root. Listing is on demand and applies Git ignore rules plus bounded exclusions for common generated or secret-bearing directories.

File tools, mentions, previews, and saves never touch Git metadata (`.git` at any depth, as a directory or gitfile), top-level credential folders (`.ssh`, `.gnupg`, `.aws`), top-level environment files other than `.env.example`, or provider credential files. Names are compared per component the way Windows and macOS resolve them: case folded, trailing dots and spaces ignored, alternate data streams and 8.3 short names mapped to their base name, and HFS+ ignorable code points dropped. A directory walk also refuses any directory whose file identity is the folder's `.git`, and Windows rejects reserved device names. `.github`, `.gitignore`, `.gitattributes`, and `.gitmodules` stay editable.

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

## Git and hosted CLIs

Every Git and hosted CLI spawn goes through one hardened runner with a named deadline, bounded output, and process-tree termination on timeout or cancellation. Each run has an origin and the folder's trust:

- Internal runs (status, listings, diffs, checkpoints, restores, observer probes) never execute hooks, fsmonitor, or signing, and never use the `ext` transport.
- User-initiated commands in trusted folders apply repository configuration and hooks exactly as the user's terminal does.
- In untrusted folders, programs defined in local or worktree configuration (pagers, editors, diff and merge tools, filters other than standard LFS, credential helpers, SSH commands, proxies) are replaced by the user's global value or a neutral value. System and global configuration always stays, so results match the terminal.

Hosted CLIs (`gh`, `glab`, `az`) list and view change requests from a private empty directory with explicit repository coordinates, so repository content cannot steer them. Creation and checkout run where they need Git context. Their Git children inherit the same hardening through the environment, and `az` runs with Python path isolation. Push, pull, and fetch accept only remotes already configured in the repository and branch names that follow Git ref rules without refspec syntax, so a request cannot redirect to a URL or force-push through a `+` refspec.

Trust is a device-local record per folder holding two digests. The Git digest covers command-bearing keys from local and worktree configuration (with included files) and the hooks directory; ordinary keys such as remotes, branches, and identity are left out so everyday work never asks again. `.gitattributes` is left out because it only selects drivers that configuration defines, and branch checkouts change it. The provider digest covers project provider configuration that can run commands, as listed in [Project provider configuration](#project-provider-configuration).

- Folders bound through the native picker ask once at binding with a native dialog. Cancelling keeps the binding and leaves trust undecided.
- A folder with no repository programs, hooks, or provider settings is trusted silently, so managed folders never ask.
- A user Git action (commit, push, pull, fetch, stage, discard, clone, worktree creation, review apply, hosted change request creation and checkout) in an undecided folder, or one whose Git digest changed, asks once and then retries. Received folders with an unknown digest therefore ask before anything repository-defined runs.
- Every run recomputes the Git digest; a mismatch with the trusted digest runs untrusted, including in a worktree whose configuration differs.
- Settings shows each folder's trust and can revoke or grant it. Rebinding to a different path or repository and unbinding clear the record.

Edits to repository metadata (`.git` at any depth) are never approved automatically outside full access, because they can change what later Git runs execute. Ganbaru keeps Cursor and OpenCode edits there reviewed; Claude prompts for its protected paths, and the Codex workspace sandbox keeps `.git` read-only.

## Checkpoints and restore

Checkpoint capture and restore verify folder and Git common-storage identity immediately before mutation, and validate dirty state, changed and untracked files, repository boundaries, and the expected checkpoint revision.

Restore is a scoped Git or filesystem operation, not a database rollback. It cannot change organizational messages, memberships, approvals, or another project's files. Previewing and applying a restore are separate authority checks. Native work runs outside database transactions, and retryable cleanup records prevent a failed step from appearing successful.

## Revocation

When membership, history, profile, target, runtime, scratch, or audience authority is reduced, new native actions fail immediately. A continuation that already materialized restricted context is interrupted and discarded or quarantined before reuse.

Provider stop, tool shutdown, terminal, preview, and scratch cleanup have bounded deadlines. Incomplete cleanup persists as a retryable record without weakening the revocation or exposing native paths.
