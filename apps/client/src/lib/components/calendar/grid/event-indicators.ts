import type { CalendarEvent } from "$lib/calendar/types";

export interface EventIndicatorState {
  hasRepeat: boolean;
  hasCallLink: boolean;
  hasLocation: boolean;
  hasGenericMeeting: boolean;
  iconCount: number;
}

export interface EventMeetingIndicatorState {
  hasCallLink: boolean;
  hasLocation: boolean;
  hasGenericMeeting: boolean;
  iconCount: number;
}

export function getMeetingIndicatorState(event: CalendarEvent): EventMeetingIndicatorState {
  const hasCallLink = event.hasCallLink === true || !!event.url;
  const hasLocation = !!event.location;
  const hasGenericMeeting = event.meetingEnabled === true && !hasCallLink && !hasLocation;
  const iconCount = [
    hasCallLink,
    hasLocation,
    hasGenericMeeting,
  ].filter(Boolean).length;

  return {
    hasCallLink,
    hasLocation,
    hasGenericMeeting,
    iconCount,
  };
}

export function getEventIndicatorState(event: CalendarEvent): EventIndicatorState {
  const hasRepeat = !!event.recurrence || !!event.recurringParentId;
  const meetingIndicators = getMeetingIndicatorState(event);

  return {
    hasRepeat,
    hasCallLink: meetingIndicators.hasCallLink,
    hasLocation: meetingIndicators.hasLocation,
    hasGenericMeeting: meetingIndicators.hasGenericMeeting,
    iconCount: meetingIndicators.iconCount + (hasRepeat ? 1 : 0),
  };
}
