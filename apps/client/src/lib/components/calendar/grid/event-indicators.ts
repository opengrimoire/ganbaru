import type { CalendarEvent } from "$lib/calendar/types";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

export interface EventIndicatorState {
  hasMusic: boolean;
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

const SOUNDSCAPES_AVAILABLE = platformHasCapability(BUILD_PLATFORM_PROFILE, "music.soundscapes");

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

/**
 * Derives the icons shown on a Calendar event surface.
 *
 * @param event - Event to inspect.
 * @param soundscapesAvailable - Whether this platform plays soundscapes, so a soundscape-only event counts as music.
 * @returns Which icons to show and how many.
 */
export function getEventIndicatorState(
  event: CalendarEvent,
  soundscapesAvailable = SOUNDSCAPES_AVAILABLE,
): EventIndicatorState {
  const hasMusic = event.hasMusicPlaylist === true || (soundscapesAvailable && event.hasSoundscape === true);
  const hasRepeat = !!event.recurrence || !!event.recurringParentId;
  const meetingIndicators = getMeetingIndicatorState(event);

  return {
    hasMusic,
    hasRepeat,
    hasCallLink: meetingIndicators.hasCallLink,
    hasLocation: meetingIndicators.hasLocation,
    hasGenericMeeting: meetingIndicators.hasGenericMeeting,
    iconCount: meetingIndicators.iconCount + (hasRepeat ? 1 : 0) + (hasMusic ? 1 : 0),
  };
}
