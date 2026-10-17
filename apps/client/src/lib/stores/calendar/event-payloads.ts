import type {
  AttendeeStatus,
  CalendarEvent,
  EventStatus,
  EventTransparency,
  EventVisibility,
  PomodoroConfig,
} from "$lib/calendar/types";
import { recurrenceToRrule } from "$lib/calendar/rrule";
import { sanitizeCalendarTime } from "$lib/calendar/utils";
import { sanitizeCalendarDescriptionHtml } from "$lib/calendar/description-sanitizer";
import { toDbTime } from "$lib/calendar/db-rows";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";

export type CalendarUpdateField =
  | { field: "title"; value: string }
  | { field: "startTime"; value: string }
  | { field: "endTime"; value: string }
  | { field: "timezone"; value: string }
  | { field: "calendarId"; value: string }
  | { field: "projectId"; value: string | null }
  | { field: "environmentId"; value: string | null }
  | { field: "playlistId"; value: string | null }
  | { field: "musicSnapshotAssignments"; value: MusicContextAssignmentDraft[] }
  | { field: "musicOverrideAssignments"; value: MusicContextAssignmentDraft[] }
  | { field: "color"; value: number | null }
  | { field: "description"; value: string }
  | { field: "rrule"; value: string | null }
  | { field: "repeatUntil"; value: string | null }
  | { field: "notifications"; value: string | null }
  | { field: "exceptions"; value: string | null }
  | { field: "allDay"; value: boolean }
  | { field: "meetingEnabled"; value: boolean }
  | { field: "location"; value: string }
  | { field: "url"; value: string }
  | { field: "transparency"; value: EventTransparency }
  | { field: "status"; value: EventStatus }
  | { field: "sourceUid"; value: string | null }
  | { field: "visibility"; value: EventVisibility }
  | { field: "priority"; value: number | null }
  | { field: "categories"; value: string | null }
  | { field: "geo"; value: string | null }
  | { field: "sequence"; value: number }
  | { field: "rdate"; value: string | null }
  | { field: "extendedProperties"; value: string | null }
  | { field: "organizer"; value: string | null }
  | { field: "localRsvpStatus"; value: AttendeeStatus | null }
  | {
      field: "guestPermissions";
      value: {
        guestCanModify: boolean;
        guestCanInviteOthers: boolean;
        guestCanSeeOtherGuests: boolean;
      };
    };

export type PomodoroConfigPatch =
  | { action: "set"; value: PomodoroConfig }
  | { action: "clear" };

export interface CalendarEventUpdatePayload {
  id: string;
  updatedAt: string;
  fields: CalendarUpdateField[];
  attendees: Array<{
    id: string;
    name: string | null;
    email: string;
    role: string;
    status: string;
    rsvp: boolean;
  }> | null;
  alarms: Array<{
    id: string;
    action: string;
    triggerType: string;
    triggerValue: string;
    description: string | null;
  }> | null;
  pomodoroConfig: PomodoroConfigPatch | null;
}

export function nowIso(): string {
  return new Date().toISOString();
}

export function localTimezone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

export function prepareEventUpdatePayload(
  patch: Partial<CalendarEvent> & { id: string },
  sourceEvents: readonly CalendarEvent[],
): { parentId: string; toUpdate: Partial<CalendarEvent> & { id: string }; payload: CalendarEventUpdatePayload } {
  const parentId = patch.recurringParentId ?? patch.id;
  let toUpdate: Partial<CalendarEvent> & { id: string };

  if (patch.recurringParentId) {
    const template = sourceEvents.find((event) => event.id === parentId);
    toUpdate = { ...patch, id: parentId };
    delete toUpdate.recurringParentId;
    if (template) {
      if (patch.start) {
        const templateStartDate = template.start.split(" ")[0];
        toUpdate.start = `${templateStartDate} ${String(patch.start).split(" ")[1]}`;
      }
      if (patch.end) {
        const templateEndDate = template.end.split(" ")[0];
        toUpdate.end = `${templateEndDate} ${String(patch.end).split(" ")[1]}`;
      }
    }
  } else {
    toUpdate = { ...patch };
    delete toUpdate.recurringParentId;
  }

  if (toUpdate.start !== undefined) {
    const sanitized = sanitizeCalendarTime(String(toUpdate.start));
    if (!sanitized) {
      throw new Error(`Invalid calendar time format: start="${toUpdate.start}"`);
    }
    toUpdate.start = sanitized;
  }
  if (toUpdate.end !== undefined) {
    const sanitized = sanitizeCalendarTime(String(toUpdate.end));
    if (!sanitized) {
      throw new Error(`Invalid calendar time format: end="${toUpdate.end}"`);
    }
    toUpdate.end = sanitized;
  }
  if ("description" in toUpdate) {
    toUpdate.description = sanitizeCalendarDescriptionHtml(toUpdate.description ?? "");
  }

  const existing = sourceEvents.find((event) => event.id === parentId);
  const homeZone = toUpdate.timezone ?? existing?.timezone ?? localTimezone();
  const allDayForDb = "allDay" in toUpdate ? !!toUpdate.allDay : !!existing?.allDay;
  const fields: CalendarUpdateField[] = [];
  const presentKeys = new Set(Object.keys(toUpdate) as Array<keyof CalendarEvent | "id">);
  const addField = (field: CalendarUpdateField) => {
    fields.push(field);
  };

  for (const key of presentKeys) {
    switch (key) {
      case "id":
      case "recurringParentId":
      case "pomodoroConfig":
      case "attendees":
      case "alarms":
      case "overrides":
        break;
      case "title":
        addField({ field: "title", value: toUpdate.title ?? "" });
        break;
      case "start":
        addField({
          field: "startTime",
          value: toDbTime(String(toUpdate.start), homeZone, allDayForDb),
        });
        break;
      case "end":
        addField({
          field: "endTime",
          value: toDbTime(String(toUpdate.end), homeZone, allDayForDb),
        });
        break;
      case "timezone":
        addField({ field: "timezone", value: toUpdate.timezone ?? "" });
        break;
      case "calendarId":
        addField({ field: "calendarId", value: toUpdate.calendarId ?? "local" });
        break;
      case "projectId":
        addField({ field: "projectId", value: toUpdate.projectId ?? null });
        break;
      case "environmentId":
        addField({ field: "environmentId", value: toUpdate.environmentId ?? null });
        break;
      case "playlistId":
        addField({ field: "playlistId", value: toUpdate.playlistId ?? null });
        break;
      case "musicSnapshotAssignments":
        addField({ field: "musicSnapshotAssignments", value: toUpdate.musicSnapshotAssignments ?? [] });
        break;
      case "musicOverrideAssignments":
        addField({ field: "musicOverrideAssignments", value: toUpdate.musicOverrideAssignments ?? [] });
        break;
      case "color":
        addField({ field: "color", value: toUpdate.color ?? null });
        break;
      case "description":
        addField({ field: "description", value: toUpdate.description ?? "" });
        break;
      case "recurrence": {
        const rrule = toUpdate.recurrence ? recurrenceToRrule(toUpdate.recurrence) : null;
        const repeatUntil = toUpdate.recurrence?.end.type === "until"
          ? toUpdate.recurrence.end.date
          : null;
        addField({ field: "rrule", value: rrule });
        addField({ field: "repeatUntil", value: repeatUntil });
        break;
      }
      case "notifications": {
        const notificationsJson = toUpdate.notifications && toUpdate.notifications.length > 0
          ? JSON.stringify(toUpdate.notifications)
          : null;
        addField({ field: "notifications", value: notificationsJson });
        break;
      }
      case "exceptions": {
        const exceptionsJson = toUpdate.exceptions && toUpdate.exceptions.length > 0
          ? JSON.stringify(toUpdate.exceptions)
          : null;
        addField({ field: "exceptions", value: exceptionsJson });
        break;
      }
      case "allDay":
        addField({ field: "allDay", value: !!toUpdate.allDay });
        break;
      case "meetingEnabled":
        addField({ field: "meetingEnabled", value: !!toUpdate.meetingEnabled });
        break;
      case "location":
        addField({ field: "location", value: toUpdate.location ?? "" });
        break;
      case "url":
        addField({ field: "url", value: toUpdate.url ?? "" });
        break;
      case "transparency":
        addField({ field: "transparency", value: toUpdate.transparency ?? "opaque" });
        break;
      case "status":
        addField({ field: "status", value: toUpdate.status ?? "confirmed" });
        break;
      case "sourceUid":
        addField({ field: "sourceUid", value: toUpdate.sourceUid ?? null });
        break;
      case "visibility":
        addField({ field: "visibility", value: toUpdate.visibility ?? "public" });
        break;
      case "priority":
        addField({ field: "priority", value: toUpdate.priority ?? null });
        break;
      case "categories":
        addField({
          field: "categories",
          value: toUpdate.categories ? JSON.stringify(toUpdate.categories) : null,
        });
        break;
      case "geo":
        addField({
          field: "geo",
          value: toUpdate.geo ? JSON.stringify(toUpdate.geo) : null,
        });
        break;
      case "sequence":
        addField({ field: "sequence", value: toUpdate.sequence ?? 0 });
        break;
      case "rdate":
        addField({
          field: "rdate",
          value: toUpdate.rdate ? JSON.stringify(toUpdate.rdate) : null,
        });
        break;
      case "extendedProperties":
        addField({
          field: "extendedProperties",
          value: toUpdate.extendedProperties ? JSON.stringify(toUpdate.extendedProperties) : null,
        });
        break;
      case "organizer":
        addField({
          field: "organizer",
          value: toUpdate.organizer ? JSON.stringify(toUpdate.organizer) : null,
        });
        break;
      case "localParticipationStatus":
        addField({
          field: "localRsvpStatus",
          value: toUpdate.localParticipationStatus ?? null,
        });
        break;
      case "guestPermissions": {
        const guestPermissions = toUpdate.guestPermissions;
        addField({
          field: "guestPermissions",
          value: {
            guestCanModify: guestPermissions?.canModify ?? false,
            guestCanInviteOthers: guestPermissions?.canInviteOthers ?? true,
            guestCanSeeOtherGuests: guestPermissions?.canSeeOtherGuests ?? true,
          },
        });
        break;
      }
      case "hasCallLink":
      case "surfaceStatus":
      case "surfaceAttendees":
      case "linkedTaskIds":
      case "icalendarComponentId":
      case "icalendarPreservationStatus":
      case "icalendarProjectionWarnings":
      case "icalendarRawJcal":
        break;
    }
  }

  const pomodoroConfig: PomodoroConfigPatch | null = allDayForDb && presentKeys.has("allDay")
    ? { action: "clear" }
    : presentKeys.has("pomodoroConfig")
    ? toUpdate.pomodoroConfig
      ? { action: "set", value: toUpdate.pomodoroConfig }
      : { action: "clear" }
    : null;

  return {
    parentId,
    toUpdate,
    payload: {
      id: parentId,
      updatedAt: nowIso(),
      fields,
      attendees: presentKeys.has("attendees")
        ? (toUpdate.attendees ?? []).map((attendee) => ({
            id: attendee.id,
            name: attendee.name ?? null,
            email: attendee.email,
            role: attendee.role,
            status: attendee.status,
            rsvp: attendee.rsvp,
          }))
        : null,
      alarms: presentKeys.has("alarms")
        ? (toUpdate.alarms ?? []).map((alarm) => ({
            id: alarm.id,
            action: alarm.action,
            triggerType: alarm.triggerType,
            triggerValue: alarm.triggerValue,
            description: alarm.description ?? null,
          }))
        : null,
      pomodoroConfig,
    },
  };
}
