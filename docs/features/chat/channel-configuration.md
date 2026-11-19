# Channel-centered agent configuration

Status: Implemented. A second account of the same provider and generic ACP agents are deferred. [Agents and coordination](agents-and-coordination.md) covers assignments and review, and [Chat access control](../../data/access-control.md) is the normative authorization rule.

## Why

When each agent carries its own name, role, instructions, model, and approval mode, and its access is edited channel by channel through history switches, folder toggles, default targets, and approval overrides, people cannot predict what an agent will do in a channel from any one screen.

Ganbaru Chat follows Claude Tag, which configures the place, not the agent: one Claude, and each channel decides how it participates, what instructions it reads, and which repositories it may use. Ganbaru keeps that model and adapts it to two local facts: several providers can be installed, and work runs on the person's own machine instead of a throwaway cloud sandbox. Agents are a deliberate exception for the model only: tagging `@Opus` or `@Luna 6` picks a model as directly as naming it, while access and instructions still come from the channel.

## Principles

- The channel is the thing you configure. What an agent can do in a channel is visible on that channel, and every agent in it works under the same rules.
- Settings inherit downward: app, then project, then channel. A lower level overrides a value; it never needs to repeat it.
- An agent is a model line, a model, or a named model, not a persona. Agents carry no roles or per-agent instructions; behavior comes from the channel.
- No control that cannot be explained in one sentence, and no control with a single option.
- No silent downgrades. When a provider cannot honor a channel's setting, the channel says what that agent will do instead.

## Agents

Agents are created the first time any device detects a supported provider (Claude, Codex, Cursor, Grok, OpenCode); nothing is seeded, so a vault with no provider has no agents, and a provider has no agent of its own, since people tag a model, not a company. Agent records are part of the vault and sync.

Detection creates one agent per model line the provider offers (`@Fable`, `@Opus`, `@Sonnet`, `@Haiku` for Claude; `@Astra`, `@Sol`, `@Terra`, `@Luna` for Codex), with IDs derived from the provider and the line. `@Mythos`, which only some Claude accounts can run, is created once a device's catalog offers it and is listed above `@Fable`. A line agent always runs the newest model of its line on the executing device, so it never needs updating when a model ships. It also creates one agent per catalog model, named after the model (`@Opus 5.5`, `@Luna 6`) with the effort `medium`, else the model's highest, and an ID derived from the provider and the model; model agents start off, so only line agents are on until a person turns more on. A person can also add a named agent from the Custom agent dialog in Settings > Agents, with a one-word name, unique ignoring case and never a model line, plus droplists for a model of that provider (each model line first, which always runs its newest model, then the catalog in the Settings > Agents order), its reasoning, and its speed, where the reasoning starts on `medium`, else the highest. Its ID is random and its name syncs. Every agent keeps its own model, so a channel cannot override it; only `/model` in a message changes it for that work. Settings > Agents lists every agent of a provider in one list with its `@` name, line agents first, then named agents, then model agents, and a switch turns each on or off for channels and mentions by archiving or restoring it; removing a named agent deletes it when it was never used and otherwise archives it and hides it from the list until a person restores it (see [Agents and coordination](agents-and-coordination.md)), and line agents carry a "Latest" mark. Model names put the version after the line and drop a leading "GPT", so `GPT-6-Luna` reads `Luna 6`, like the agent people tag.

Deferred: a second account of the same provider, or a generic ACP agent, will be added by hand in Settings > Agents with a short label. Its ID will be random, and the label will become its name and handle (`Claude (work)`, `@claude-work`), so mentions are never ambiguous. The label will sync; each device will bind the agent to one of its own provider installations, and until it does, the agent will count as not installed there.

Settings > Agents is provider setup: for each provider, whether it is installed and signed in on this device, its agents, and a test. Agents and their models sync; installation, executables, and credentials stay device-local, as today. When a device's provider does not offer the chosen model, the run uses that provider's recommended model and the thread shows which model ran.

On a device where a provider is not installed, its agent still appears in channels and history, can still be mentioned, and its work waits in the thread for Start here on a device that has it. An agent has no home device: work runs on the desktop where the mention was posted when the provider is ready there, and otherwise waits for Start here. Android never executes agent work; it shows the agents, their work, and reviews.

## Configuration scopes

| Scope | Where it is edited | What it sets |
| --- | --- | --- |
| App | Settings > Chat | Instructions for every channel, and the default access |
| Project | The project's Chat settings panel | Project instructions, the project's folders, and the access its channels inherit |
| Channel | The channel's details panel, under Agents | Channel instructions, the agents in the channel, and the access |

Instructions are concatenated in that order (app, project, channel), so a channel adds to what is above it instead of replacing it. Instructions are guidance for the agent, not limits: what an agent may touch is decided only by access. Channel notes stay what they are today: facts the channel keeps, read by every agent there, which a person saves and an agent may only suggest. They play the role of Claude Tag's channel memory.

Work reads its instructions and model when it starts, so editing them changes new work and leaves running threads alone. Access is different: lowering it applies to running work at once, and raising it applies to work that starts afterward.

The channel's Agents section shows the resolved values, each with where it comes from ("From project"), and a reset to inherit. The channel dialog keeps name, topic, and members; agents are added and removed there or in the details panel, and both open the same controls.

## Agents in a channel

An agent works in a channel only after it is added, as in Slack. Adding needs no follow-up choices: the agent gets the channel's access immediately. An agent in a channel reads the channel the way a person who joins it does, within the bounded context package; there is no separate history switch or history range.

An agent acts only when mentioned, and a reply in its assignment thread continues its work without a new mention. Automatic replies to messages that do not mention it are a later option; the setting appears only when that second mode exists.

The mention menu offers only the channel's current members, so an agent joins a channel from the channel's member settings, never from the composer or a message alone.

## Access

Access is one setting per channel, inherited from the project and the app. It applies to every agent in the channel and to every folder of the project.

| Option | What agents can do |
| --- | --- |
| Talk only | Agents reply from the channel alone, with no folder, file name, or path from disk. |
| Read only | Agents read the project's folders but cannot change them. |
| Ask before commands (default) | Agents edit files and ask you before running each command. |
| Work on their own | Agents edit files and run commands without asking, confined to the project's folders. |

Editing files inside the project's folders never asks under Ask before commands or Work on their own, but an edit to a hidden path such as `.git` or a provider's settings folder goes to a person, or is refused where the provider's sandbox keeps it read-only.

Work on their own is offered only on a device that can confine commands, either through the provider's own sandbox or through Ganbaru's sandbox; elsewhere it is shown disabled with the reason.

Pushing to a Git remote is never part of a level. Agents under Ask before commands or Work on their own may request a push, and every push needs a person's decision, Work on their own included: the confirmation card names the branch, remote, and commit, checked as in [Chat security](../../data/security/chat.md). Push and don't ask again records that decision for that channel, agent, folder, push URL, and destination branch, so later matching requests push when their turn ends; the channel's Agents section lists these approvals, and lowering the channel's effective access below Ask before commands, removing the agent from the channel, or revoking the approval there ends it.

Private scratch follows the same level: Talk only and Read only offer no scratch, Ask before commands asks before each command there, and Work on their own runs commands there without asking under the same confinement rule. Promoting scratch files into a folder needs a level that edits files.

The channel lists each agent with one line describing what it will actually do under the current access on this device, for example "Asks before every command" or "Read only here: Cursor cannot ask before commands". When a provider cannot honor the level, the agent works at the closest narrower level it supports and that line says so; it never widens and never hides the difference. A provider that cannot ask before commands but can keep edits inside the folder works under Ask before commands by editing files without running commands.

A channel uses every folder of its project. A run uses a target chosen explicitly when the work starts, else the folder its thread already works in, else a folder the message names, else the project's primary folder; under Talk only it has no folder. The reply names the folder when the project has more than one. Private scratch stays a target a request can name.

## Model and effort

Each agent runs its own model: a line agent the newest model of its line, any other agent the model and effort it was made with, and the channel's Agents section shows which. There is no default model and no channel model override. A thread switches its own model with `/model` in a message that starts or continues the work, and later work in that thread keeps the model it last chose. The channel composer has no other model control, so a post never carries hidden model choices. Every agent reply names the model that produced it, so a switch or a device falling back to the recommended model is visible where it happened.

## Common journeys

- **First use:** the person installs Claude Code. Settings > Agents shows Claude as ready with `@Fable`, `@Opus`, and each Claude model, adding `@Opus` to `#general` from the channel's member settings puts it in that channel's mention menu and lets it edit the project folder and ask before commands.
- **A brainstorming channel:** the person sets the channel's access to Talk only. Every agent in it replies without touching files.
- **A trusted project on Linux:** the person sets the project's access to Work on their own. Every channel of the project inherits it unless it overrides.
- **A second device:** the laptop shows the same agents and channel settings. Work mentioned there runs on the laptop when the provider is installed there; otherwise it waits for Start here on a device that has it.

## Data consequences

An agent channel membership holds only presence. Access and instructions are values on the app settings, the project, and the channel, synced with the rest of Chat; concurrent access edits keep the narrower level. An agent record holds its provider family and model; a line or model agent takes its name from the line or model, and a named agent keeps the name a person gave it. Each assignment freezes its model and composed instructions when it is created, and records the model that ran. [Chat schema](../../data/schema/chat.md) owns the tables.

## Decisions

- Agents are added to a channel explicitly instead of joining every channel of a project, so a channel can stay human-only, as in Slack.
- A channel uses every folder of its project; narrower folder sets wait for human collaborators, since one person's channels rarely need different folders.
- Delegation names the helper agent and the task, and the helper works under the same channel access, capped by the delegating work.

## Open questions

- How do planned human collaborators combine with channel access? Recommendation: a collaborator's agent works under the channel's access capped by that collaborator's role.
