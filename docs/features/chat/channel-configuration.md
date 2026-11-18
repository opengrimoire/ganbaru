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

An agent exists for each supported provider: Claude, Codex, Cursor, Grok, OpenCode, and a generic ACP agent. Its name, handle, and avatar come from the provider (`@Claude`, `@Codex`), and nothing renames them. The agent record is part of the vault and syncs; its ID is derived from the provider kind, so two devices that detect the same provider converge on one agent instead of creating duplicates.

A person who uses two accounts of the same provider adds a second instance in Settings > Agents and gives it a short label. The label becomes part of the name and handle (`Claude (work)`, `@claude-work`), so mentions are never ambiguous.

Settings > Agents becomes provider setup: for each provider, whether it is installed and signed in on this device, its default model and effort, and a test. Installation, executables, and credentials stay device-local, as today.

On a device where a provider is not installed, its agent still appears in channels and history, can still be mentioned, and its work waits in the thread for Start here on a device that has it. Because no device creates a provider agent, the agent has no home device: work runs on the desktop where the mention was posted when the provider is ready there, and otherwise waits for Start here. Android never executes agent work; it shows the agents, their work, and reviews.

## Configuration scopes

| Scope | Where it is edited | What it sets |
| --- | --- | --- |
| App | Settings > Chat | Instructions for every channel, and the default access |
| Project | The project's Chat settings panel | Project instructions, the project's folders, and the access its channels inherit |
| Channel | The channel's details panel, under Agents | Channel instructions, the agents in the channel, the access, and model overrides |

Instructions are concatenated in that order (app, project, channel), so a channel adds to what is above it instead of replacing it. Channel notes stay what they are today: facts the channel keeps, read by every agent there, which a person saves and an agent may only suggest. They play the role of Claude Tag's channel memory.

The channel's Agents section shows the resolved values, each with where it comes from ("From project"), and a reset to inherit. The channel dialog keeps name, topic, and members; agents are added and removed there or in the details panel, and both open the same controls.

## Agents in a channel

An agent works in a channel only after it is added, as in Slack. Adding needs no follow-up choices: the agent gets the channel's access immediately. An agent in a channel reads the channel the way a person who joins it does, within the bounded context package; there is no separate history switch or history range.

An agent acts only when mentioned, and a reply in its assignment thread continues its work without a new mention. Automatic replies to messages that do not mention it are a later option; the setting appears only when that second mode exists.

Mentioning an agent that is not in the channel is offered in the mention menu as "Add @Codex to #general", which adds it and keeps the mention in the draft. Nothing is added by a message alone.

## Access

Access is one setting per channel, inherited from the project and the app. It applies to every agent in the channel and to every folder of the project.

| Option | What agents can do |
| --- | --- |
| Talk only | Agents reply in the channel without opening any folder. |
| Read only | Agents read the project's folders but cannot change them. |
| Ask before commands (default) | Agents edit files and ask you before running each command. |
| Work on their own | Agents edit files and run commands without asking, confined to the project's folders. |

Work on their own is offered only on a device that can confine commands, either through the provider's own sandbox or through Ganbaru's sandbox; elsewhere it is shown disabled with the reason.

Pushing to a Git remote is never part of a level. Every push asks on a confirmation card naming the branch, remote, and commit, and Push and don't ask again remembers that folder, remote, and branch for that agent, as today.

The channel lists each agent with one line describing what it will actually do under the current access on this device, for example "Asks before every command" or "Talk only here: Cursor cannot ask before commands". When a provider cannot honor the level, the agent works at the closest narrower level and that line says so; it never widens and never hides the difference.

A channel uses every folder of its project. The folder a run uses follows [Execution targets](../../data/access-control.md#execution-targets): the project's primary folder unless the request names another one, and the reply names the folder when the project has more than one.

## Model and effort

Each agent's default model and effort are set in Settings > Agents. A channel may override the model for an agent in its Agents section; the row shows "Default (model)" until it does. A thread switches its own model with `/model` in the message that starts the work, and work that continues in that thread keeps it. The composer has no model or effort controls beyond that command.

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
- Should Talk only also hide the project's file names from the context package? Recommendation: yes, Talk only shares the channel and nothing from disk.
