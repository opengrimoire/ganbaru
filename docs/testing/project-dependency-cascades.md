# Project dependency cascade testing

**Status: Reference.** Dependency calculation is native; Gantt geometry and date dragging remain TypeScript concerns.

## Automated coverage

Native tests compute complete chains independent of any view window, covering downstream propagation, optional date fields, milestones, cycles, undated or invalid dates, foreign endpoints, overflow, protected work, and bounded input. A failed write on a later task rolls back every earlier date, revision, history row, and the receipt. A committed receipt survives reopening SQLite and does not overwrite a later edit on retry.

Frontend tests validate untrusted preview payloads, suppress late results after project switches, keep the reviewed proposal and operation identity on uncertain apply, and preserve newer canonical task revisions.

## Manual acceptance

Run in the real desktop and Android app:

- Hide a dependency endpoint by filtering or collapsing Gantt. Date review still lists every affected task with original date, proposed date, and reason.
- Apply a branched chain with a milestone and due-date-only tasks. Durations and populated date fields survive reopening the project.
- A completed, archived, or Calendar-scheduled affected task explains the conflict and blocks Apply. Reference-only links allow ordinary date repair.
- A change made in another window after preview makes Apply report that review changed.
- Delay preview and switch projects; the old proposal never appears. Interrupt apply, retry, and confirm dates and history change once.
- Gantt drag feedback stays immediate, and task detail, selection, keyboard navigation, and touch scrolling still work.
