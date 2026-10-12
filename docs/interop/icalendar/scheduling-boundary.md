# Scheduling boundary

Status: Reference. Scheduling metadata is preserved offline; transport-backed scheduling is Planned.

iCalendar scheduling metadata is data. Sending scheduling messages is a separate capability that needs identity and a transport. See the [decision record](./decisions.md#2026-05-14-preserve-scheduling-metadata-but-do-not-act-without-transport).

## Offline compatibility

Without any account, Ganbaru AI parses, preserves, and exports scheduling fields: `METHOD`, `ORGANIZER`, `ATTENDEE` with `PARTSTAT`, `RSVP`, `ROLE`, `CUTYPE`, `DELEGATED-FROM`, `DELEGATED-TO`, `SENT-BY`, and `MEMBER`, `REQUEST-STATUS`, `STATUS:CANCELLED`, and the recurrence overrides used for updates and cancellations.

## Transport-backed actions (planned)

These require identity and a user-configured transport (email, CalDAV, Google, Microsoft, or another provider): sending invitations, cancellations, and RSVP replies; updating someone else's calendar; notifying attendees; sending email alarms; and remote sync. No transport is required for base import and export.

## UI policy

Ganbaru AI has no identity model yet. Therefore:

- Imported attendee response status is read-only metadata, not proof that a reply was sent.
- The user cannot accept, decline, or tentatively accept as another attendee.
- Organizer-side edits, such as marking a guest optional, are allowed only where editing the event is allowed.
- Offline edits never imply that attendees were notified.
- Local RSVP state is app-local metadata and is never exported as an `ATTENDEE`.

With future identity support, the app can match the current user's attendee row and update it, but sending the reply still requires a transport.

## Import and export

- Import preserves `METHOD` and all organizer and attendee parameters, projects event data where possible, and treats the result as the user's local copy.
- Export reuses a preserved `METHOD` only when the calendar has exactly one valid distinct method. Missing, invalid, or mixed methods normalize to `METHOD:PUBLISH`, because the generated `VCALENDAR` carries one object-level method. This fallback does not preserve the meaning of mixed methods; a future backup mode may omit `METHOD` instead.
- Ordinary export never generates `METHOD:REQUEST` and never fabricates attendee replies.
- Planned: user-visible diagnostics when an edited offline invitation is exported, and an import note that invitation workflow semantics are not acted on.

## Future export modes

Each mode should declare whether it preserves, strips, or generates scheduling metadata:

- **Calendar backup:** calendar data with no send semantics.
- **Invitation file:** one event as a scheduling object for manual sending.
- **Transport send:** delivery through a configured email, CalDAV, or provider API.
- **Subscription feed:** a read-only feed shape.
