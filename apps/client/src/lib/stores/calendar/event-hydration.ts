import type {
  AttendeeStatus,
  CalendarEvent,
  EventOverride,
  EventVisibility,
  IcalendarPreservationStatus,
} from "$lib/calendar/types";
import { sanitizeCalendarDescriptionHtml } from "$lib/calendar/description-sanitizer";
import {
  mapAlarm,
  mapAttendee,
  mapOverride,
  mapRow,
  mapWindowAttendee,
  type DbAlarm,
  type DbAttendee,
  type DbCalendarEvent,
  type DbOverride,
  type DbWindowAttendee,
} from "$lib/calendar/db-rows";
import {
  parseJsonEventOrganizer,
  parseJsonGeoCoordinates,
  parseJsonStringArray,
  parseJsonStringRecord,
  safeJsonParse,
} from "$lib/calendar/json-fields";

export type DbFullEvent = DbCalendarEvent & {
  description: string | null;
  url: string | null;
  source_uid: string | null;
  visibility: string;
  priority: number | null;
  categories: string | null;
  geo: string | null;
  sequence: number;
  extended_properties: string | null;
  organizer: string | null;
  meeting_enabled: number;
  guest_can_modify: number;
  guest_can_invite_others: number;
  guest_can_see_other_guests: number;
  icalendar_component_id: string | null;
  icalendar_preservation_status: IcalendarPreservationStatus | null;
  icalendar_projection_warnings: string | null;
  icalendar_raw_jcal: string | null;
};

export type DbFullOverride = DbOverride & {
  description: string | null;
  location: string | null;
  url: string | null;
  visibility: string | null;
  extended_properties: string | null;
  icalendar_component_id: string | null;
  icalendar_raw_jcal: string | null;
};

export interface CalendarWindowRows {
  events: DbCalendarEvent[];
  overrides: DbOverride[];
  attendees: DbWindowAttendee[];
  total_event_count: number | null;
}

export interface CalendarNotificationSchedulerRows {
  events: DbCalendarEvent[];
  overrides: DbOverride[];
}

export interface CalendarPanelEventRows {
  event: DbFullEvent | null;
  attendees: DbAttendee[];
}

export interface CalendarFullEventRows {
  event: DbFullEvent | null;
  attendees: DbAttendee[];
  alarms: DbAlarm[];
  overrides: DbFullOverride[];
}

export interface CalendarIcalendarExportMetadata {
  method: string | null;
  mixed_methods: boolean;
}

function applyFullEventFields(row: DbFullEvent, event: CalendarEvent) {
  if (row.description) event.description = sanitizeCalendarDescriptionHtml(row.description);
  if (row.url) event.url = row.url;
  if (row.source_uid) event.sourceUid = row.source_uid;
  if (row.visibility && row.visibility !== "public") {
    event.visibility = row.visibility as EventVisibility;
  }
  if (row.priority != null) event.priority = row.priority;
  const categories = parseJsonStringArray(row.categories);
  if (categories) event.categories = categories;
  const geo = parseJsonGeoCoordinates(row.geo);
  if (geo) event.geo = geo;
  if (row.sequence) event.sequence = row.sequence;
  const extendedProperties = parseJsonStringRecord(row.extended_properties);
  if (extendedProperties) event.extendedProperties = extendedProperties;
  const organizer = parseJsonEventOrganizer(row.organizer);
  if (organizer) event.organizer = organizer;
  if (row.meeting_enabled === 1) event.meetingEnabled = true;
  if (row.local_rsvp_status) {
    event.localParticipationStatus = row.local_rsvp_status as AttendeeStatus;
  }
  if (row.guest_can_modify === 1
    || row.guest_can_invite_others === 0
    || row.guest_can_see_other_guests === 0) {
    event.guestPermissions = {
      canModify: row.guest_can_modify === 1,
      canInviteOthers: row.guest_can_invite_others !== 0,
      canSeeOtherGuests: row.guest_can_see_other_guests !== 0,
    };
  }
  if (row.icalendar_component_id) event.icalendarComponentId = row.icalendar_component_id;
  if (row.icalendar_preservation_status) {
    event.icalendarPreservationStatus = row.icalendar_preservation_status;
  }
  const projectionWarnings = parseJsonStringArray(row.icalendar_projection_warnings);
  if (projectionWarnings) event.icalendarProjectionWarnings = projectionWarnings;
  const rawJcal = safeJsonParse(row.icalendar_raw_jcal);
  if (rawJcal) event.icalendarRawJcal = rawJcal;
}

function mapOverrides(
  rows: DbOverride[],
  renderZone: string,
  parentAllDayById: Map<string, boolean>,
): Map<string, EventOverride[]> {
  const overridesByParentId = new Map<string, EventOverride[]>();
  for (const row of rows) {
    const overrides = overridesByParentId.get(row.parent_event_id) ?? [];
    overrides.push(mapOverride(row, renderZone, parentAllDayById.get(row.parent_event_id) === true));
    overridesByParentId.set(row.parent_event_id, overrides);
  }
  return overridesByParentId;
}

export function mapWindowRows(rows: CalendarWindowRows, renderZone: string): CalendarEvent[] {
  const mapped = rows.events.map((r) => mapRow(r, renderZone));
  if (mapped.length === 0) return mapped;

  const parentAllDayById = new Map(mapped.map((event) => [event.id, event.allDay === true]));
  const overridesByParentId = mapOverrides(rows.overrides, renderZone, parentAllDayById);
  const surfaceAttendeesByEventId = new Map<string, DbWindowAttendee[]>();
  for (const attendee of rows.attendees) {
    const existing = surfaceAttendeesByEventId.get(attendee.event_id);
    if (existing) existing.push(attendee);
    else surfaceAttendeesByEventId.set(attendee.event_id, [attendee]);
  }
  for (const event of mapped) {
    const overrides = overridesByParentId.get(event.id);
    if (overrides?.length) event.overrides = overrides;
    const surfaceAttendees = surfaceAttendeesByEventId.get(event.id);
    if (surfaceAttendees?.length) {
      event.surfaceAttendees = surfaceAttendees.map(mapWindowAttendee);
    }
  }
  return mapped;
}

export function slimEvent(event: CalendarEvent): CalendarEvent {
  const slim: CalendarEvent = {
    id: event.id,
    title: event.title,
    start: event.start,
    end: event.end,
    timezone: event.timezone,
    calendarId: event.calendarId,
  };
  if (event.projectId) slim.projectId = event.projectId;
  if (event.environmentId) slim.environmentId = event.environmentId;
  if (event.playlistId) slim.playlistId = event.playlistId;
  if (event.color !== undefined) slim.color = event.color;
  if (event.recurrence) slim.recurrence = event.recurrence;
  if (event.notifications && event.notifications.length > 0) slim.notifications = event.notifications;
  if (event.exceptions && event.exceptions.length > 0) slim.exceptions = event.exceptions;
  if (event.recurringParentId) slim.recurringParentId = event.recurringParentId;
  if (event.recurrenceDate) slim.recurrenceDate = event.recurrenceDate;
  if (event.allDay) slim.allDay = true;
  if (event.meetingEnabled) slim.meetingEnabled = true;
  if (event.hasCallLink || event.url) slim.hasCallLink = true;
  if (event.location) slim.location = event.location;
  if (event.transparency === "transparent") slim.transparency = "transparent";
  if (event.status && event.status !== "confirmed") slim.status = event.status;
  if (event.localParticipationStatus) slim.localParticipationStatus = event.localParticipationStatus;
  if (event.pomodoroConfig) slim.pomodoroConfig = event.pomodoroConfig;
  if (event.rdate && event.rdate.length > 0) slim.rdate = event.rdate;
  if (event.overrides && event.overrides.length > 0) slim.overrides = event.overrides;
  return slim;
}

export function hydratePanelEvent(
  rows: CalendarPanelEventRows,
  renderZone: string,
): CalendarEvent | undefined {
  if (!rows.event) return undefined;
  const event = mapRow(rows.event, renderZone);
  applyFullEventFields(rows.event, event);
  if (rows.attendees.length > 0) event.attendees = rows.attendees.map(mapAttendee);
  return event;
}

export function hydrateFullEvent(
  rows: CalendarFullEventRows,
  renderZone: string,
): CalendarEvent | undefined {
  if (!rows.event) return undefined;
  const row = rows.event;
  const event = mapRow(row, renderZone);
  applyFullEventFields(row, event);

  if (rows.attendees.length > 0) {
    event.attendees = rows.attendees.map(mapAttendee);
  }
  if (rows.alarms.length > 0) {
    event.alarms = rows.alarms.map(mapAlarm);
  }
  if (rows.overrides.length > 0) {
    event.overrides = rows.overrides.map((overrideRow) => {
      const override = mapOverride(overrideRow, renderZone, row.all_day === 1);
      if (overrideRow.description) {
        override.description = sanitizeCalendarDescriptionHtml(overrideRow.description);
      }
      if (overrideRow.location) override.location = overrideRow.location;
      if (overrideRow.url) override.url = overrideRow.url;
      if (overrideRow.visibility) override.visibility = overrideRow.visibility as EventVisibility;
      const extendedProperties = parseJsonStringRecord(overrideRow.extended_properties);
      if (extendedProperties) override.extendedProperties = extendedProperties;
      if (overrideRow.icalendar_component_id) override.icalendarComponentId = overrideRow.icalendar_component_id;
      const rawJcal = safeJsonParse(overrideRow.icalendar_raw_jcal);
      if (rawJcal) override.icalendarRawJcal = rawJcal;
      return override;
    });
  }
  return event;
}
