# Calendar event editing

## Event fields

A local event can contain:

- An optional plain-text title.
- Timed start and end instants with a home zone, or an all-day date range.
- One of 32 stable event-color slots.
- Sanitized rich description content.
- Zero or more notification offsets.
- Calendar identity, visibility, free/busy state, status, and imported metadata.
- Optional meeting location, call link, organizer, and attendee metadata.
- Optional project, task links, Pomodoro configuration, idle behavior, and music assignments.
- An optional recurrence rule and preserved interoperability data.

The [Calendar schema](../../data/schema/calendar.md) owns storage and limits. The editor communicates user-visible limits before save and retains unsupported imported material through the [interoperability preservation boundary](../../interop/icalendar/README.md).

## Create and edit flow

Clicking or dragging an empty time range creates a draft and opens the event panel. A new all-day event starts from a date selection. The draft may remain untitled.

The panel groups time, calendar, project, focus, music, meeting, description, recurrence, and notification controls without requiring every group for a simple event. Save validates one coherent draft. Closing a dirty panel asks for discard confirmation. Recurring events use the scoped behavior in [Recurrence editing](recurrence-editing.md).

Save, drag saves, `End now`, and `Enable Focus` are native semantic commands: the Calendar change and its Focus consequences commit together in Rust. If a reply is lost, the panel shows an unresolved Save notice with Retry that resubmits the exact original request; a newer draft cannot take its place. Pending saves disable mutation controls, and a delayed confirmation never saves a different event that the user has since selected.

## Project and task linkage

The project selector shows active groups and projects, not the task tree. Selecting a project can fill an empty title from the project's default event name and applies compatible defaults to a non-active draft. It never replaces an existing title.

Project defaults can include color, duration, Pomodoro mode, idle behavior, and phase soundtrack assignments. Active events do not take a duration default, because changing projects must not resize work already in progress.

Tasks link through explicit task-event link records. Links require the task and event to belong to the same project. Unlinking does not delete either record.

## Meeting metadata

Locally authored events can edit location, call link, organizer, and attendees. Attendees who use Ganbaru AI are chosen from contacts with the shared participant picker and show a response chip (accepted, tentative, pending, declined), as described in [Contacts and invitations](../collaboration/README.md). Imported organizer and attendee identities remain read-only until Ganbaru AI can prove the current user's identity and write permission.

The app never changes another attendee's response based only on a displayed email or label. Local response metadata may drive local rendering without becoming external attendee data.

## Notifications

Notifications use native platform delivery. Each reminder identifies the event, lead time, interval or all-day state, and location when available. Tapping a notification opens the relevant Calendar context.

Desktop schedules while the app runtime is available and catches up within a bounded interval after resume. Android keeps a bounded native schedule so reminders fire while the WebView and Rust runtime are absent. See [Android permissions and security](../../platforms/android/permissions-and-security.md).

## Status and visual meaning

Event color is the selected palette slot. Dimming past events is an optional presentation preference. Free/busy is scheduling metadata, not decoration.

Cancelled or declined events use a pattern and text treatment in addition to color. Response rendering (accepted, tentative, pending, declined) appears only when a matching local identity or explicit local response exists.

## Active-session protection

An event with an active Pomodoro run cannot be deleted, archived, moved, retimed from the start, or structurally changed in a way that detaches the active run. The primary action is to end the event or stop the run first. `End now` completes the run at the native acceptance time.

Edits that materialize a recurring active occurrence transfer run references atomically. Historical timer records are never restarted or rewritten to imitate an undo.

## Coordination and capacity

Calendar conflict detection is advisory. Overlap is permitted because meetings, travel, parallel reminders, and all-day context can legitimately coexist. Capacity views explain conflicts and insufficient time without silently moving commitments.

Any future automatic scheduling presents a proposal before mutation and preserves explicit user locks, dependencies, protected history, and working-hour preferences.

## Import and export

iCalendar behavior, loss boundaries, recurrence, timezone handling, merge policy, and conformance fixtures are owned by [iCalendar interoperability](../../interop/icalendar/README.md).
