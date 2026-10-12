# System map

This document defines the ownership boundaries between Ganbaru AI systems. It is intentionally shorter than the feature specifications. Its purpose is to prevent integrations from creating competing sources of truth.

## Canonical owners

| Concern | Canonical owner | Other systems may do |
| --- | --- | --- |
| Time, availability, reminders, and reservations | Calendar | Propose events, link records, and read authorized capacity |
| Focus rhythm and completed focus history | Pomodoro | Start from an eligible Calendar block and publish bounded progress |
| Tasks, dependencies, assignment, review, and requirement history | Projects | Accept reviewed proposals and link execution or schedule records |
| Pages, blocks, databases, comments, and durable research | Notes | Export derivative context and link other application records |
| Lightweight capture cards | Quick notes | Promote or copy content through an explicit user action |
| Communication, mentions, replies, and decision provenance | Chat | Propose typed changes to Projects, Notes, and Calendar |
| Provider turns, tools, terminals, artifacts, and checkpoints | Execution sessions | Report normalized activity beneath an authorized Chat work thread |
| Browser and application blocking rules and evidence | Distraction blocker | React to Pomodoro and environment state without owning either |
| Playlists, media library, playback state, and media controls | Music | React to Calendar and Pomodoro context |
| Human workspace setup | Work environments (planned) | Activate configured apps, tabs, Music, and anti-distraction policy |
| Identity presentation | Profile and participant records | Resolve names and avatars without rewriting historical content |

## Important transitions

### Calendar and Pomodoro

A Calendar event can carry Pomodoro configuration. Starting focus creates Pomodoro history; it does not mutate past Calendar data. Editing or deleting a protected active event follows the Calendar and Pomodoro continuity rules rather than erasing recorded work.

### Projects and Calendar

Projects can store target dates, dependencies, estimates, and hard deadlines. Calendar stores actual reservations and availability. Rescheduling may be proposed from Projects, but only an accepted Calendar operation changes scheduled time.

### Projects, Notes, and Chat

Projects stores committed work. Notes stores specifications and research. Chat stores discussion and provenance. A message or plan becomes a task, note update, or schedule change only through a typed, reviewable transition.

### Chat and execution sessions

Channels and reply threads outlive provider sessions. A coding-agent run has one authorized execution target and a frozen authorization revision. Provider identity, model, and continuation state are execution metadata, not organizational participant identity.

### Pomodoro, the distraction blocker, and Music

Pomodoro phase changes can activate or relax configured distraction rules and can pause, resume, or switch Music according to user settings. Neither subsystem may infer broader authority from the timer alone, particularly on Android where operating-system access requires separate consent.

### Calendar and work environments (planned)

An eligible session block can activate a human work environment. That action may open applications, prepare browser tabs, choose Music, and apply anti-distraction policy. It never grants an AI agent folder or shell access.

### Diary, sleep, and future AI (planned)

The future sleep flow can open morning or evening diary prompts. Diary data remains private user content. Any later AI analysis is opt-in, permission-bounded, and replaceable without changing diary ownership.

### Sync and collaboration

Local device linking moves a complete vault between one person's devices with a single writer at a time. Planned concurrent synchronization distributes authorized canonical data; neither replaces local storage with a hosted source of truth. Search, reports, mentions, exports, notifications, and AI context must enforce the same resource visibility as direct reads.

## Integration rule

When adding an integration, identify the canonical owner, the initiating record, the proposed transition, the authority check, the audit record, and the failure behavior. If two systems would both become authoritative for the same fact, redesign the boundary before implementation.
