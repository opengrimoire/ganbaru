# Chat

Status: Partial. Project channels and messaging are implemented on desktop and Android; local coding-agent execution is desktop-only. Direct messages, task discussions, delegation, and human collaboration are planned.

Chat is Ganbaru AI's project communication and coding-work surface. It combines durable human-readable conversations with bounded provider runs, review, files, terminals, and checkpoints without making provider threads the organizational source of truth.

## Current scope

| Capability | Status |
| --- | --- |
| Project `#general`, custom channels, personal sections, navigation, archive, and search | Implemented |
| Messages, reply threads, mentions, drafts, attachments, scheduled sends, and combined timeline | Implemented |
| Codex, Claude, Cursor, Grok, and OpenCode execution | Implemented |
| Interactive requests, cancellation, inspector, file browser and editor, review, terminals, browser previews, and Git checkpoints | Implemented |
| AI agents with per-channel access profiles, folder grants, and frozen context packages | Implemented |
| Private scratch execution with explicit promotion and cleanup | Implemented |
| Structured delegation, budgets, quiet periods, and task-linked assignments | Planned |
| Direct messages and task discussions | Planned; the schema reserves these conversation kinds |
| Human members, invitations, and encrypted sync | Planned; the experience is designed in [Contacts and invitations](../collaboration/README.md) |

## Product boundary

Chat owns communication: projects, channels, reply threads, participants, memberships, messages, mentions, drafts, scheduled sends, work assignments, review, and organizational history.

Provider runtimes own execution details: provider sessions and continuation, tool calls, reasoning events, interactive requests, process lifecycle, workspace edits, terminals, browser previews, and checkpoints.

A channel can link many sequential or parallel provider runs and never depends on one provider's continuation model. Rationale: providers differ and change; the organizational record must outlive any of them.

See [Conversations](conversations.md), [Agents and coordination](agents-and-coordination.md), and [Execution and workspace](execution-and-workspace.md).

## First use

Opening Chat creates or resolves the selected project's durable `#general` channel. Chat is useful without a configured provider for reading, writing, organizing, searching, and scheduling messages.

Provider setup is explicit. Ganbaru AI discovers supported provider families, explains missing or invalid installations, and never claims a provider is ready before its probe succeeds. Discovery never blocks reading local history; see [Provider runtimes](../ai/provider-runtimes.md).

## Data ownership

The active vault owns organizational communication, canonical provider events, projections, drafts, attachment metadata, checkpoints, and authorization records. Managed attachment bytes live under the vault. Device-local state owns external paths, executable discovery, provider homes, native credentials, process state, and presentation preferences that should not travel. Authorization rules live in [Chat access control](../../data/access-control.md).

Archive is reversible. Permanent deletion first records cleanup work for provider sessions, workspaces, terminals, previews, attachments, and credentials that cannot be removed in the same transaction.

## Safety and accessibility

Chat never grants the frontend a generic shell or arbitrary filesystem access. Untrusted provider events and file content are validated and bounded before display. Approval prompts state the requested action and effective authority.

All conversation, navigation, composer, request, file, review, and terminal controls are keyboard reachable. Responsive layouts render the same canonical conversation; there is no divergent mobile copy.
