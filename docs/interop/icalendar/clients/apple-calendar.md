# Apple Calendar

Apple Calendar is a practical compatibility target, not the source of truth for iCalendar behavior. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Apple documents exporting an individual calendar's events to a `.ics` file and importing events from one.
- Apple calendar archives use `.icbu`; those are outside the `.ics` compatibility goal.

Source: <https://support.apple.com/en-afri/guide/calendar/icl1023>

## Client-specific priorities

- Test macOS Calendar first, because it supports direct file import and export, and record the macOS version.
- Include a `.ics` export from macOS Calendar, private events, attachments or `URL` fields, and custom timezone export.

## Behavior to verify

- Whether Apple Calendar rewrites line folding or parameter escaping.
- Whether it stores attachments as URI values or inline data.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
