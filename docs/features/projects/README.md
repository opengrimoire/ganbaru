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

Groups organize projects. Projects own settings, statuses, priorities, tags, custom fields, views, history, and optional working folders. Sections organize tasks without becoming task parents. Tasks can nest and can link to scheduled Calendar events.

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

The built-in Routine group holds a fixed set of life-maintenance projects (such as Learning, Reading, Exercise, Chores, and Sleep). The group cannot be deleted or renamed, built-in Routine projects keep their names, group, and order. A fresh vault receives them once; a missing built-in project is not recreated when the workspace loads, so a deletion made on another device stays deleted. Otherwise they are ordinary projects for tasks, views, scheduling, Notes, and Music defaults; their protected identity grants no broader access or special task semantics.

Rationale: Calendar, Pomodoro, and Music need stable everyday categories that exist in every vault without setup.

## Source of truth

Projects, tasks, schemas, views, links, and history are SQLite-canonical. Working-folder files and Git repositories remain file-authoritative and can be linked to a project without becoming project rows. Project Notes are canonical Notes pages with project membership.

Every linked device edits Projects offline, including Android. Deleting a group, removing an external working folder, and linking or unlinking a task and a Calendar event stay on the main device, and a linked device that tries them shows the ownership prompt; see [Device linking and synchronization](../../data/sync.md).

When linked devices edit the same project name, task title or description, or checklist item title, the Projects view that shows it offers the concurrent versions to choose from. A dependency that two devices added and that together closed a cycle is hidden and offered the same way, labelled as removed to break a cycle: keeping it removed is always available, and adding it back is available only while it would not recreate the cycle. Deleted groups, projects, sections, statuses, and tasks with edits the deletion missed appear in the sync settings recovery list with a preview and can be restored or discarded.

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
