# Channel-centered agent configuration

Status: Planned. This document is the intended end state that replaces user-made agents and per-agent channel access. Until it ships, [Agents and coordination](agents-and-coordination.md) and [Chat access control](../../data/access-control.md) describe current behavior.

## Why

Agent setup today is agent-centered: each agent carries a name, role, instructions, model, and approval mode, and Settings > Agents edits its access channel by channel through about a dozen controls (presets, history switches, a history range, four folder toggles, a default target, and approval overrides at three levels). People cannot predict what an agent will do in a channel from any one screen.

Ganbaru Chat follows Claude Tag, which configures the place, not the agent: one Claude, and each channel decides how it participates, what instructions it reads, which model it starts on, and which repositories it may use. Ganbaru keeps that model and adapts it to two local facts: several providers can be installed, and work runs on the person's own machine instead of a throwaway cloud sandbox.

## Principles

- The channel is the thing you configure. What an agent can do in a channel is visible on that channel, and every agent in it works under the same rules.
- Settings inherit downward: app, then project, then channel. A lower level overrides a value; it never needs to repeat it.
- An agent is a provider, not a persona. There are no user-made names, roles, or per-agent instructions.
- No control that cannot be explained in one sentence, and no control with a single option.
- No silent downgrades. When a provider cannot honor a channel's setting, the channel says what that agent will do instead.

## Agents

An agent is created the first time any device detects a supported provider (Claude, Codex, Cursor, Grok, OpenCode); nothing is seeded, so a vault with no provider has no agents. Its name, handle, and avatar come from the provider (`@Claude`, `@Codex`), and nothing renames them. The agent record is part of the vault and syncs; its ID is derived from the provider kind, so two devices that detect the same provider converge on one agent instead of creating duplicates.

A second account of the same provider, or a generic ACP agent, is added by hand in Settings > Agents with a short label. Its ID is random, and the label becomes its name and handle (`Claude (work)`, `@claude-work`), so mentions are never ambiguous. The label syncs; each device binds the agent to one of its own provider installations in Settings > Agents, and until it does, the agent counts as not installed there.

Settings > Agents becomes provider setup: for each agent, whether its provider is installed and signed in on this device, its default model and effort, and a test. The default model and effort sync with the agent; installation, executables, and credentials stay device-local, as today. When a device's provider does not offer the chosen model, the run uses the provider's default and the thread shows which model ran.

On a device where a provider is not installed, its agent still appears in channels and history, can still be mentioned, and its work waits in the thread for Start here on a device that has it. Because no device creates a provider agent, the agent has no home device: work runs on the desktop where the mention was posted when the provider is ready there, and otherwise waits for Start here. Android never executes agent work; it shows the agents, their work, and reviews.

## Configuration scopes

| Scope | Where it is edited | What it sets |
| --- | --- | --- |
| App | Settings > Chat | Instructions for every channel, and the default access |
| Project | The project's Chat settings panel | Project instructions, the project's folders, and the access its channels inherit |
| Channel | The channel's details panel, under Agents | Channel instructions, the agents in the channel, the access, and model overrides |

Instructions are concatenated in that order (app, project, channel), so a channel adds to what is above it instead of replacing it. Instructions are guidance for the agent, not limits: what an agent may touch is decided only by access. Channel notes stay what they are today: facts the channel keeps, read by every agent there, which a person saves and an agent may only suggest. They play the role of Claude Tag's channel memory.

Work reads its instructions and model when it starts, so editing them changes new work and leaves running threads alone. Access is different: lowering it applies to running work at once, and raising it applies to work that starts afterward.

The channel's Agents section shows the resolved values, each with where it comes from ("From project"), and a reset to inherit. The channel dialog keeps name, topic, and members; agents are added and removed there or in the details panel, and both open the same controls.

## Agents in a channel

An agent works in a channel only after it is added, as in Slack. Adding needs no follow-up choices: the agent gets the channel's access immediately. An agent in a channel reads the channel the way a person who joins it does, within the bounded context package; there is no separate history switch or history range.

An agent acts only when mentioned, and a reply in its assignment thread continues its work without a new mention. Automatic replies to messages that do not mention it are a later option; the setting appears only when that second mode exists.

Mentioning an agent that is not in the channel is offered in the mention menu as "Add @Codex to #general", which adds it and keeps the mention in the draft. Nothing is added by a message alone.

## Access

Access is one setting per channel, inherited from the project and the app. It applies to every agent in the channel and to every folder of the project.

| Option | What agents can do |
| --- | --- |
| Talk only | Agents reply from the channel alone, with no folder, file name, or path from disk. |
| Read only | Agents read the project's folders but cannot change them. |
| Ask before commands (default) | Agents edit files and ask you before running each command. |
| Work on their own | Agents edit files and run commands without asking, confined to the project's folders. |

Work on their own is offered only on a device that can confine commands, either through the provider's own sandbox or through Ganbaru's sandbox; elsewhere it is shown disabled with the reason.

Pushing to a Git remote is never part of a level. Agents under Ask before commands or Work on their own may request a push, and every push, Work on their own included, waits on a confirmation card naming the branch, remote, and commit, checked as in [Chat security](../../data/security/chat.md). Push and don't ask again remembers that agent, folder, push URL, and destination branch; the channel's Agents section lists these approvals, and lowering the channel's access below Ask before commands, removing the agent from the channel, or revoking the approval there ends it.

The channel lists each agent with one line describing what it will actually do under the current access on this device, for example "Asks before every command" or "Read only here: Cursor cannot ask before commands". When a provider cannot honor the level, the agent works at the closest narrower level it supports and that line says so; it never widens and never hides the difference.

A channel uses every folder of its project. A run uses the folder its thread already works in, else a folder the request names, else the project's primary folder; under Talk only it has no folder. The reply names the folder when the project has more than one. Private scratch stays a target a request can name, unchanged.

## Model and effort

Each agent's default model and effort are set in Settings > Agents. A channel may override the model for an agent in its Agents section; the row shows "Default (model)" until it does. A thread switches its own model with `/model` in the message that starts the work, and work that continues in that thread keeps it. This command replaces the composer's model and effort controls, so a post never carries hidden model choices. Every agent reply names the model that produced it, so a switch, a channel override, or a device falling back to the provider default is visible where it happened.

## Common journeys

- **First use:** the person installs Claude Code. Settings > Agents shows Claude as ready, `@Claude` appears in the mention menu with an Add option in every channel, and adding it to `#general` lets it edit the project folder and ask before commands.
- **A brainstorming channel:** the person sets the channel's access to Talk only. Every agent in it replies without touching files.
- **A trusted project on Linux:** the person sets the project's access to Work on their own. Every channel of the project inherits it unless it overrides.
- **A second device:** the laptop shows the same agents and channel settings. Work mentioned there runs on the laptop when the provider is installed there; otherwise it waits for Start here on a device that has it.

## Data consequences

The membership keeps only presence. Read history, take assignments, the history boundary, per-folder grants, the default target, and the runtime approval overrides at channel, folder, and scratch level are removed. Access, instructions, and model overrides become values on the app settings, the project, and the channel, synced with the rest of Chat. Agent identity rows lose name, role, instructions, and avatar, which come from the provider. There are no users yet, so this is a schema replacement with a development vault reset, not a migration.

## Open questions

- Should every agent be in every channel of a project by default, removing the Add step? Recommendation: no, keep explicit adding, because it lets a channel stay human-only and matches Slack.
- Should a channel be able to use fewer folders than its project? Recommendation: not until human collaborators ship, since one person's channels rarely need different folders.
- How does delegation pick a helper without roles? Recommendation: the delegating agent names the agent and the task, and the helper works under the same channel access.
- How do planned human collaborators combine with channel access? Recommendation: a collaborator's agent works under the channel's access capped by that collaborator's role.
