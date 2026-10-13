# Project tasks and views

## Task model

A task has stable identity, project, section, optional parent task, title, description, status, priority, type, estimate, start and due dates with optional times, target end date, milestone state, blocker reason, dependencies, tags, custom values, archive state, and timestamps. Task assignees and reviewers are planned.

Subtasks are normal tasks with a parent. Checklist items are lighter completion records owned by one task. A parent and its descendants keep explicit states; completing a parent does not silently erase open child work.

The project header's add button opens task detail on an empty draft in the default section and status from any view. The draft offers the same sections as an existing task, but nothing is persisted until it is submitted with a title: the task is created first, then its checklist, subtasks, parent, tags, dependencies, scheduled blocks, and custom values are written against it, and any that fail are reported on the created task. Closing a changed draft asks before discarding it.

## Sections and ordering

Sections provide project-local organization. They can be created, renamed, reordered, collapsed, hidden, archived, and restored. Archiving a section never deletes its tasks.

Manual order is meaningful only in views and groups that use it; sorting or computed grouping never rewrites it. Moves are computed against the complete sibling order in native storage, not only the loaded view, so a move lands where the user asked even when other siblings are hidden. Custom fields and their options follow the same rule. Moves are atomic and retry-safe.

## Archive and restore

Archive is preferred to deletion when a task has history, scheduled blocks, dependencies, subtasks, Notes, or review context. Archiving a parent also archives its descendants, so active children never hide under an inactive parent, and restore follows the same closure. Archived tasks remain reachable through explicit filters and history.

## Selection and bulk actions

List and Kanban support multi-select with bulk status, priority, scheduling, archive, and restore from the shared selection toolbar. Bulk edits of dates and custom fields are not supported yet. A bulk change commits the complete accepted selection and its history atomically. Missing tasks, mixed projects, or values that changed since selection reject the whole operation rather than silently skipping items. Archive and restore include descendants outside the loaded view, and a failed or uncertain response can be retried with the same operation identity.

## Sidebar and toolbar

On desktop, Projects has one docked sidebar laid out like the Chat channel rail: a project search at the top, then one collapsible section per project of the selected project's group, listing that project's views the way Chat sections list channels. A search lists matching projects from every visible group, matching project or group names. Opening a view of another project selects that project. Each project heading has a plus menu to add a view and a menu to rename the project, move it to another group, open its settings, and hide, show, archive, or restore it; built-in Routine projects keep their name and group, so rename and move appear unavailable. Notes and Chat open the project's pages and channels beside the same sidebar, without the Notes tree or the channel rail, and move between pages and channels through the header breadcrumb; the full explorers and working-Markdown files stay in the Notes and Chat tabs. The header breadcrumb also switches groups and projects and owns their creation; the task search opens with its shortcut. Ctrl (Cmd on Apple) with a digit opens the selected project's views in sidebar order from anywhere in the Projects tab, including while typing in a hosted view, so plain digits stay with the open view, such as the Calendar view's day spans. On desktop, the Calendar view shows its navigation and span controls in the header row while they fit there, and in their own row otherwise. Collapsing the sidebar keeps a narrow rail with the selected project's views only; the collapsed sidebar, collapsed project sections, and each project's open view persist per device. Mobile keeps the header view menu.

## Project views

Views are the entries of a project's sidebar section. A new project starts with Overview (the Dashboard), List, Notes, and Chat; Kanban, Calendar, Gantt, and further copies of any task view are added from the plus menu, so a project can keep several Lists with different filters. Notes and Chat appear at most once per project and are unavailable in the menu while present. A view's menu renames it, moves it up or down, and removes it after confirmation; views also reorder by dragging within their project. The icon always follows the view type, and a blank name restores the type's label. Removing a view removes its saved filters and presentation but never tasks, Notes pages, or Chat channels. Other view types, such as an embedded web page, are planned.

Each task view autosaves its filters, sort, grouping, archived task and inactive section toggles, Calendar span, and List columns and presentation, so returning to the view restores them. The task search text lasts only for the session.

Toolbar actions follow what the view honors: every view offers filters and project settings, List and Kanban offer sorting, and only List offers grouping and columns. Gantt always orders by date. A configuration panel the new view does not offer closes when the view changes.

## List

List shows section or computed groups, configurable columns, inline editing, selection, task creation, and horizontal access to wide schemas. It uses the [shared collection components](../collections.md); task property editors, section controls, subtasks, selection, and manual ordering stay project-specific.

Expanding a task lists its subtasks as indented rows with the same columns, followed by an add-subtask row that creates a subtask in the parent's section with the default status. Subtasks have one level, so their rows have no chevron, and they are not dragged in the list. Expansion lasts for the session.

Columns can include status, dates, priority, assignee, reviewer, estimate, schedule, dependencies, tags, and custom fields. Per List view, users can show, hide, reorder (by dragging a header or from its menu), resize, wrap, and freeze columns, and choose display formats (date style, time style, number grouping or percent). Display choices never change stored values, query predicates, or sorting.

Column headers can insert a custom property beside the current column, rename it, change its icon, edit its options, and delete it after confirmation together with its values. Status, priority, due, and scheduled columns can also group the list. Built-in columns keep their fixed names and icons and cannot be deleted. Duplicating a property creates a new schema with fresh option identities and no copied values.

Column calculations (count, filled, empty, and for numeric columns sum, average, minimum, maximum) run over every task matched by the native query, not just the loaded page. Calculations never show totals from an older filter.

Conditional row colors use ordered project-local status or priority rules; the first matching rule tints the row.

Rows can group by section, status, priority, due-date bucket, or scheduled state. Computed groups are projections over the same tasks, and quick creation pre-fills a group value only when it maps safely to a canonical field.

## Kanban

Kanban groups cards by status and supports creation, movement, selection, and compact task metadata. Moving a card changes its status through the same validation as task detail. With manual sorting, cards can be reordered within a column; other sorts permit status changes without rewriting manual order. Card menus offer movement so drag is never required. A rejected move keeps the previous canonical position.

Hidden or archived statuses never make tasks disappear without an explicit filter or recovery path.

## Calendar

The project Calendar view shows events linked to the project. It reuses canonical Calendar events and task-event links rather than keeping a separate scheduling source of truth. Scheduling follows [Settings and scheduling](settings-and-scheduling.md#scheduling-tasks).

## Gantt

Gantt renders dated tasks and milestones on a proportional timeline with sections, today, overdue, blocked, done, dependencies, and finish-to-start conflict signals. Dragging and resizing propose date changes, and dependency repair follows [Dependency date proposals](settings-and-scheduling.md#dependency-date-proposals).

## Dashboard

Dashboard summarizes task counts, completion, open estimates, scheduled hours, blockers, deadlines, overdue work, unscheduled due tasks, missing estimates, and recent changes and completions. These are planning signals; they never become productivity scores, rankings, or automatic judgments.

## Task detail

Task detail is a focused editor available from every view. It edits a local draft, shows content (description, checklists, subtasks, dependencies, scheduled blocks, activity) beside an always-visible property column, and stacks them on narrow layouts. Unsaved closure uses the shared discard flow. Links to Calendar, Notes, Chat, files, and dependencies remain explicit and navigable. Start and due dates can each carry a reminder with the same offsets as Notes dates; reminders are delivered on desktop while the app runs, are not replayed after the app was closed, and opening one selects the task's project.

## Accessibility and touch

All views provide keyboard routes for navigation, creation, editing, selection, bulk action, movement, and opening task detail. Drag always has a keyboard or menu alternative. Touch layouts use native two-axis panning for wide List and Gantt surfaces and never hide critical actions behind hover.
