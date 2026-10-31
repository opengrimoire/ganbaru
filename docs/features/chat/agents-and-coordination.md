# Chat agents and coordination

Status: Partial. AI agents, channel access profiles, mention-triggered assignments, frozen context packages, and assignment review states are implemented. Structured delegation, budgets, quiet periods, task-linked assignments, and human collaborators are planned.

Ganbaru coordinates work through durable conversations, explicit assignments, and review. AI providers are execution mechanisms beneath that model, not organizational identities. There is no privileged manager agent or general vault agent, and no planning agent is seeded by default.

## Participants and agents

A participant is the local person, a persistent AI agent, or (planned) a human collaborator. An agent has a stable identity, role, and instructions independent of provider and model. Changing its provider or model creates a new policy revision, not a new participant, and its history stays intact. An agent is archived and restored rather than deleted, because its identity is shared with other devices of the vault.

An agent's policy selects the provider instance, model and options, effort, and approval mode. Its configuration state reports when it needs setup, its provider is unavailable, or it lacks folder access.

Profiles never grant authority by themselves. An agent acts in a channel only through a channel membership that carries:

- An access profile, either a built-in recipe (Conversation only, Read only, Edit files, Build and test, Publish changes) or a custom one.
- Separate capabilities to read channel history and to participate.
- A history boundary: the entire channel or only messages from the grant onward.
- Working-folder grants, each ordered as none, read, edit, execute, or publish.

Effective authority is the intersection of these ceilings, resolved by [Chat access control](../../data/access-control.md).

## Invocation

Ordinary messages never invoke every visible agent. Work starts through an explicit mention or, in the future, a typed assignment, a documented workflow, or a user-approved scheduled action. Mentions distinguish reference from invocation: naming an agent in prose is not enough, and an actionable invocation is visible before execution begins.

An invocation creates a work assignment in a reply thread, linked to its triggering message and any previous assignment. Assignments move through queued, working, waiting for an answer, waiting for approval, ready for review, completed, failed, or cancelled. A settled assignment run (completed, failed, or cancelled) ignores later provider events, so a late or replayed event cannot reopen it or change how it ended. A reply in an assignment thread continues with that thread's latest teammate without a new mention, so review feedback reaches the agent that did the work; this requires the teammate to still participate in the channel, and removing it stops implicit continuation. Only the local owner posts today; when other people can post, a reply continues an assignment only within its author's own authority (planned).

Assignments sync between linked devices, and each one runs on a single device: the teammate's home device (the device that created it) while that device can still write Chat, or the desktop where the mention was posted. When the executing device stops writing Chat, as after an unlink or a revoked writer, a device that can write Chat takes over its active work at startup: work that had not started moves over, and started work fails with a reason so Retry can run it again. Start takes over a mention only while its work is still active; finished work is never rerun. When the mention arrives on another device first, that device shows it without starting work; a desktop can start it there instead (the Start action in the timeline is planned). Any device can review, complete, or cancel an assignment, and cancelling it elsewhere stops the run on the device that executes it. Scheduled messages sync too and are sent by the device that scheduled them; other devices can cancel them, and an edit made elsewhere while the message is being sent is lost, since the sent message keeps the sending device's text.

## Resolution before work starts

Rust resolves, and records with the assignment:

- Requesting conversation, participant, and project.
- The agent and its current policy revision.
- Authorized working folder or private scratch generation.
- A bounded context package.
- Effective access and approval policy.
- Provider, model, interaction mode, and capability.
- Planned: task link, time, token, cost, and delegation budgets.

Provider-native trust, a previous run, shell availability, an agent profile, or membership in a broad channel cannot widen any resolved value.

## Context packages

A context package is a bounded selection of information frozen for one assignment, with each source's revision and content hash recorded. It includes the triggering message, its reply thread when the thread root is within the agent's history boundary, permitted channel messages, attachments, and referenced resources. Planned sources include approved Notes, task state, and Calendar context. The package is sent as JSON Lines, one record per source with its kind, ID, and text, so message text cannot pose as another source.

Context assembly follows participant access and the execution target. It never scans the complete vault or attaches unrelated history because it might be useful. The recorded package makes later review possible without treating provider memory as canonical.

## Results and review

Agents report material outcomes as typed updates (plan, replan, question, approval, failure, result, review) rather than narrating every tool call into a shared channel. Detailed execution stays in the run timeline.

Review focuses on decisions and risk: what changed or was proposed, what could not be completed and why, which assumptions or approvals affected the outcome, what needs human judgment, and which checkpoint or files hold the result. Approval of one result does not grant standing approval for later work, and rejected or superseded proposals remain understandable in history.

Proposals that would change Projects or Calendar remain drafts until accepted through the owning feature's typed command. See [Guided planning and review](../projects/guided-planning.md).

## Delegation (planned)

Delegation creates explicit child assignments with their own parent, objective, target, authority subset, budget, expected result, and review path. An agent cannot delegate authority it does not hold. Parallel work stays visible as separate, independently cancellable assignments rather than hidden provider-native subagents, and a failed child never broadens another assignment to compensate.

## Anti-burnout constraints

Coordination should reduce work fragmentation, not create pressure to supervise agents continuously. Defaults favor bounded work, quiet progress, batch review, explicit deadlines, and visible budgets. The system must not manufacture urgency, shame inactivity, optimize for message volume, or treat agent activity as proof of human productivity. Planned: scheduled work respects quiet periods and defers non-urgent notifications.

## Authority and safety

Destructive, externally visible, credential-related, security-sensitive, or broad-scope actions require typed boundaries and appropriate confirmation. Revocation stops future reads and context assembly without rewriting legitimate historical messages.

## Human collaboration (planned)

People join spaces through contacts, invitations, and the roles defined in [Contacts and invitations](../collaboration/README.md), which also fixes how the people and agent flows share one picker, one access step, and one member list. Encryption, key distribution, revocation, historical visibility, offline copies, and conflict resolution belong to the [sync](../../data/sync.md) and permission design.
