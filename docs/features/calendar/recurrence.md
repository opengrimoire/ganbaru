# Calendar recurrence

Recurring events use a template-plus-occurrence model. The template is canonical structured data; visible occurrences are expanded natively for the requested window.

## Supported rules

Ganbaru AI supports daily, weekly, monthly, and yearly frequency with interval, weekday, month-day, ordinal weekday, set-position, week-number, and year-day selection, count or end date, exception dates, and additional dates. Imported rule parts beyond that set are preserved according to the [iCalendar interoperability contract](../../interop/icalendar/README.md) even when the app cannot apply them. Exact expansion rules are in [Recurrence expansion](../../algorithms/calendar/recurrence-expansion.md).

The editor offers common presets and a bounded advanced editor. It must not silently simplify an imported rule in a way that changes future occurrences. The current import path still violates this for sub-daily rules; see [critical compatibility gaps](../../interop/icalendar/conformance/README.md#critical-compatibility-gaps).

## Identity model

The persisted template row is also the first occurrence and uses the template ID. Later generated occurrences use a deterministic identity:

```text
<template-id>::YYYY-MM-DD
```

The date is the original recurrence date in the event's home zone, so identity stays stable even when an override moves the displayed occurrence. Each occurrence has independent Pomodoro history; work recorded on one occurrence never carries into another.

Pomodoro records keep both a live event reference and the exact original occurrence identity. Archiving can clear the live reference while historical joins continue through the original identity.

## Protected occurrences

An occurrence is protected when it has started or has a run, segment, override, exception, active session, or another durable reference. Protection is evaluated across the whole affected recurrence range, not only the visible window. Timed occurrences compare canonical instants; all-day occurrences compare dates against the device's current civil date.

Protected occurrences need not form one continuous prefix: an override can move an earlier identity into the future, and recorded execution can protect a later one. Structural edits preserve protected meaning through one of these forms:

- A historical template capped at the protected boundary.
- A detached standalone event with its own identity.
- An archived event when it should no longer appear as active calendar data.

Future untracked occurrences remain mutable.

## Structural operations

### Only this

The selected occurrence is excluded from the source template and becomes a standalone event, or an independent recurring template if the user set a different rule. Run references for that occurrence transfer atomically when its identity changes. Use this for a one-off variation.

### Following

The old template is capped before the selected occurrence. If recurrence remains enabled, a new template begins at the selected occurrence; if recurrence is cleared, the selected occurrence becomes one standalone survivor. Existing exceptions on the new side transfer with it, so an occurrence previously detached, archived, or deleted cannot regenerate after a split. Use this for a permanent change from a selected point forward.

### All

Without protected history, the template is updated directly. Otherwise the old template keeps the protected side and a new template begins at the first mutable occurrence; protected identities on the new side are preserved individually rather than making intervening mutable occurrences uneditable.

If recurrence is cleared, protected history remains and the selected occurrence becomes the one non-recurring survivor on the mutable side. A protected selection cannot become the edited survivor, and an exhausted series with no mutable occurrence rejects edits instead of inventing a new future series.

### Adding recurrence

Adding recurrence to a non-recurring event converts it into a template that keeps its ID as the first occurrence. No scope selector appears.

## Delete and archive scope

Scoped deletion follows the same identity and protection rules. See [Deletion and undo](deletion-and-undo.md).

## Active sessions

The selected active occurrence can only use `Only this`, and the scope selector is hidden. It may change a valid end boundary but cannot move its recorded start or edit the repeat chain.

If another occurrence in the affected series is active, it stays on the protected side or is materialized unchanged before the edit, and its run reference transfers in the same transaction.

## Time shifts

Template-wide time changes apply only to mutable occurrences. Historical blocks keep their original time so Pomodoro segments stay aligned visually and analytically.

See [Recurrence editing](recurrence-editing.md) and [data invariants](../../data/invariants.md).
