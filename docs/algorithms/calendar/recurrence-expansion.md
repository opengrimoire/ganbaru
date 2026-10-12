# Recurrence expansion

**Status: Implemented.** One native Rust engine (`crates/ganbaru-calendar/src/recurrence/`) is the only expansion authority. Calendar windows, Focus scheduling, Music activation, Android reminder preparation, previews, and scoped mutations all consume it; the frontend caches and filters native occurrences but never generates them.

Recurrence expansion converts one normalized template into bounded concrete occurrences for a requested window.

## Inputs and output

Inputs are:

- A template event with stable identity, start, end, and home zone (or floating dates).
- An optional normalized recurrence rule.
- Exception dates, additional dates, and occurrence overrides.
- A requested window.

The output contains concrete events that overlap the window. An event may begin before the window and still be returned when its end reaches into it. Non-recurring events pass through when they overlap. A template with additional dates but no rule produces its original occurrence plus eligible additional occurrences.

## Supported rules

The engine applies daily, weekly, monthly, and yearly frequency with positive interval, BYDAY (including ordinal weekdays), BYMONTHDAY, BYMONTH, BYYEARDAY, BYWEEKNO, BYSETPOS, WKST, intersections of those selectors, and COUNT or UNTIL termination. Rule parts outside that set, or contradictory combinations, are rejected with a diagnostic rather than approximated. Import may preserve more iCalendar data than the engine applies; preservation is not support. See [iCalendar interoperability](../../interop/icalendar/README.md).

## Civil dates and instants

Recurrence walks civil dates in the event's home zone, not fixed UTC durations, so a weekly 09:00 event stays at 09:00 across offset changes. After selecting a civil date, the engine combines it with the local time and home zone to get instants. Duration across a DST transition follows those instants. The recurrence date remains the identity. UNTIL in UTC is compared against instants.

Generated wall times that do not exist (DST gap) are skipped; repeated wall times use the first occurrence. All-day occurrences use date spans directly. Timed multi-day occurrences keep the template's civil day span and local times. A positive interval that crosses a repeated hour, so that its end wall clock is earlier than its start, keeps its positive elapsed duration on later occurrences.

## Window semantics

An occurrence is returned when its interval overlaps the requested window, compared by exact instants. A home-zone date can differ from the displayed date without losing the occurrence. Generation may fast-forward to the first candidate that could overlap, but must yield the same logical occurrence count as walking from the template; fast-forward is an optimization, not a different rule.

## Expansion order

For one template:

1. Validate the date range, rule shape, positive interval, and bounds.
2. Keep the template ID for its first occurrence and emit it when it overlaps and is not excluded or cancelled.
3. Walk or fast-forward the recurrence period by period.
4. Stop at COUNT, UNTIL, a cancellation-from boundary, the window end, or the work budget.
5. For each eligible date, apply exclusion and cancellation, preserve the day span, then apply an exact-date override.
6. Add distinct additional dates not already generated, excluded, or cancelled.
7. Return deterministic concrete events. Callers apply display ordering.

An override changes occurrence fields while keeping the original series and recurrence-date provenance. A moved override stays associated with the recurrence date it replaced and is filtered against the window after it is applied.

## Concrete identity

The first occurrence keeps the template ID. A generated occurrence uses `<template-id>::YYYY-MM-DD` from the original home-zone recurrence date and records the template as its parent. Identity never depends on title, display time, device zone, or the expansion window. Persisted override rows have their own storage identity, but projections keep the generated occurrence and series relationships needed by edits, project links, and Pomodoro history.

## Invalid dates

Rules such as day 31 do not clamp to day 30 in a shorter month; that period contributes no occurrence, and the original anchor does not drift. Leap days follow the same principle. A nonexistent ordinal weekday contributes no occurrence for that month. Parsing must never normalize an unsupported form into a superficially similar rule with different meaning.

## Exceptions, additional dates, and overrides

Exception dates remove an occurrence by recurrence date. Additional dates add an occurrence with the template's time and span unless an override replaces it. The same date appears at most once; exclusion and cancellation win over emission.

COUNT limits the rule set before exceptions are subtracted, including when the original occurrence is excluded, and nonexistent wall times are omitted before COUNT. Additional dates are independent of the rule's COUNT or UNTIL: they can extend past termination, deduplicate against generated dates, and can themselves be excluded.

## Bounds and failure behavior

One request shares a work budget across every template and override lookup: at most 10,000 emitted occurrences and a fixed candidate-date allowance. Exhausting it is an explicit error that asks for a narrower window, never a silently truncated result. Imports and reads also bound input events and recurrence data before expansion. Invalid data returns a controlled error; it must not hang, allocate without bound, or generate occurrences outside the window to satisfy a malformed count.

## Scoped edit partitions

Scoped edits (`Following`, `All` with protected history) split one series into a historical side and an edited side. Partition membership uses original recurrence dates, even when an override moves the displayed occurrence. Both sides must reload through canonical expansion to reproduce exactly the original occurrences, with no overlap.

- The historical side ends with a COUNT equal to the generated prefix (counted before exclusions), rather than an UNTIL that could round wrongly for fractional timestamps.
- The edited side keeps the original termination. When the boundary occurrence can become the new anchor without changing period selection or duration, its COUNT subtracts the consumed prefix. Future exceptions and additional dates transfer to it.
- When the boundary cannot be a clean anchor (an off-pattern additional date, a week-number rule near a year boundary, or a DST-sensitive interval), the edited side keeps the earlier anchor and uses prefix exceptions, including for cancelled and excluded members, so that the boundary is its first visible occurrence and nothing reappears.
- Prefix enumeration shares the request's work budget and fails explicitly when exceeded.

## Draft geometry on scoped edits

Applying a draft to the edited side follows these rules:

- Metadata-only drafts keep the source geometry.
- A time edit applies the selected occurrence's resolved interval at the edited anchor; the anchor shifts by the selected start's civil-date difference. Rule selectors such as BYDAY are kept as requested, never silently rewritten to match a new start.
- An unchanged rule keeps the partition's cadence and finite termination when possible. A date, zone, or rule change can require a new anchor, which then consumes the original prefix exactly once. Promoting an off-pattern additional date to the anchor removes its old additional-date entry. Prefix exclusions that only kept the old cadence are dropped; user-authored exclusions remain.
- Exception and override identities stay civil-date identities. Additional dates keep their dates and adopt a changed shared time. Date-kind and zone conversions keep the authored meaning of other override endpoints. Converting a timed rule to all-day turns a timed UNTIL into the last admitted home date.
- The resulting series is decoded again through the canonical engine, so a changed rule cannot activate malformed override geometry.

## Examples

### Daily count

A template starting June 1, daily, COUNT 3 has dates June 1, 2, and 3. A June 2 exception leaves June 1 and June 3; it does not create a June 4 replacement.

### Multi-day overlap

An occurrence spanning June 1 through June 3 overlaps a June 3 through June 7 window and is returned.

### Weekly DST change

A 09:00 event in America/New_York stays at 09:00 on each selected weekday; its UTC instant changes with the offset.

### Generated identity

Template `event-1` produces `event-1::2026-06-08` for June 8 whether the caller expands June alone or the whole quarter.

## Conformance

Native tests cover fast-forward, multi-day overlap, exceptions, additional dates, moved overrides, cancellation, invalid month days, DST boundaries, partitions, and budget exhaustion. The shared recurrence-set fixture (`apps/client/src/lib/calendar/recurrence-set-fixtures.json`) is consumed by the canonical engine. Frontend tests cover native identity and projection contracts and import parsing only.
