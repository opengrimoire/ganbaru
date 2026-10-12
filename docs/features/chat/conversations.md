# Chat conversations

## Organizational vocabulary

- **Group:** the top-level local organizational container.
- **Project:** a durable work area with channels and optional authorized working folders.
- **Channel:** a durable project conversation, beginning with `#general`.
- **Reply thread:** focused discussion attached to one message; AI work assignments run inside reply threads.
- **Provider run:** one bounded execution session linked into a conversation.
- **Direct message** (planned): a durable conversation with stable participants.
- **Task discussion** (planned): a conversation attached to one project task.

Provider thread IDs and model names are never conversation or participant identities.

## Navigation and lifecycle

The sidebar shows project channels, personal sections, direct messages, pending invitations, and search, following the [shared Notes and Chat sidebar design](../notes/pages-and-navigation.md#shared-notes-and-chat-sidebar-design). Channel ordering and membership are durable vault data. Personal sections and the last selected channel are device-local presentation state. Direct messages and invitations follow [Contacts and invitations](../collaboration/README.md).

One channel dialog, opened from the sidebar, edits the name, topic, personal section, and members together. The local person is always the first member and cannot be removed there; leaving a channel is an action on the channel itself, and the last person cannot leave. Members are grouped as People and Agents, added through the shared participant picker, and changes apply on save. Adding an agent from the channel grants the conversation-only default, and its detailed access is edited in Settings > Chat > Agents. Removing an agent that has active work asks for confirmation with the impact.

The channel header shows the members as an avatar stack with a count. It opens a details panel with the topic and the member list, where members can be added or removed by someone who may manage members and the channel dialog can be opened. A channel never shows an in-feed prompt to add agents.

Archive hides a conversation from normal navigation without destroying its messages, links, drafts, or review history. Restore returns it to its project. Permanent deletion is explicit and includes a cleanup plan for owned execution resources.

Search uses bounded, paginated reads over active or archived conversations according to the selected filter. Opening a result restores the conversation and position without loading its complete history.

## Messages and replies

Messages are durable participant-authored records. They can include text, validated attachments, references to tasks, projects, folders, and files, replies, mentions, and provenance for generated or transformed content.

Reply threads keep detailed work from overwhelming the main channel. A provider run started from a reply posts concise state and results to the main timeline; detailed execution stays inspectable in the linked run.

## Composer

The composer supports drafts, attachments, replies, explicit agent mentions, provider interactions, and scheduled sends. A message can be sent without invoking AI, and plain conversation never silently becomes an execution request.

Slash commands are discoverable actions, not an alternate security model. They can change presentation, select an interaction mode, or start a typed application action, but cannot bypass project, folder, provider, or confirmation boundaries.

When a message looks actionable but lacks an authorized target or a clear objective, Chat keeps it as conversation or asks for clarification. It never guesses a working folder or broadens authority.

## Timeline

The timeline combines organizational messages with normalized run events in canonical order, with stable anchors and enough provenance to show who requested work, which provider executed it, what authority applied, and what it produced.

Routine low-level provider events can fold after settlement. Errors, interactive requests, decisions, file changes, checkpoints, and final results stay easy to find. Folding is presentation only and never deletes history.

## Attachments

Images and bounded text context are copied into managed vault storage. The attachment record keeps original display metadata and its owning conversation or message; external absolute paths are never portable attachment identity.

Unsupported, oversized, missing, or unsafe files fail explicitly. Previewing an attachment does not grant the provider access. A run receives an attachment only when its typed request and authorization include it.

## Responsive behavior

Desktop and mobile use the same conversation model. On narrow screens, channel navigation replaces the conversation temporarily instead of covering it with an ambiguous overlay. Android Back unwinds navigation, reply, request, and composer layers before leaving Chat.
