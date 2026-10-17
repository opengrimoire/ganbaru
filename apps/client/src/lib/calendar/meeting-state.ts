import type {
  CalendarEvent,
  GuestPermissions,
} from "$lib/calendar/types";

/** Return whether guest permissions differ from the calendar defaults. */
export function hasNonDefaultGuestPermissions(
  value: GuestPermissions | undefined,
): boolean {
  return !!value && (value.canModify || !value.canInviteOthers || !value.canSeeOtherGuests);
}

/** Return whether an event carries any state that requires meeting persistence or UI. */
export function hasMeetingState(value: Partial<CalendarEvent>): boolean {
  return value.meetingEnabled === true
    || !!(value.attendees && value.attendees.length > 0)
    || !!value.organizer
    || !!value.location
    || !!value.url
    || !!value.geo
    || value.localParticipationStatus !== undefined
    || hasNonDefaultGuestPermissions(value.guestPermissions);
}
