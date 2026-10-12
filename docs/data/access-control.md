# Chat access control

This document is the normative authorization specification for organizational Chat: who may read channel history, which resources a run may use, how authority is reduced, and what happens after restricted context has already been materialized. Product presentation belongs in the [Chat feature documents](../features/chat/README.md). Security boundaries are indexed in [Security](security/README.md).

**Status: Implemented** for project-owned channels (including each project's built-in general channel), memberships, access profiles, working-folder and scratch execution, runtime approvals, internal host tools, and revocation. Cross-project groups and direct-message surfaces are planned.

## Principles

1. Identity is not authority. Creating an AI agent does not create membership, history access, a folder grant, scratch scope, or ambient context.
2. The local owner is the only required seeded participant. There is no privileged default AI agent.
3. Provider, model, role, instructions, and runtime defaults are replaceable configuration on an ordinary agent identity.
4. Channel presence, readable history, and executable resources are separate decisions.
5. Access profiles are reusable ceilings and defaults. They are not principals and cannot grant authority without membership and an exact resource grant.
6. Authority is closed world. Missing, stale, fabricated, revoked, or unsupported authority is denied.
7. A mention, message link, task link, or provider reference identifies context. It never transfers authority.
8. Denial and revocation override convenience defaults, provider-native trust, cached context, and previous approval.

## Principals and scopes

The portable vault owns stable identities for the local owner and configured AI agents. Provider identities are not application principals. Planned human collaborators are principals with their own key-based identity, reachable only through accepted contacts, as specified in [Contacts and invitations](../features/collaboration/README.md).

Channel membership answers whether a participant may appear in, read, or contribute to one channel. It never implies access to another channel in the same project. Resource grants answer a separate question: what the participant or assigned run may do with a working folder, scratch generation, terminal, Git repository, attachment, preview, or internal application tool.

Human roles (owner, administrator, member, guest, or custom) are presets over the same capability vocabulary as agent access profiles, plus management capabilities that agents never hold. A role granted on a group is the default for its projects, and a project role is the default for its channels; each lower level may narrow the inherited role and never widens it. An agent owned by a collaborator acts within the intersection of that collaborator's role and the agent's access profile.

Every decision is scoped to the active vault. IDs copied from another vault authorize nothing.

## Effective authority

Effective authority is the intersection of every applicable restriction: active vault and device, an active participant, current channel membership and its history boundary, the selected access-profile revision, the exact resource grant, the resolved execution target, runtime approval and provider capability, application-wide security ceilings, current filesystem identity for external folders, and any denial or revocation.

No layer may widen another. A provider that supports unrestricted filesystem access still receives only the folder the application granted. A profile that permits edits does not create an edit grant. A folder grant does not expose channel history.

Authorization is checked when work is assigned and again at every privileged boundary. A long-lived continuation cannot rely on a decision made before a membership, profile, binding, or approval changed.

## Channel capabilities and history

An agent membership stores two explicit channel capabilities, reading history and participating, plus a history boundary. Each may inherit from the selected profile or be set on the membership.

The history boundary is either the entire retained history or a lower bound fixed when access was granted. Adding an agent does not automatically disclose earlier messages. The bound applies to reads, context construction, search, references, summaries, exports, and internal tools.

Archiving a channel stops new ordinary activity but does not erase authorized history and is not a substitute for revocation. Personal channel sections and last-selected-channel state are device-local presentation preferences and never change organizational visibility.

## Access profiles and revisions

An access profile sets default channel capabilities, a default history boundary, and a maximum folder capability. Built-in profiles cover the common ceilings (conversation only, read only, edit files, build and test, publish changes); users may define others.

Every assignment records the profile revision used for its authorization decision. Editing a profile creates a new revision and never rewrites the meaning of an earlier run. A stricter revision may revoke or interrupt current work. A more permissive revision does not silently upgrade an already authorized continuation.

## Working-folder authority

Folder authority is an ordered ceiling: none, read, edit, execute (terminal and build commands), and publish (Git publication). Each level includes the levels below it, and the effective level is the lowest of the membership grant and the profile ceiling. Terminal approval never grants secondary folders.

Managed project folders resolve from the active vault and a validated relative path. External folders resolve through device-local bindings scoped by vault ID, device ID, and working-folder ID.

Before privileged use, Rust reopens or canonicalizes the target and checks its filesystem identity. Symlinks, traversal, stale bindings, replaced directories, and external bindings pointing at or inside the active vault are rejected. Git operations additionally validate the Git common-directory identity; a mismatch revokes Git authority without necessarily revoking ordinary access to a still-matching folder. Every file path is relative to the authorized root and validated again at the operation boundary.

## Runtime approvals

Actions with materially different impact need distinct approval: reading a bounded file, writing a file, starting a terminal, mutating Git, launching an external application, opening a URL, and using a sensitive application tool are not interchangeable.

The approval policy (ask, auto-approve, unattended, or provider-defined) comes from the membership or folder grant. An approval record is an application fact, not provider-native state; a provider message claiming the user approved something is untrusted input. Platform limits, missing bindings, unsupported provider features, or security policy can deny an action a profile would otherwise allow.

## Provider enforcement

Rust owns provider process creation, transport, event ingestion, cancellation, and shutdown. Provider configuration supplies nonsecret settings and opaque credential references; secret material is resolved only at the native boundary that needs it.

Provider sandbox or trust configuration is defense in depth and cannot widen organizational authority. The provider receives one resolved workspace and a bounded environment. Host tools independently validate the run, channel, membership, profile revision, target, arguments, and revocation state.

Provider output, changed-file reports, terminal output, model labels, protocol events, URLs, and errors are untrusted. Persisting or displaying them grants nothing.

## Execution targets

Planning and discussion may remain targetless. Native filesystem, terminal, Git, checkpoint, restore, preview, or process work requires exactly one execution target, resolved in this order:

1. Retain the locked target of an active compatible continuation.
2. Use the target explicitly selected during assignment review.
3. Infer a target only when all executable references resolve to one eligible environment.
4. Use the membership's valid default folder when one exists.
5. Otherwise remain targetless and request a choice before native work.

A run cannot combine a working folder and a scratch generation as coequal roots. Any future secondary folder access requires its own explicit capability and path boundary and must not be smuggled through attachments, links, environment variables, or provider configuration.

## Private scratch

Scratch is a private execution target, not an unowned temporary directory. Each generation has a stable logical identity, one owning scope, lifecycle state, and cleanup record; its native path stays device-local.

Scratch content is not published because a run completed. Promotion into a working folder or managed attachment is explicit, preserves provenance, checks the destination grant, and revalidates every file. Replacing a generation revokes the old one for new work, and a continuation that materialized the old scratch is never silently rebound to the new one.

Scratch reuse resolves the agent through the run's assignment and authorization revision. The generation must belong to that agent and destination, remain active, and satisfy every retained-source restriction.

## Attachments, references, and derived context

Attachments are imported through Rust into a managed, bounded store, and message or draft references control retention. Referencing an attachment from another channel requires authority over both the source and the destination audience.

Structured references store identity and provenance, not embedded access. Resolving a message, file, Note, task, checkpoint, or provider event repeats the current authorization check. A caller who cannot read the source receives neither the content nor a revealing preview.

Search results, summaries, mention suggestions, unread counts, context packages, exports, and AI prompts are derived context and apply the same membership, history, resource, and audience restrictions as canonical reads. A cache key includes every authorization dimension that affects its content.

## Strict disclosure

Information may move between channels only when the destination audience is a subset of the source's authorized audience, or when an explicit declassification creates new content under the destination policy. A common participant is not enough: the system compares effective readable audiences, history bounds, and resource restrictions. Changing a membership is validated against references the destination already retains. AI-generated summaries do not escape this rule.

## Internal host tools

Native Chat sessions receive an ephemeral loopback MCP endpoint exposing only application tools authorized for the current run. The endpoint is infrastructure, not a participant.

Every call validates an unguessable run-scoped credential, a method allowlist, bounded arguments, and the current assignment, target, profile revision, membership, and revocation state. Tools return bounded DTOs without secrets, external absolute paths, or unrelated vault data. The endpoint ends with the run or earlier revocation.

Planned external MCP and CLI integrations require separate user authorization and must not reuse an internal endpoint or infer rights from an organizational identity.

## Revocation and materialized context

Reducing membership, history, profile, folder, runtime, or audience authority takes effect immediately for new reads and privileged actions.

If an active continuation has already received now-restricted content, logical filtering is insufficient. The application must:

1. Mark the affected scope revoked.
2. Interrupt active runs.
3. Reject later host-tool calls and publication.
4. Stop the provider session within a bounded deadline.
5. Discard or quarantine the native continuation.
6. Require a fresh authorization revision before resuming.
7. Schedule retryable cleanup when immediate cleanup cannot complete.

Revocation cannot make an external provider forget data it already received. The product avoids sending unnecessary context and discloses this limitation where external services are used.

## Persistence boundary

Portable SQLite owns identities, memberships, capabilities, history bounds, profile revisions, logical folder grants, assignments, authorization revisions, scratch generations, command receipts, and revocation jobs. The platform config directory owns external absolute paths, executable paths, provider homes, native process state, and filesystem identity material. Secrets stay in native credential storage. Planned concurrent sync must preserve grant scope and revision history and never synchronizes device-local bindings or secrets.

## Required test matrix

Authorization tests must cover at least:

- Missing, stale, expired, fabricated, and revoked IDs.
- Membership without history access, and history boundaries at exact ordinals.
- Profile permission without a resource grant, and a grant above the profile ceiling.
- Targetless planning followed by denied native work.
- Managed and external folder identity replacement.
- Traversal, symlink, absolute-path, and Git common-directory escape.
- Provider attempts to exceed application authority.
- References, summaries, search, attachments, and exports across audience boundaries.
- Profile or membership reduction during an active run.
- Scratch replacement, promotion, and failed cleanup.
- Replayed host-tool credentials and calls after run termination.

Tests assert both denial and absence of leaked metadata. A safe error must not reveal that a restricted resource exists.
