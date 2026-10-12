# Chat agents and coordination

Status: Partial. AI agents, channel access profiles, mention-triggered assignments, frozen context packages, and assignment review states are implemented. Structured delegation, budgets, quiet periods, task-linked assignments, and human collaborators are planned.

Ganbaru AI coordinates work through durable conversations, explicit assignments, and review. AI providers are execution mechanisms beneath that model, not organizational identities. There is no privileged manager agent or general vault agent, and no planning agent is seeded by default.

## Participants and agents

A participant is the local person, a persistent AI agent, or (planned) a human collaborator. An agent has a stable identity, role, and instructions independent of provider and model. Changing its provider or model creates a new policy revision, not a new participant, and its history stays intact.

An agent's policy selects the provider instance, model and options, effort, and approval mode. Its configuration state reports when it needs setup, its provider is unavailable, or it lacks folder access.

Profiles never grant authority by themselves. An agent acts in a channel only through a channel membership that carries:

- An access profile, either a built-in recipe (Conversation only, Read only, Edit files, Build and test, Publish changes) or a custom one.
- Separate capabilities to read channel history and to participate.
- A history boundary: the entire channel or only messages from the grant onward.
- Working-folder grants, each ordered as none, read, edit, execute, or publish.

Effective authority is the intersection of these ceilings, resolved by [Chat access control](../../data/access-control.md).

## Invocation

Ordinary messages never invoke every visible agent. Work starts through an explicit mention or, in the future, a typed assignment, a documented workflow, or a user-approved scheduled action. Mentions distinguish reference from invocation: naming an agent in prose is not enough, and an actionable invocation is visible before execution begins.

An invocation creates a work assignment in a reply thread, linked to its triggering message and any previous assignment. Assignments move through queued, working, waiting for an answer, waiting for approval, ready for review, completed, failed, or cancelled.

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

A context package is a bounded selection of information frozen for one assignment, with each source's revision and content hash recorded. It includes the triggering message, its reply thread, permitted channel messages, attachments, and referenced resources. Planned sources include approved Notes, task state, and Calendar context.

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
