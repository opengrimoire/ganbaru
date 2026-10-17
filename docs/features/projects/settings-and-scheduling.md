# Project settings and scheduling

## Project defaults

Project settings can define:

- Name and managed icon or emoji.
- Default event color, optional event name, and timed duration or all-day behavior.
- Default Pomodoro mode and custom rhythm.
- Global or project-specific idle behavior.
- Focus, short break, and long break playlists.
- Project-local statuses, priorities, tags, and custom fields.
- Working folders where the platform supports them.

New projects default to no event name, the adaptive Pomodoro preset, global idle behavior, and no playlists unless a template says otherwise.

Playlist selectors list real Music playlists. Choosing a playlist plays it automatically during that phase; choosing None means silence. A missing or deleted playlist remains an explicit unavailable assignment with a repair path. Track-level settings stay in Music playlist membership data; see [Music automation](../music/automation.md).

Work-environment and anti-distraction defaults are planned and must not appear as functional selectors until their storage and runtime behavior exist.

## Default application

Defaults initialize a new event or task-originated schedule. They do not continuously inherit after creation.

Selecting a project in an event draft can fill an empty title, color, Pomodoro mode, idle behavior, and soundtrack intent. It never replaces authored title text or resizes an event that already has deliberate timing. Created events keep a snapshot of the project's phase soundtrack assignments, so later default changes do not alter them.

## Statuses, priorities, and tags

Statuses and priorities are project-local, ordered, named, and colored with stable palette slots. Statuses also have a semantic category: not started, active, blocked, or done. Deleting an in-use value requires reassignment or is rejected. Reordering changes presentation, not task identity or history.

Tags are project-local labeled colors with stable identity and order. Deleting a tag removes its task associations while preserving tasks and their history.

## Custom fields

Projects support text, number, select, multi-select, status, date, person, files, checkbox, URL, phone, and email custom fields. Select-like fields own stable options. Schema changes validate existing values, and deleting a field requires confirmation before its values are removed.

## Project templates

Built-in templates (Blank, Software, Course, Routine, Reading, and Chores) initialize sections plus the default statuses and priorities. Applying a template produces independent canonical rows; there is no ongoing inheritance.

Planned: user-defined templates that can also carry settings, tags, custom fields, and planning defaults, with the same no-inheritance rule.

## Scheduling tasks

Scheduling creates a Calendar event from the task title and the project's color, duration, Pomodoro, idle, and soundtrack defaults, and connects task and event through an explicit link. An all-day project default creates an all-day event without Focus configuration. Bulk scheduling creates one independent, linked event per selected task.

The duration chosen in the scheduling form overrides the project default. Timed events in a batch run consecutively, and the task start and target end dates follow the scheduled day. Due dates, completion, status, and unrelated fields stay intact.

A batch is one native transaction: it rejects entirely if any task is archived, missing, from another project, or changed since review, and a changed project default requires a new review. An uncertain response can be retried without creating duplicate events.

Existing project events can also be linked from task detail; a link is valid only when task and event belong to the same project. Unlinking preserves both records, and archiving either side preserves the relationship as history.

## Dependency date proposals

When dated tasks conflict with finish-to-start dependencies, the native service reads the complete canonical project graph and proposes downstream shifts. Gantt filters, collapsed sections, and paging do not limit the proposal. The preview lists every affected task with original and proposed dates and the dependencies that require each change. Durations and the set of populated date fields are preserved, and a milestone remains a single date.

Completed tasks, archived tasks, and tasks with scheduled Calendar links are protected. Cycles, missing or foreign endpoints, undated dependencies, and invalid dates produce explicit conflicts, and a proposal with a conflict cannot be partially applied.

Applying is explicit and transactional. If any task, dependency, completion state, or scheduling commitment changed after review, the apply is rejected and a new preview is required; an accepted cascade never silently expands. Only task date fields change, together with their history, and retrying after an uncertain response returns the original result.

Planned: explicit date locks and per-proposal protection overrides. The task schema has no date-lock field yet.

## Working folders

A project can own a managed working folder (created inside the vault) and device-local bindings to external folders where supported. Organizational Chat runs resolve their execution target through the authorized binding; see [Chat execution and workspace](../chat/execution-and-workspace.md).

Working-folder files remain filesystem-canonical. Project settings store identity, provider preference, and primary selection, not file contents. Mobile omits local coding execution and desktop path selection.

Each folder can name a preferred Chat provider instance or Automatic. Unconfigured providers are visible with their setup status but cannot be saved, and a removed provider stays visibly unavailable until replaced. Missing bindings and repository mismatches are shown on the folder row.

Folder changes are part of the settings draft. Choosing a path previews it without creating an association or changing a binding; Save applies it only to the authorized project and active vault after checking that the folder was not replaced. Removing an external folder requires confirmation and never deletes its files.

## Settings lifecycle

All settings changes, including deletions and reordering, are staged until Save settings; Discard restores the last saved state, and closing with unsaved changes prompts. Save validates the draft first and applies mutations sequentially. If a step fails, completed steps stay saved, the remaining changes stay in the draft for retry, and retry does not duplicate completed creations.

Settings remain usable on narrow layouts without requiring hover or drag.
