# Chat schema

Chat persistence separates organizational conversation, provider execution, authorization, workspace identity, and derived presentation. This keeps a project channel stable when a provider session fails or is replaced and prevents provider-native trust from becoming application authority.

Authorization semantics are normative in [Chat access control](../access-control.md). This document describes durable ownership and relationships.

## Organizational layer

The current organization layer is project-owned channels. Each project has a built-in general channel and may have additional navigable or archived channels. Channel identity, name, archive state, ordering, and membership are independent of any provider.

Projects and working folders replicate between devices, so Chat creates a project's general channel and each folder's current-folder execution environment (never for an archived folder) on demand, when a device that writes Chat lists channels, environments, or assignment targets, or resolves a run target. The general channel's ids derive from the project id, so devices agree on them; fresh vaults therefore seed no conversations. A device that cannot write Chat shows only stored rows. A conversation has exactly one channel.

Organizational ids are a kind prefix and 128 random bits, or derive from their inputs when devices must agree: a reply thread's id derives from its root message and a work assignment input's id from its message. They fit the 128-byte sync row key bound. Ordinary commands archive or end organizational rows instead of deleting them: agents are archived, and an agent's channel membership ends with a removal timestamp that every reader filters. Device-local rows that reference organizational rows cascade when that row is deleted.

Membership records participant identity, capability, history boundary, defaults, and current authorization revision. Personal navigation sections and last-selected channel are device-local presentation state rather than shared organization membership.

Canonical messages and coordination events retain channel identity, author or actor identity, sequence, reply or reference provenance, and delivery state. A provider transcript is not a replacement for this history.

Broader groups, cross-project workspaces, and direct messages are future schema work unless a later migration and feature specification introduce them.

## Channel and provider-session links

A channel can link several provider sessions over time in a stable order. The link explains which execution continuation contributed to which organizational scope and when it was active. Replacing or handing off a provider session creates or updates linkage without rewriting earlier channel history.

Provider thread rows store the continuation and execution timeline understood by the application. Provider-native IDs are opaque handles. They do not encode membership, folder authority, audience, or ownership.

Provider configuration and model mapping are separate from provider sessions. Changing a configured provider does not mutate historical messages or sessions.

## Canonical events and projections

Provider events are validated, bounded, normalized, and appended as canonical application events. Sequence and projection changes commit before a lightweight frontend notification is emitted. Event, item, and request IDs that a provider numbers per process are scoped to the provider session, and a projection row keyed by a provider item ID is only ever updated by events of its own thread.

The combined timeline, turn state, message view, command activity, usage summary, and inspector data are projections. They may be persisted for performance but remain rebuildable from canonical events and organizational messages. Projection rebuild preserves channel and authorization scope.

Provider-reported changed paths are converted to canonical relative paths only after verifying that they remain below the authorized workspace. Absolute paths, traversal, malformed paths, duplicates, and secret-bearing diagnostics are removed at ingestion.

## Assignments and execution targets

An organizational thread or run may remain targetless for discussion and planning. Current schema explicitly permits targetless Chat assignments. Before native work begins, the application locks the run to exactly one authorized target, either:

- A managed or externally bound project working folder.
- A private scratch generation.

A run cannot mix both. Target consistency constraints and service validation ensure that a selected folder or scratch environment belongs to the expected project and logical scope.

Working-folder identity is portable. Managed paths are relative to the active vault. External absolute bindings, executable paths, Git identity, and current filesystem fingerprints remain device-local.

Scratch generations have logical identity, ownership, lifecycle, provenance, and cleanup state. Their native paths remain device-local. Promotion into a managed folder is an explicit transfer, not a target mutation.

## Access profiles and authorization revisions

Portable rows store participants, memberships, channel capabilities, history cutoffs, reusable access profiles, immutable or historically interpretable profile revisions, exact resource grants, assignments, approvals, authorization revisions, revocations, and command receipts.

Historical runs retain the revision and target under which they were authorized. A later profile edit does not retroactively change that meaning. A reduction may revoke active work, but a more permissive revision does not silently upgrade it.

Participants, AI teammates, teammate policy revisions, access profiles, and profile revisions replicate. Revision rows are immutable and carry no stored number: the displayed revision counts the rows of the teammate or profile in `(created_at, id)` order. A teammate and a profile name their current revision with a pointer that the command writing a revision sets, together with the revision's safety mode or folder capability ceiling, so concurrent edits resolve to the more restrictive revision (a Custom safety mode ranks least restrictive, because the provider's own configuration governs it); readers use the pointed revision, or the newest arrived one while it is missing. A pointer whose copied safety mode or capability ceiling disagrees with the revision it names, which only a corrupt or dishonest operation can write, gives its teammate or profile no effective revision, so the teammate is refused until an honest edit replaces it; the materialize hook revokes the affected teammates' authorizations and logs the pointer instead of failing the apply, which would stop every later apply. A teammate records the device that created it as its home device; no reader uses it yet. Commands refuse a teammate or custom profile name the device already holds, but devices that edit concurrently can end up with equal names, so readers tell them apart by id. After sync applies these rows, a materialize hook creates the device-local access state of arrived teammates, bumps the audience and access counters an arrived profile revision affects, and revokes authorizations a narrowing revision no longer allows; on desktop the revocation drain then stops their runs.

Conversations, channels, conversation memberships, AI channel memberships, and working folder grants replicate too. Memberships and grants carry a `sync_key` derived from their key columns, so devices that add the same membership or grant concurrently write one row; opening a vault converts older rows to derived ids and keys before sync starts. Concurrent edits of one membership or grant keep one whole value: a removal or revocation first, then the access that explicitly clears more of history reading, participation, and full history or the lower folder capability, then the stricter runtime approval override. Inherited values rank as the widest they can resolve to: an inherited capability as granted, and no approval override as less strict than any explicit one. That holds for the channel override, which falls back to the teammate's default and can only narrow it, but a folder or scratch override replaces the channel override, so a concurrent clear that would inherit a stricter channel override still loses to a looser explicit folder or scratch override; the merge cannot see the other side's value, so this stays open until the engine exposes it, and it cannot occur while Chat writes stay owner-only, because replicas do not write Chat rows until then. The access profile a channel membership names is not ranked yet, so concurrent edits that differ only in their profile keep either. An edit made after seeing a removal replaces it, so a re-add converges to active. A concurrent archive of a channel or conversation outranks an unarchive. After sync applies membership or grant rows, the materialize hook bumps the conversation audience and the teammate's access counter, and revokes the teammate's active authorizations that the current rows no longer cover, resolving the approval override that governs each one in the runtime's precedence (scratch or folder override, then channel override), so a widening or a repeated apply revokes nothing. A deleted membership, grant, or conversation, including one removed by a cascade, no longer names its teammate, so the hook checks every teammate with an active authorization the same way. Deleting a conversation also cascades away the authorizations it was the destination of and their runs, so no revocation job can name them; the hook reports the deleted conversation, a local command whose delete cascades into conversations (deleting a project group) wakes the drain after committing, and on desktop the revocation drain stops every active run whose host-tool scope names an authorization that no longer exists, keeping the scope until the provider's shutdown succeeds so a failed shutdown is retried. Android never holds an active authorization, because runs execute only on desktop.

The channel timeline replicates as well: reply threads, conversation items, communication messages, their immutable revisions, message references with their target rows, and conversation and reply thread read cursors. Items order by `(order_key, id)` within the root timeline or one reply thread, so concurrent posts that take one order key read in the same order on every device; page, unread, history, and channel reference range comparisons use that pair. A history granted from a point stores the order key after the channel's last root item, and a channel reference range stores its lower and upper positions. Reply counts are counted, and revision display numbers count the message's revisions in `(created_at, id)` order. Read cursors carry a derived `sync_key` and only move forward, so a concurrent earlier read never moves another device's cursor back. A revision's Markdown is bounded to 128 KiB and its rich content to 192 KiB so one revision fits an operation; projected provider output longer than the Markdown bound is cut at a character boundary and its rich content marks it truncated, which the timeline shows. Search rows follow local writes through insert triggers that stay quiet while sync applies, the Chat materialize hook rebuilds them for applied messages and revisions, and a deleted message leaves the index on every path. Mention targets keep unchecked ids, so a deleted folder leaves its targets to render their label snapshots.

Resource IDs and references are not grants. Every privileged service resolves current effective authority from the stored relationships and device-local target state.

## Drafts, attachments, and context

Drafts belong to an exact organizational scope and may reference managed attachments. Imported bytes live in the `chat/attachments` asset class of the active vault, named `{sha256}.{ext}`, so attachments with equal bytes share one file; source absolute paths are not persisted. A message revision holds at most 20 attachments as rows owned by the revision, each carrying the attachment by value (id, working folder, kind, display name, content type, size, and digest), so the attachment travels with the revision and its file moves as a managed asset; an empty text attachment names no asset. `chat_attachments` rows are device-local: the materialize hook creates one for each attachment an applied revision names whose working folder this device holds, and opening a vault renames older files to their digest names.

A draft save names the stored version it was based on, and Rust refuses it when another window has saved since. The composer then shows the stored version and says that it replaced recent local edits. A send stores its snapshot unconditionally because the send already acted on it. Each save tells the other windows of the vault, and a composer showing that draft reloads it while it has no unsaved edits.

Thread and channel mutations carry revision checks. After one succeeds, the window tells the other windows of the vault which threads and channels changed, and they reread those records and the open channel and reply thread. Receiving windows never announce the reread, so windows do not echo each other.

Message, scheduled message, draft, and queued reference rows control attachment retention. Chat never deletes attachment files: deferred cleanup removes a `chat_attachments` row only when no reference names its digest, and the sync asset sweep removes a file once no replicated reference and no device-local attachment row names it. Browser artifact cleanup still removes its files, rechecking current references first, treating an already missing file as a recoverable result, and recording bounded retry state.

Context packages, mentions, workspace file references, channel references, and selected Notes or tasks store identity and provenance. Resolving them repeats current authorization. Materialized provider context records enough scope and revision to support revocation and continuation disposal.

## Checkpoints, review, and restore

Git checkpoints, changed-file review, patches, and restoration retain exact run, thread, working-folder, authorization, and Git-storage identity. They do not identify a repository only by remote URL, branch name, or mutable path.

Checkpoint capture and restore use bounded native work around explicit database phases. A failed filesystem or Git operation leaves a recoverable status and cleanup record rather than falsely committing success.

Review artifacts are derived from authorized repository state. Persisted patch or preview data is bounded and scoped. Restoring an older checkpoint does not roll back organizational messages, membership, approvals, or unrelated vault data.

## Terminals, previews, and workspace observation

Terminal sessions, browser previews, internal file tools, and workspace observers retain exact thread and execution-target identity. Native process handles, ports, absolute paths, and watcher state are device-local runtime data.

Only one mutable provider driver generation owns an active thread at a time. Stopped or mismatched generations cannot append ordinary projection events. Workspace observation coalesces bounded semantic events and uses typed invalidation on overflow instead of persisting an unbounded path stream.

Preview and terminal metadata retained in SQLite is organizational evidence or resumable presentation state, not ambient process authority. Restart requires fresh target and authorization checks.

Each model preview tool call in a direct session has an app-owned audit row keyed by its turn, with the tool name, an argument hash, the outcome, and the result size; the rows also hold that turn's call and byte budget and are deleted with the turn. Organizational host tool calls keep their own audit rows keyed by the authorization revision.

## Coordination and command receipts

Scheduling, follow-ups, interaction responses, handoffs, and other coordination commands have stable request or receipt identity where retry can occur. Replaying the same accepted command cannot duplicate a message, assignment, or provider action. A queued follow-up keeps its thread for life: saving it again replaces only that thread's still queued follow-up, and a sent or cancelled one is never revived.

Canonical organization state commits before process notifications. If process delivery fails, the retry state identifies the intended command without inventing another organizational event.

## Cleanup and retention

Provider sessions, attachments, scratch generations, checkpoints, browser artifacts, and native processes have different retention and cleanup requirements. Cleanup records state the exact logical target, reason, attempt state, and safe retry information without exposing secrets or unnecessary absolute paths.

Deletion of a provider continuation does not cascade into the channel. Archiving a channel does not by itself delete provider-native state.

Chat foreign keys into Projects (projects and project working folders) cascade on delete, so deleting a project removes the same Chat rows on every replica instead of being refused by whichever row a device happens to hold. Removing an external working folder is still refused while Chat references it, through an explicit in-use query. Audit rows (authorization revisions and their sources, scratch generation sources, and scratch promotions) stay immutable while their parents exist and are removed only by that cascade. The command that deletes projects collects their browser artifact paths before the transaction and removes those files after it commits, keeping any path a remaining row still names; their attachment files are left to the asset sweep.

Revocation cleanup has priority over convenience retention. A continuation that materialized restricted context is stopped and discarded or quarantined before reuse, as specified in [Chat access control](../access-control.md).

## Device-local boundary

The active vault database may store logical provider configuration, credential references, provider-native continuation IDs, organizational identities, and portable execution history. It does not store secret values, provider homes, executable paths, probe process state, external absolute folder bindings, native terminal handles, or ephemeral loopback credentials.

The platform app config directory stores device-local bindings and nonsecret runtime bootstrap data. Secrets are resolved from the operating-system credential store only at the native boundary that needs them.

LAN sync replicates Chat identity and policy rows (participants, AI teammates, policy revisions, access profiles, and profile revisions) and conversation access rows (conversations, channels, memberships, AI channel memberships, and working folder grants), and the channel timeline (reply threads, items, messages, revisions, references, and read cursors); replicating the rest of the portable organization and authorization records is later work. Device-local bindings, process state, and credentials never replicate.
