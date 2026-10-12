# Outlook

Outlook is a practical compatibility target, not the source of truth for iCalendar behavior. Shared procedure, import expectations, and questions are in the [client index](./README.md).

## Source notes

- Microsoft documents importing `.ics` files into Outlook calendars and subscribing to iCalendar feeds. An imported file does not refresh when the source calendar changes.
- Outlook desktop exports iCalendar through calendar sharing, with detail controlled by sharing settings.
- Outlook commonly emits Windows timezone names such as `Pacific Standard Time`. Ganbaru AI maps recognized Windows names to IANA zones for projection and keeps the original `TZID` in preservation. See [recurrence and timezones](../recurrence-and-timezones.md#timezones).

Sources:

- <https://support.microsoft.com/en-us/office/import-or-subscribe-to-a-calendar-in-outlook-com-or-outlook-on-the-web-cff1429c-5af6-41ec-a5b4-74f2c278e98c>
- <https://learn.microsoft.com/en-us/office/vba/api/outlook.calendarsharing.saveasical>

## Client-specific priorities

- Record Outlook on the web, Windows, and macOS separately, because their behavior can differ.
- Include Windows `TZID` values, recurrence across DST and overrides with Windows `TZID`, attachments, private details hidden by sharing settings, meeting invitations, and cancellation and update messages.

## Behavior to verify

- Whether Outlook accepts an IANA `TZID` without a full `VTIMEZONE`.
- Whether Outlook rewrites IANA timezones into Windows names.

## Observed behavior log

No manual Ganbaru AI compatibility run recorded yet.
