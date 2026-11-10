# Diary

Diary is a planned private reflection feature with short morning and evening touchpoints. It should help the user notice patterns without turning mood, energy, or rest into performance scores.

## Morning entry

The planned morning entry can be opened manually or after a future sleep-alarm dismissal. It captures:

- Mood through a small optional scale.
- Energy through a small optional scale.
- Self-reported sleep quality.
- A short intention or selected planned task.

Any sleep suggestion must use explicit bedtime or sleep-window information and be labeled as an estimate. Alarm configuration time is not bedtime and cannot be treated as sleep duration.

After entry, the app can show the day's planned blocks and optional morning routine. The entry remains skippable.

## Evening entry

The evening entry can capture:

- What went well.
- What was difficult.
- Optional free-form reflection.
- One next-day priority.

A wind-down summary uses neutral planning language. It does not grade the day, shame incomplete work, or optimize for time spent in the app.

## Storage

Diary entries are planned as SQLite rows in the vault database, like Notes, so they get the same transactions, device sync, conflict handling, history, and encrypted backups as every other personal domain. Export writes Markdown to a location the user picks; it is never the canonical entry. See [data architecture](../data/architecture.md).

## Privacy and personal baselines

Mood, energy, rest, and reflection are highly private. A project channel, collaborator, manager workflow, report, or shared AI context package never receives raw diary data through project membership.

An explicitly authorized personal AI action may use a bounded derived capacity signal or selected entries. The UI states what leaves the device and which provider receives it. Local trends are suggestions, not medical conclusions or measures of personal value.

See [Sleep alarm](sleep-alarm.md) and [AI integrations](ai/README.md).
