# Google Calendar

Google Calendar is a practical compatibility target, not the source of truth for iCalendar behavior. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Google Calendar Help documents importing `.ics` files on desktop and notes that imported events do not stay synced between accounts.
- Google Calendar Help states that guests and conference data are not imported when importing an event file.
- Google Calendar export Help lists the iCalendar fields included in downloads: event start and end time, recurrence frequency data, invitees and response statuses, title, description, location, creation time, and last modification time. It does not promise event colors, Google Meet state, or every Google Calendar API field.
- The Google Calendar API event resource documents event `end` as exclusive, matching RFC 5545 all-day behavior.
- Google exports can arrive as a zip with one `.ics` file per calendar.

Sources:

- <https://support.google.com/calendar/answer/37118?hl=EN>
- <https://support.google.com/calendar/answer/10041132?hl=en>
- <https://developers.google.com/workspace/calendar/api/v3/reference/events>

## Known Google file quirks

These are client-compatibility notes, not standards rules. Ganbaru AI should stay standards-first and use these notes to decide warnings, repair prompts, and fixture priorities.

- **Stale unbounded recurrences can appear in exports.** A Google export can contain old recurring master `VEVENT`s that no longer appear in the Google Calendar UI. The problematic shape is a confirmed master event with `RRULE:FREQ=DAILY` or similar, no `UNTIL`, no `COUNT`, no `EXDATE` covering the future, no `STATUS:CANCELLED`, and no `RECURRENCE-ID;RANGE=THISANDFUTURE` cancellation. Per iCalendar semantics, that file says the event repeats forever. Ganbaru AI must not silently invent an end date, but a future import review should warn about old unbounded recurring events and let the user exclude them.
- **Google can mix UTC and `TZID` event times in the same export.** For example, one event may use `DTSTART;TZID=America/Mexico_City:20210510T080000` while another uses `DTSTART:20260515T204500Z`. Both are legal. Import must treat `Z` values as UTC, `TZID` values as local to the given zone, and persist the projected wall clock without applying the timezone offset twice.
- **"This and following" deletion is not always exported as a future cancellation.** When the file contains a capped `RRULE` with `UNTIL`, Ganbaru AI should respect it. When the file contains `RECURRENCE-ID;RANGE=THISANDFUTURE` with `STATUS:CANCELLED`, Ganbaru AI should suppress that occurrence and later occurrences. When Google exports neither shape and leaves an unbounded confirmed master event, the file is incomplete relative to the Google UI state.
- **Duplicate master revisions may appear.** If two master `VEVENT`s share a `UID`, Ganbaru AI projects the newest revision by `SEQUENCE`, then `LAST-MODIFIED`, `DTSTAMP`, or `CREATED`. This handles files where a newer capped recurrence appears next to an older uncapped revision.
- **Colors are not reliable through `.ics`.** Google Calendar API resources expose color IDs, but Google `.ics` exports do not reliably carry original calendar or event colors. File import should map to Ganbaru AI's default palette unless a standard or extension color property is actually present.
- **Guests and conference data are not symmetric through file import.** Google exports can include attendees, organizer data, `X-GOOGLE-CONFERENCE`, and generated Google Meet text inside `DESCRIPTION`, but Google Help states that importing an event file into Google does not import guests and conference data. Ganbaru AI should preserve these fields when present, promote Google Meet conference URLs into the call-link field, and avoid showing generated Meet boilerplate as a user-authored description when it can identify the block. File round trips through Google may still drop or rewrite those fields.

## Ganbaru AI policy for Google quirks

- Treat the `.ics` file as the immediate source of truth for import.
- Do not auto-repair stale Google recurrence data without user consent.
- Surface suspicious old unbounded recurrences as import warnings once the import review UI exists.
- Keep exporting "following" deletes as capped recurrence rules so other clients receive a bounded series.
- Preserve raw Google fields where possible, but do not promise Google API-only behavior from file import/export.

## Client-specific priorities

- Yearly all-day birthday or anniversary
- Mixed UTC and `TZID` values in one export
- Events with guests, guest permissions, and Google Meet or conference data
- Free (`TRANSP:TRANSPARENT`) and private (`CLASS:PRIVATE`) events; `TRANSP` stays scheduling metadata, not a color modifier
- An exported calendar zip with multiple `.ics` files

## Behavior to verify

- Whether Google rewrites organizer and attendee fields on import.
- Whether Google drops conference data from imported files.
- Whether Google exports stale unbounded recurring masters that are hidden in the UI.
- Whether Google represents "this and following" edits as a capped `RRULE`, split master events, `RANGE=THISANDFUTURE`, or no visible marker.
- Whether Google accepts binary attachments or only URI attachments.

## Observed behavior log

Partial manual compatibility run recorded in May 2026:

- Ganbaru AI export imported into Google preserved ordinary timed events and a recurring series capped by the app's "following" delete behavior.
- Google export back to Ganbaru AI included expected May 2026 events as UTC `Z` values, while older recurring routine events used `TZID=America/Mexico_City`.
- The same Google export contained old unbounded daily recurrences that were not visible in the Google Calendar UI. The file represented them as confirmed infinite recurrences, so a standards-based importer expands them.
- A Google invitation with an empty title was exported with organizer, attendees, and conference properties. Ganbaru AI should import it as an event with an empty stored title unless the UI adds a display-only "Untitled" fallback.

Future runs should preserve the date and account context required by the shared result record so behavior changes can be distinguished from the May 2026 observations.
