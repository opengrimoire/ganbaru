# Chat security

The local coding-agent Chat starts native provider processes and operates on user-selected repositories. Provider CLIs, protocol events, model output, repository instructions, terminal output, URLs, Git metadata, and errors are untrusted. Rust is the policy boundary.

Organizational authorization is specified in [Chat access control](../access-control.md). This document covers how native boundaries enforce it.

## Provider lifecycle

Rust owns provider discovery, startup, transport, cancellation, event ingestion, and shutdown. The frontend selects a configured provider and sends typed commands. It cannot spawn an arbitrary executable, choose an arbitrary provider home, or connect directly to a provider's local server.

Owned local OpenCode servers are always authenticated. Every start generates a fresh 32-byte password from the operating-system CSPRNG, passes it only through the child environment, keeps it in memory for basic authentication, and never persists it. Startup fails closed unless a request without credentials to a protected route returns 401, because loopback ports are reachable by every local user. Only an owned, authenticated server receives the internal MCP endpoint and its bearer token; external OpenCode servers never do.

One driver generation owns an active thread. Events from a stopped, settled, or mismatched generation cannot enter the projection stream. Shutdown rejects new commands, flushes bounded pending events, and enforces deadlines even when a provider hangs.

Provider-native approval, trust, sandbox, or workspace state is defense in depth. It never widens folder, channel, runtime, or internal-tool authority granted by Ganbaru.

An approval answers the tool call the provider asked about. Claude approvals forward the provider's own tool input, and a replacement input from the webview that differs from it is rejected, because no approval flow edits tool input. Question answers are assembled in Rust from the offered option IDs. A future edit flow must validate the edited input against that tool's schema in Rust.

In-app confirmations for permanently deleting a thread (typed title), closing a running terminal, recreating a deleted file, cleaning up scratch folders, and starting a preview recording guard against mistakes in the UI. They are not a control against a compromised app webview, which the command ACL limits to the app-owned windows; each of these effects is reachable through other commands that window may call. Security-relevant consent that must hold against a compromised webview is minted in Rust, such as terminal start grants, provider launch confirmations, and preview origin grants.

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
- Codex: the provider digest covers the `.codex` tree. Codex loads project `.codex/config.toml`, hooks, and rules only for folders its own configuration trusts by path, and worktrees of a trusted repository inherit that trust, so a path the user once trusted in Codex would load whatever a handoff or provider later writes there. Restricted launches in a folder with a `.codex` entry pass `-c projects={"<launch directory>"={trust_level="untrusted"}}`, which takes precedence over a trusted path or parent repository for that launch only and never changes the user's Codex configuration. Folders without `.codex` keep Codex's own trust and defaults. MCP status reads without a live session start restricted rather than asking.
- Cursor and Grok: project rules, MCP servers, and hooks under `.cursor/` may load in ACP mode, and whether Cursor's workspace trust applies there is unknown. This is a known residual risk until verified with the pinned version. Not verified on this device.
- OpenCode: `opencode.json` and `.opencode/` plugins load without a trust gate of its own. This is a known residual risk until a restriction is verified with the pinned version; `OPENCODE_DISABLE_PROJECT_CONFIG` is the candidate. Not verified on this device.

Isolated worktrees load project configuration only when the folder is trusted and every configuration file in the worktree matches the trusted folder's; otherwise they start restricted without asking. Edits a provider makes during a turn change the digest, so the next start asks again. Claude permission suggestions accepted with "Allow for session" stay in the session and are never written to the folder's settings files. Rust reduces them to allow rules, extra directories, and accepting edits, rescoped to the session; deny or ask rules, rule replacement or removal, directory removal, and other modes such as bypassing permissions are dropped. The approval panel shows exactly those grants, and the decision forwards exactly them, so nothing is granted that the user did not see. When none remain, the request does not offer "Allow for session". Ganbaru never launches editors, so editor folders such as `.vscode` in a received project stay inert until the user opens the folder in their own editor.

## Working-folder authorization

Portable working-folder identity contains no external absolute path. Managed folders resolve to a real directory directly inside the vault `projects/` directory, named by a project ID that must be one folder name; links below the vault root are refused (see [Access control](../access-control.md#working-folder-authority)). External folders resolve through a device-local binding scoped by vault, device, and logical folder ID.

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

Starting a terminal needs a start grant held in Rust memory for the vault, working folder, and execution environment. Only an OS-rendered confirmation that names the folder and shell creates it, so a compromised webview cannot open a shell on its own. The grant lasts for the app launch and is never persisted, because a stored grant would leave that folder's shell open to a compromised webview on every later launch. Unbinding, rebinding, archiving, or removing the folder and revoking an authorization that ran in it drop the grant; archiving one thread does not. Terminals require a persisted open thread in the requested folder or an unsaved draft thread that Rust reserved for that folder; unknown thread IDs are rejected.

Input, output, scrollback, event queues, and process lifetime are bounded. Pasted text loses control and escape characters before it reaches the shell, and Rust rejects input that closes bracketed paste anywhere but at its end, so clipboard content cannot leave paste mode and run as typed input. Live output reaches only the views attached to that terminal, each through its own channel, and a view that stops acknowledging output stops receiving it and later replays scrollback, so no other window sees the output and a stalled view cannot queue it without bound. Terminal output is untrusted display data; escape sequences and link detection must not trigger an unreviewed native action. Stopping, revoking, changing target, or closing the owning scope terminates or quarantines the terminal, and a stale handle cannot attach to a newer run. A running terminal blocks vault transfer, restore, and rollback, and no terminal starts while a vault operation holds the vault, so a shell cannot write into a vault that is being copied or replaced.

## Internal MCP host tools

Each native Chat provider session may receive an ephemeral loopback MCP endpoint with a small application-owned method allowlist, authenticated by a random bearer token and accepting only loopback hosts and origins. It is not a general localhost service and not an organizational participant. It runs in one of two modes.

In a direct session (a person chatting with a provider in a thread), the endpoint offers the preview tools and the thread's attached resources. The endpoint and its token live as long as the provider session, but tool calls and resource reads are admitted only while a turn the app dispatched is running. Completion, abort, interruption, a failed dispatch, and provider exit close them, so a process the model left running cannot drive the preview between turns. Listings stay open so providers can discover tools when the session starts.

In an organizational run (a teammate assignment), every request validates the run-scoped credential, method, bounded arguments, current run and thread, channel membership, access-profile revision, execution target, and revocation state. Tools that change a folder require the source's frozen runtime approval to be auto-approve or unattended. The endpoint closes with the run, and replayed credentials or calls after interruption or revocation fail.

Results exclude credentials, external absolute paths, and unrelated vault content. A future external MCP service requires separate authorization and cannot reuse this endpoint.

## Event ingestion and diagnostics

Provider event size, nesting, sequence, identifiers, and payload shape are bounded before persistence. Unknown event kinds become typed unsupported data or safe diagnostics, never dynamically executed.

Diagnostics exclude secret-like fields, authorization material, unnecessary home paths, provider environments, and credentials. Persisted errors explain the application boundary without becoming a second transcript of sensitive provider output.

Projection changes commit before the frontend is notified, and notifications carry identity and invalidation data, not unbounded payloads.

## Attachments and context

Attachment import rejects symlink sources, unsupported signatures, invalid text, traversal, and oversized files. Bytes are hashed and written with restrictive permissions below the managed Chat asset root, and metadata stores the managed relative identity rather than the source path. Message and draft references own retention, and deferred cleanup rechecks references before removal.

Mentions, workspace files, Notes, tasks, channel messages, browser artifacts, and other context sources are resolved under current authority. Natural-language instructions or provider requests cannot introduce a hidden source. Audience restrictions still apply after content is summarized or transformed.

## Browser previews

Preview URLs and Chat links opened externally must be HTTP or HTTPS with a host and no embedded credentials; other schemes are rejected in Rust, never offered for confirmation. Loopback targets (local development servers) open directly, and the in-app confirmation before other preview destinations is a guard against mistakes, not a security control: an external page gains model access only through the Rust-held origin grant below. Navigation stays within the opened origin and preview pages cannot open new windows. Each thread keeps a loopback origin allowlist keyed by scheme and port (every loopback host spelling counts as one origin), because another local port is another service, such as a database console or a provider server. Opening or typing a loopback URL in the Browser panel adds its origin; a page or the model reaching a loopback origin the thread has not allowed is blocked and shows a one-click approval in the Browser panel, and the model is told the navigation waits for that approval. Hosts Tauri uses for its own protocols on Windows (`.localhost` subdomains on the default port, such as `tauri.localhost` and `ipc.localhost`) are rejected even after confirmation; a `.localhost` subdomain with an explicit port is an ordinary server. App documents send `frame-ancestors 'none'` so a previewed page cannot frame the app, and app commands are granted only to the app's own webviews, never to preview webviews.

Model preview tools (snapshot, screenshot, click, type, key presses, scrolling, and waits) act only on loopback pages or on external origins the user allowed for that thread from the Browser panel. Opening an external page for the user does not grant the model access to it, because a logged-in page would otherwise give a prompt-injected model the user's session. Every call resolves one tab and checks its live URL, its navigation allowance, and the last document that finished loading, so a navigation in flight never exposes the page it is leaving; waits recheck on every poll. Status reads show only scheme and host, without the page title, for pages the model may not use. Origin grants are exact scheme, host, and port, stored device-locally per thread with named limits, and revocable in Chat permission settings.

Preview pages have network access, so the preview tools are a network path out of a provider sandbox. When a session's provider reports that the turn's sandbox denies network access, the first preview tool call asks the user once per native session from the Browser panel and returns a waiting-for-approval error to the model until the user allows it; the tools are never silently disabled. Calls made before the provider reports the turn's policy wait briefly for it, then return a retryable busy error.

Webview identity, port, generation, navigation events, screenshots, recordings, and cleanup are tied to the owning thread and target, so a stale preview cannot emit current events. Tab ids are caller-chosen, so opening or storing a tab whose id another thread already owns, live or only in SQLite, is refused instead of overwriting that thread's tab. Page-controlled titles and URLs are bounded to the stored limits before they reach runtime state, SQLite, or the model, and each tab persists them through one coalescing writer that skips unchanged values and logs failures. Browser artifacts are bounded managed assets with explicit references and cleanup. A model resize is clamped to the inner size of the window that holds the preview and to the capture pixel budget, and the applied size is returned; every capture checks the webview area against that budget (an 8K viewport) before the platform snapshot allocates it. Model screenshots run one at a time per thread under a rolling 24-hour per-thread count and byte quota that refuses before anything is written; user captures and recordings are not counted against the model. Cookies, local storage, and authenticated browsing are not imported into provider context.

Preview webviews never share web data with the app's own webviews (main and detached windows, the Music YouTube embed) or with another working folder. Each working folder gets a persistent data directory under the app config directory, never inside the vault, so preview logins survive restarts but a service worker or cookie set for one folder's dev server cannot reach another folder on the same port. The directory is named by a random token in Chat device state; unbinding or removing the folder, or Clear preview data in Chat permission settings, clears open tabs' data, replaces the token, and deletes the directory. Replacing the token matters because Linux keeps each WebKit web context, and its in-memory session, alive until the app exits. Directories a failed delete left behind are removed before the vault's first preview in the next app run. Each previewed folder costs one WebKit network process on Linux until exit.

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

Checkpoint indexes, review object stores and patch spools, and source-control working directories live in the system temporary directory, each in a new directory with a random name that only the current user can access (mode 0700 on Unix; Windows uses the per-user temporary directory). Another local user can neither read the repository content they hold nor pre-create or redirect them.

Whether a finished assignment is ready for review comes from the files recorded for its turn, and in a Git folder those include the post-turn checkpoint diff, which is captured before the completion is recorded. A provider therefore cannot skip review by omitting changed files. The diff covers the whole folder since the thread's previous checkpoint, so concurrent edits by others can also mark work for review. Folders without Git rely on the files the provider reports.

Restore is a scoped Git or filesystem operation, not a database rollback. It cannot change organizational messages, memberships, approvals, or another project's files. Previewing and applying a restore are separate authority checks. Native work runs outside database transactions, and retryable cleanup records prevent a failed step from appearing successful.

## Revocation

When membership, history, profile, target, runtime, scratch, or audience authority is reduced, new native actions fail immediately. A continuation that already materialized restricted context is interrupted and discarded or quarantined before reuse.

Archiving the project or a run's folder, unbinding that folder, or rebinding it to another directory revokes the active runs that execute in or were granted the folder and stops their providers. Provider events are checked against the same live authority; the first event that fails it, rather than a database error, revokes the run and stops the provider instead of only rejecting the event.

Provider stop, tool shutdown, terminal, preview, and scratch cleanup have bounded deadlines. Incomplete cleanup persists as a retryable record without weakening the revocation or exposing native paths.
