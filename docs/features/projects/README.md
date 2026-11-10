# Projects

Status: Partial. Local planning, task views, scheduling, settings, dependencies, and history are implemented; assignments, guided planning, and reports are planned.

Projects turns intentions into durable tasks, schedules, decisions, and reviewable work. It provides a calm local planning surface without requiring AI and supplies the organizational context used by Calendar, Notes, Chat, and Music.

## Hierarchy

```text
Group
  Project
    Section
      Task
        Subtask
        Checklist item
```

Groups organize projects, and a project may also stand outside every group. Deleting a group keeps its projects with everything they own and moves them outside every group. Deleting a project from its sidebar menu, after a confirmation, removes its tasks, structure, views, and Chat history, while its Notes pages and Calendar events stay without a project and its managed folder stays on disk. Projects own settings, statuses, priorities, tags, custom fields, views, history, and optional working folders. Sections organize tasks without becoming task parents. Tasks can nest and can link to scheduled Calendar events.

## Current scope

| Capability | Status |
| --- | --- |
| Groups, projects, sections, tasks, subtasks, checklists, archive, and restore | Implemented |
| Dashboard, List, Kanban, Calendar, and Gantt views | Implemented |
| Project views that can be added, renamed, reordered, and removed, with autosaved filters per view | Implemented |
| Embedded web page views | Planned |
| Filters, sorting, grouping, selection, bulk actions, and custom columns | Implemented |
| Project defaults, statuses, priorities, tags, custom fields, and icons | Implemented |
| User-defined project templates | Planned |
| Task scheduling and task-event links | Implemented |
| Focus and break playlist defaults | Implemented |
| Task dependencies, milestones, dependency date proposals, and history | Implemented |
| Explicit task date locks and cascade protection overrides | Planned |
| Task assignees, reviewers, and task-linked Chat work | Planned; assignees and reviewers are chosen from project members with the shared participant picker, and only the local person exists today |
| Project and group members with roles | Planned; see [Contacts and invitations](../collaboration/README.md) |
| Guided planning and generated reports | Planned |
| Work-environment and anti-distraction defaults | Planned |

## Routine group

The built-in Routine group holds a fixed set of life-maintenance projects (such as Learning, Reading, Exercise, Chores, and Sleep). The group cannot be deleted or renamed, and built-in Routine projects cannot be deleted and keep their names, group, and order. A fresh vault receives them once; a missing built-in project is not recreated when the workspace loads, so a deletion made on another device stays deleted. Otherwise they are ordinary projects for tasks, views, scheduling, Notes, and Music defaults; their protected identity grants no broader access or special task semantics.

Rationale: Calendar, Pomodoro, and Music need stable everyday categories that exist in every vault without setup.

## Project names

A project name is unique within its group, or among the projects outside every group, ignoring case and surrounding whitespace; the same name may appear in different groups. Creating, renaming, or moving a project where the name is taken is refused, and the form says so. Deleting a group can leave two projects outside every group with the same name; both are kept. Two linked devices that create the same name concurrently keep both projects.

## Source of truth

Projects, tasks, schemas, views, links, and history are SQLite-canonical. Working-folder files and Git repositories remain file-authoritative and can be linked to a project without becoming project rows. Project Notes are canonical Notes pages with project membership.

Every linked device edits Projects offline, including Android, links or unlinks tasks and Calendar events, deletes groups, and removes external working folders; see [Device linking and synchronization](../../data/sync.md).

When linked devices edit the same project name, task title or description, or checklist item title, the Projects view that shows it offers the concurrent versions to choose from. A dependency that two devices added and that together closed a cycle is hidden and offered the same way, labelled as removed to break a cycle: keeping it removed is always available, and adding it back is available only while it would not recreate the cycle. Deleted projects, sections, statuses, and tasks with edits the deletion missed appear in the sync settings recovery list with a preview and can be restored or discarded.

## Principles

- Planning can remain exploratory until an explicit commitment creates or updates canonical work.
- Archive, not deletion, is the normal way to retire projects and tasks, so history and links survive.
- Automatic scheduling and dependency repair present proposals before mutation.
- Project defaults initialize new work but never silently rewrite active or authored records.
- AI assignments use the same project, access, and review boundaries as human-created work.
- Capacity and progress are planning signals, not measures of personal worth.

## Documentation map

- [Tasks and views](tasks-and-views.md)
- [Settings and scheduling](settings-and-scheduling.md)
- [Guided planning and review](guided-planning.md)
- [Shared collection views](../collections.md)
- [Chat agents and coordination](../chat/agents-and-coordination.md)
- [Calendar](../calendar/README.md)
- [Notes](../notes/README.md)
