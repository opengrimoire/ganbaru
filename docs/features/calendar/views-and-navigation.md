# Calendar views and navigation

## View modes

**Day view** shows one day with timed and all-day regions. It is the default mobile view and the most detailed planning and focus surface.

**Work-cycle view** shows a configurable compact span for the user's working rhythm without assuming a Monday-to-Friday culture.

**Week view** shows seven days and supports cross-day planning.

**Month view** summarizes each day with compact event chips and opens a full-day list when events do not fit legibly. It is a planning surface and does not render the Pomodoro timeline rail.

Changing views never stops an active session. Detailed views rebuild the event block and progress rail from recorded timer history. If that history cannot load (for example it exceeds its size limit), navigation continues and a localized notice offers retry or a shorter range; a failed read is never shown as empty history.

## Navigation

Users can move by the view's natural interval, jump to today, select a date, and use keyboard shortcuts when focus is not inside an editor, picker, or dialog. Held-key navigation is paced and cancellable.

Rapid navigation is latest-request-wins: older data never replaces a newer window. Prefetch may speed up adjacent navigation but never blocks foreground movement or becomes the source of truth.

Windows contain native occurrences read from one consistent SQLite snapshot; the frontend reuses covering windows by filtering, never by generating recurrence. Committed edits invalidate older cached and in-flight reads. Window membership compares exact instants, so an occurrence whose home-zone date differs from the displayed date still appears without changing its identity. Rendering, Focus, and notifications share this rule.

## Scrolling and zoom

Day and week timelines keep a stable relationship between scroll position, wall-clock time, and event geometry. Zoom changes time density without changing event instants and keeps the user's temporal anchor when practical.

Automatic scrolling to the active event or current time must not fight recent manual scrolling.

## Timed events

Timed blocks occupy their interval and share horizontal space when they overlap. Dragging an empty interval creates a draft; dragging an event moves it; resizing changes the corresponding boundary.

Active events protect their recorded start. Direct manipulation may change the end within valid limits but cannot move the block or its start.

## All-day events

All-day events use inclusive dates; a one-day event has equal start and end dates. The iCalendar codec converts to and from the exclusive `DTEND`. All-day events render in the all-day band and month cells, and multi-day events keep one identity across their span.

Turning a timed event into all-day must not shift its intended dates through the device zone. Turning an all-day event into timed requires an explicit time and duration.

## Month overflow

Month cells keep labels legible before maximizing count. When events do not fit, a localized `+N more` control opens a scrollable day list whose rows open the normal event panel. Closing the list also closes any nested panel through the normal unsaved-change flow.

## Calendar visibility

Visibility affects presentation and scheduling queries without deleting events. The built-in local calendar cannot be deleted. Deleting another calendar follows event protection rules: future untracked events are removed, while protected history is archived with its source identity.

## Accessibility

Every view provides a non-pointer route for navigation, event opening, creation, and overflow access. Color is never the only status signal; patterns, labels, and accessible names communicate cancellation, response state, active focus, and controls.

Timed blocks and all-day chips are focusable with accessible names, and keyboard activation opens the same panel as a click without starting a drag. Touch layouts keep adequately sized controls and native panning; hover affordances also appear on focus.
