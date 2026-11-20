# Chat access control

This document is the normative authorization specification for organizational Chat: who may read channel history, which resources a run may use, how authority is reduced, and what happens after restricted context has already been materialized. Product presentation belongs in the [Chat feature documents](../features/chat/README.md). Security boundaries are indexed in [Security](security/README.md).

**Status: Implemented** for project-owned channels (including each project's built-in general channel), agent channel membership, channel access levels inherited from the project and the app, working-folder and scratch execution, runtime approvals, internal host tools, and revocation. Cross-project groups and direct-message surfaces are planned. Product presentation of access is in [Channel-centered agent configuration](../features/chat/channel-configuration.md).

## Principles

1. Identity is not authority. A provider agent existing in the vault does not create membership, a folder, scratch scope, or ambient context. It acts in a channel only after a person adds it there.
2. The local owner is the only required seeded participant. There is no privileged default AI agent.
3. An agent is a provider identity; its default model is replaceable configuration, and instructions are guidance that never changes authority.
4. Channel presence decides whether an agent may work in a channel; the channel's access level decides what that work may touch.
5. The effective access level is the only agent authority. It never applies without membership, and the provider on the executing device must be able to enforce it.
6. Authority is closed world. Missing, stale, fabricated, revoked, or unsupported authority is denied.
7. A mention, message link, task link, or provider reference identifies context. It never transfers authority.
8. Denial and revocation override convenience defaults, provider-native trust, cached context, and previous approval.

## Principals and scopes

The portable vault owns stable identities for the local owner and one AI agent per detected provider family. Provider identities are not application principals. Planned human collaborators are principals with their own key-based identity, reachable only through accepted contacts, as specified in [Contacts and invitations](../features/collaboration/README.md).

Channel membership answers whether a participant may appear in, read, or contribute to one channel. An agent in a channel reads it the way a person who joins it does, within the bounded context package. Membership never implies access to another channel in the same project. The channel's access level answers a separate question: what an assigned run may do with a working folder, scratch generation, terminal, Git repository, attachment, preview, or internal application tool.

Planned human roles (owner, administrator, member, guest, or custom) add management capabilities that agents never hold. A role granted on a group is the default for its projects, and a project role is the default for its channels; each lower level may narrow the inherited role and never widens it. An agent owned by a collaborator will act within the intersection of that collaborator's role and the channel's access.

Every decision is scoped to the active vault. IDs copied from another vault authorize nothing.

## Effective authority

Effective authority is the intersection of every applicable restriction: active vault and device, an active participant, current channel membership, the channel's effective access level as the executing device's provider can enforce it, the resolved execution target, runtime approval, application-wide security ceilings, current filesystem identity for external folders, and any denial or revocation.

No layer may widen another. A provider that supports unrestricted filesystem access still receives only the folder the application resolved. Instructions at any scope, provider-native trust, and earlier approvals grant nothing.

Authorization is checked when work is assigned and again at every privileged boundary. A long-lived continuation cannot rely on a decision made before a membership, access level, binding, or approval changed.

## Access levels

Access is one level per channel, ordered from narrowest: Talk only (`talk`), Read only (`read`), Ask before commands (`ask`), and Work on their own (`own`). The app settings hold a level that is never empty and defaults to `own`, which a device that cannot confine commands resolves to `ask`; a project and a channel may each hold one or inherit. The effective level is the channel's value, else the project's, else the app's, and every read reports which scope it came from. It applies to every agent in the channel and every non-archived working folder of the channel's project.

Each device resolves the effective level against the native enforcement its provider reports, and never widens it:

| Level | Provider must enforce | Folder capability | Runtime approval | Scratch |
| --- | --- | --- | --- | --- |
| `own` | Confined commands, a writable root, and a network boundary | Publish when publication is classified, else execute | Auto approve | Auto approve |
| `ask` | A writable root, a network boundary, and asking the person before every command | Publish when publication is classified, else execute | Ask | Ask |
| Edit only fallback for `ask` | A writable root, a denied shell, and a network boundary | Edit | Ask | None |
| `read` | A read-only root, a denied shell, and a network boundary | Read | Ask | None |
| `talk` | An isolated conversation | None | Ask | None |

When a level is not enforceable, the device steps down to the next narrower row and records a reason (`cannot_confine_commands`, `cannot_ask_before_commands`, `cannot_read_only`, `cannot_isolate`); when even `talk` is not enforceable, the agent cannot work on that device. Confinement alone does not satisfy `ask`, because Ask before commands promises the person sees each command. The channel shows each agent's resolved behavior on the device, and Work on their own is offered only where it resolves to itself.

Concurrent access edits on linked devices keep the narrower level, and an inherited value never outranks a stored one in that comparison. Lowering the effective level at any scope, or removing an agent from a channel, applies to running work at once: authorizations whose frozen access the new resolution no longer covers are revoked on the device that observes the change, whether a local edit or an applied sync. Raising it applies to work that starts afterward.

Archiving a channel stops new ordinary activity but does not erase authorized history and is not a substitute for revocation. Personal channel sections and last-selected-channel state are device-local presentation preferences and never change organizational visibility.

## Authorization revisions

Every assignment records the authorization revision used for its decision, whose scope digest carries the resolved level, folder capability, runtime approval, and the digest of the frozen instructions. A changed membership or access level creates a new revision and never rewrites the meaning of an earlier run. A stricter revision may revoke or interrupt current work. A more permissive revision does not silently upgrade an already authorized continuation.

## Working-folder authority

Folder authority is an ordered capability: none, read, edit, execute (terminal and build commands), and publish (Git publication). Each level includes the levels below it, and the capability a run holds is the one its resolved access level gives. Terminal approval never grants secondary folders. These capabilities bound agents and their runs; a terminal the user opens is the user's own shell, bound to the thread's authorized folder as described in [Chat security](security/chat.md#terminals-and-processes).

Managed project folders resolve from the active vault as `projects/<folder name>`, using the name this vault claimed for the folder, never a path read from a replicated row. The name must be exactly one portable folder name that does not start with a dot, because Git and several coding agents read configuration from hidden folders in the parents of the folder they run in; the folder is created without following links below the vault root and must canonicalize to a direct child of the vault `projects/` directory, otherwise it stays unbound and is listed with that state. External folders resolve through device-local bindings scoped by vault ID, device ID, and working-folder ID.

Before privileged use, Rust reopens or canonicalizes the target and checks its filesystem identity. Symlinks, traversal, stale bindings, replaced directories, and external bindings pointing at or inside the active vault are rejected. Git operations additionally validate the Git common-directory identity; a mismatch revokes Git authority without necessarily revoking ordinary access to a still-matching folder. Every file path is relative to the authorized root and validated again at the operation boundary.

## Runtime approvals

Actions with materially different impact need distinct approval: reading a bounded file, writing a file, starting a terminal, mutating Git, launching an external application, opening a URL, and using a sensitive application tool are not interchangeable.

The approval policy (ask or auto-approve) comes from the resolved access level: auto-approve only under Work on their own, ask otherwise. Native assignment runs use the frozen policy, and the send boundary rejects any other mode. Pushing to a Git remote is never part of a level and always needs a person's decision or a matching standing push approval, as specified in [Chat security](security/chat.md#push-requests). A person may also let an agent repeat one public web origin, exact command, or tool in a channel without asking, as specified in [Chat security](security/chat.md#remembered-approvals); such an approval lapses when the channel's effective access drops below the access it was granted under. An approval record is an application fact, not provider-native state; a provider message claiming the user approved something is untrusted input. Platform limits, missing bindings, unsupported provider features, or security policy can deny an action a grant would otherwise allow.

## Provider enforcement

Rust owns provider process creation, transport, event ingestion, cancellation, and shutdown. Provider configuration supplies nonsecret settings and opaque credential references; secret material is resolved only at the native boundary that needs it.

Provider sandbox or trust configuration is defense in depth and cannot widen organizational authority. The provider receives one resolved workspace and a bounded environment. Host tools independently validate the run, channel, membership, authorization revision, target, arguments, and revocation state.

The send gate resolves the authorized target to an organizational scope (conversation, scratch, or folder read, edit, execute, or publish), passes that scope to the driver, and admits the run only when the provider reports native enforcement for each boundary the scope needs, following the same rule as the [access level table](#access-levels): auto approval needs confined commands, Ask needs every command relayed to the person, and an edit-only scope needs a denied shell. Publish also needs classified publication, which no provider reports yet. A change of scope always starts a fresh provider session.

Provider output, changed-file reports, terminal output, model labels, protocol events, URLs, and errors are untrusted. Persisting or displaying them grants nothing.

## Execution targets

Planning and discussion may remain targetless. Native filesystem, terminal, Git, checkpoint, restore, preview, or process work requires exactly one execution target, resolved in this order:

1. Use a target the request names explicitly, including private scratch.
2. Otherwise retain the folder the reply thread already works in.
3. Otherwise infer a folder when the request's references resolve to one non-archived folder of the channel's project.
4. Otherwise use the project's primary working folder.
5. Otherwise remain targetless, so the run is conversation only.

Under Talk only every run is targetless and folder references are ignored. Each non-archived folder of the project holds the resolved folder capability, and the target listing marks the primary folder as the default, so the composer and the send gate agree.

A run cannot combine a working folder and a scratch generation as coequal roots. Any future secondary folder access requires its own explicit capability and path boundary and must not be smuggled through attachments, links, environment variables, or provider configuration.

## Private scratch

Scratch is a private execution target, not an unowned temporary directory. Each generation has a stable logical identity, one owning scope, lifecycle state, and cleanup record; its native path stays device-local.

Scratch is available only under Ask before commands and Work on their own, with the runtime approval of the resolved level. Scratch content is not published because a run completed. Promotion into a working folder or managed attachment is explicit, preserves provenance, requires an effective level that edits files in the destination, and revalidates every file. Replacing a generation revokes the old one for new work, and a continuation that materialized the old scratch is never silently rebound to the new one.

Scratch reuse resolves the agent through the run's assignment and authorization revision. The generation must belong to that agent and destination, remain active, and satisfy every retained-source restriction.

## Attachments, references, and derived context

Attachments are imported through Rust into a managed, bounded store, and message or draft references control retention. Referencing an attachment from another channel requires authority over both the source and the destination audience.

Structured references store identity and provenance, not embedded access. Resolving a message, file, Note, task, checkpoint, or provider event repeats the current authorization check. A caller who cannot read the source receives neither the content nor a revealing preview.

Search results, summaries, mention suggestions, unread counts, context packages, exports, and AI prompts are derived context and apply the same membership, history, resource, and audience restrictions as canonical reads. A cache key includes every authorization dimension that affects its content.

## Strict disclosure

Information may move between channels only when the destination audience is a subset of the source's authorized audience, or when an explicit declassification creates new content under the destination policy. A common participant is not enough: the system compares effective readable audiences and resource restrictions. Changing a membership is validated against references the destination already retains. AI-generated summaries do not escape this rule.

## Internal host tools

Native Chat sessions receive an ephemeral loopback MCP endpoint exposing only application tools. The endpoint is infrastructure, not a participant.

For an organizational run, every call validates an unguessable run-scoped credential, a method allowlist, bounded arguments, and the current assignment, target, authorization revision, membership, and revocation state, and the endpoint ends with the run or earlier revocation. A direct session keeps one session token and admits preview tool calls and resource reads only while a dispatched turn runs. Tools return bounded DTOs without secrets, external absolute paths, or unrelated vault data. [Chat security](security/chat.md#internal-mcp-host-tools) owns the details.

Planned external MCP and CLI integrations require separate user authorization and must not reuse an internal endpoint or infer rights from an organizational identity.

## Revocation and materialized context

Removing an agent from a channel, lowering the effective access level at any scope, or reducing audience authority takes effect immediately for new reads and privileged actions.

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

Portable SQLite owns identities, memberships, access levels and instructions at app, project, and channel scope, assignments, authorization revisions, scratch generations, push and remembered approvals, and command receipts; revocation jobs are device-local rows. The platform config directory owns external absolute paths, executable paths, provider homes, native process state, and filesystem identity material. Secrets stay in native credential storage. Sync preserves access levels with the narrower-wins merge and revision history, and never synchronizes device-local bindings or secrets.

## Required test matrix

Authorization tests must cover at least:

- Missing, stale, expired, fabricated, and revoked IDs.
- Effective access resolution across channel, project, and app scope, and every provider support combination with its reason code.
- A run whose folder, approval, or scratch use exceeds its resolved level, and Talk only work that would disclose a folder, file name, or path.
- Work by an agent that is not in the channel, and implicit continuation after removal.
- Narrowing at each scope during an active run, including a project or app narrowing reaching inheriting channels and push and remembered approvals lapsing.
- Concurrent access edits converging on the narrower level.
- Targetless planning followed by denied native work.
- Managed and external folder identity replacement.
- Traversal, symlink, absolute-path, and Git common-directory escape.
- Provider attempts to exceed application authority.
- References, summaries, search, attachments, and exports across audience boundaries.
- Scratch replacement, promotion, and failed cleanup.
- Replayed host-tool credentials and calls after run termination.

Tests assert both denial and absence of leaked metadata. A safe error must not reveal that a restricted resource exists.
