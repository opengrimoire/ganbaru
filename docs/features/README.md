# Feature documentation

Feature documents describe intended product behavior and the reasons behind it. They are normative unless a section is explicitly labeled as current implementation status. Status labels (Implemented, Partial, Planned, Deferred) follow the [documentation status vocabulary](../README.md#status-language).

Source paths, schema inventories, cache tuning, device test matrices, and delivery history belong in architecture, data, performance, testing, or roadmap documentation. Feature specs keep security bounds and limits when those bounds affect user expectations or protect durable data.

## Current and active domains

| Domain | Status | Documentation |
| --- | --- | --- |
| Calendar | Partial | [Calendar](calendar/README.md) |
| Pomodoro | Partial | [Pomodoro](pomodoro/README.md) |
| Projects | Partial | [Projects](projects/README.md) |
| Notes | Partial | [Notes](notes/README.md) |
| Chat | Partial | [Chat](chat/README.md) |
| AI integrations | Partial | [AI integrations](ai/README.md) |
| Distraction blocker | Partial | [Distraction blocker](distractions/README.md) |
| Music | Partial | [Music](music/README.md) |
| Themes | Implemented | [Themes](themes/README.md) |
| Localization | Partial | [Localization](localization.md) |
| Workspace tabs | Partial | [Workspace tabs](workspace-tabs.md) |
| Quick notes | Implemented | [Quick notes](quick-notes.md) |
| Profile | Implemented | [Profile](profile.md) |
| Contacts and invitations | Partial | [Contacts and invitations](collaboration/README.md) |

Linked devices share a vault: each starts from a snapshot copy of the linked vault and then writes every domain. Quick notes, portable preferences with the profile picture, user themes, contacts, the Music library, Projects with working folder bindings, Calendar, distraction rules, daily distraction usage totals, distraction block history, Focus history with device activity, and Chat organizational content sync concurrently, editable on every linked device, and Android also exchanges in the background. Notes replicates in full (structure, databases, comments and suggestions, and assets), editable on every linked device; text merging is planned. Linked devices follow coordinator address changes, find the coordinator on the LAN, connect over private and overlay networks such as Tailscale, and exchange directly with each other on the LAN while the coordinator is off. An anchor desktop orders device enrollment and removal, with admin roles, a recovery kit, anchor handover, quarantine, and late changes from removed devices. This is implemented in source, with physical multi-device acceptance pending. The encrypted relay remains planned. See [Device linking and synchronization](../data/sync.md).

## Planned product surfaces

- [Diary](diary.md)
- [Edge panel](edge-panel.md)
- [Sleep alarm](sleep-alarm.md)
- [Work environments](work-environments.md)

## Deferred product surfaces

- [Gamification](gamification.md)

Platform-specific behavior lives under [platform documentation](../platforms/README.md). Data ownership and persistence rules live under [data documentation](../data/README.md). Pure logic that can be specified independently of UI or persistence lives under [algorithms](../algorithms/README.md).
