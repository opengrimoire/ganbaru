# Chat schema

Chat persistence separates organizational conversation, provider execution, authorization, workspace identity, and derived presentation. This keeps a project channel stable when a provider session fails or is replaced and prevents provider-native trust from becoming application authority.

Authorization semantics are normative in [Chat access control](../access-control.md). This document describes durable ownership and relationships.

## Organizational layer

The current organization layer is project-owned channels. Each project has a built-in general channel and may have additional navigable or archived channels. Channel identity, name, archive state, ordering, and membership are independent of any provider.

Membership records participant identity, capability, history boundary, defaults, and current authorization revision. Personal navigation sections and last-selected channel are device-local presentation state rather than shared organization membership.

Canonical messages and coordination events retain channel identity, author or actor identity, sequence, reply or reference provenance, and delivery state. A provider transcript is not a replacement for this history.

Broader groups, cross-project workspaces, and direct messages are future schema work unless a later migration and feature specification introduce them.

## Channel and provider-session links

A channel can link several provider sessions over time in a stable order. The link explains which execution continuation contributed to which organizational scope and when it was active. Replacing or handing off a provider session creates or updates linkage without rewriting earlier channel history.

Provider thread rows store the continuation and execution timeline understood by the application. Provider-native IDs are opaque handles. They do not encode membership, folder authority, audience, or ownership.

Provider configuration and model mapping are separate from provider sessions. Changing a configured provider does not mutate historical messages or sessions.

## Canonical events and projections

Provider events are validated, bounded, normalized, and appended as canonical application events. Sequence and projection changes commit before a lightweight frontend notification is emitted.

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

Resource IDs and references are not grants. Every privileged service resolves current effective authority from the stored relationships and device-local target state.

## Drafts, attachments, and context

Drafts belong to an exact organizational scope and may reference managed attachments. Attachment metadata is portable; imported bytes live below assets/chat/attachments in the active vault. Source absolute paths are not persisted.

Message and draft reference rows control attachment retention. Deferred cleanup rechecks current references before deletion, treats an already missing managed file as a recoverable cleanup result, and records bounded retry state.

Context packages, mentions, workspace file references, channel references, and selected Notes or tasks store identity and provenance. Resolving them repeats current authorization. Materialized provider context records enough scope and revision to support revocation and continuation disposal.

## Checkpoints, review, and restore

Git checkpoints, changed-file review, patches, and restoration retain exact run, thread, working-folder, authorization, and Git-storage identity. They do not identify a repository only by remote URL, branch name, or mutable path.

Checkpoint capture and restore use bounded native work around explicit database phases. A failed filesystem or Git operation leaves a recoverable status and cleanup record rather than falsely committing success.

Review artifacts are derived from authorized repository state. Persisted patch or preview data is bounded and scoped. Restoring an older checkpoint does not roll back organizational messages, membership, approvals, or unrelated vault data.

## Terminals, previews, and workspace observation

Terminal sessions, browser previews, internal file tools, and workspace observers retain exact thread and execution-target identity. Native process handles, ports, absolute paths, and watcher state are device-local runtime data.

Only one mutable provider driver generation owns an active thread at a time. Stopped or mismatched generations cannot append ordinary projection events. Workspace observation coalesces bounded semantic events and uses typed invalidation on overflow instead of persisting an unbounded path stream.

Preview and terminal metadata retained in SQLite is organizational evidence or resumable presentation state, not ambient process authority. Restart requires fresh target and authorization checks.

## Coordination and command receipts

Scheduling, follow-ups, interaction responses, handoffs, and other coordination commands have stable request or receipt identity where retry can occur. Replaying the same accepted command cannot duplicate a message, assignment, or provider action.

Canonical organization state commits before process notifications. If process delivery fails, the retry state identifies the intended command without inventing another organizational event.

## Cleanup and retention

Provider sessions, attachments, scratch generations, checkpoints, browser artifacts, and native processes have different retention and cleanup requirements. Cleanup records state the exact logical target, reason, attempt state, and safe retry information without exposing secrets or unnecessary absolute paths.

Deletion of a provider continuation does not cascade into the channel. Archiving a channel does not by itself delete provider-native state. Project deletion must explicitly plan organization history, managed assets, working folders, and user-authored files.

Revocation cleanup has priority over convenience retention. A continuation that materialized restricted context is stopped and discarded or quarantined before reuse, as specified in [Chat access control](../access-control.md).

## Device-local boundary

The active vault database may store logical provider configuration, credential references, provider-native continuation IDs, organizational identities, and portable execution history. It does not store secret values, provider homes, executable paths, probe process state, external absolute folder bindings, native terminal handles, or ephemeral loopback credentials.

The platform app config directory stores device-local bindings and nonsecret runtime bootstrap data. Secrets are resolved from the operating-system credential store only at the native boundary that needs them.

Remote synchronization is not implemented. A future sync layer may synchronize portable organization and authorization records, but never device-local bindings, process state, or credentials.
