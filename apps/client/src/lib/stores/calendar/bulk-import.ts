/**
 * Pure helpers for calendar bulk import payloads. ICS parsing runs in
 * TypeScript, and the Rust `calendar_bulk_import` command applies the
 * payload to the database.
 */
import type {
  CalendarEvent,
  EventAlarm,
  EventAttendee,
  EventOverride,
} from "$lib/calendar/types";
import type {
  IcsPreservedComponent,
  IcsPreservedObject,
  IcsPreservationPayload,
  IcsPreservationStatus,
} from "$lib/calendar/ics/types";
import { recurrenceToRrule } from "$lib/calendar/rrule";
import { sanitizeCalendarDescriptionHtml } from "$lib/calendar/description-sanitizer";
import { hasMeetingState } from "$lib/calendar/meeting-state";
import { toDbTime } from "$lib/calendar/db-rows";

export type CalendarImportSourceKind =
  | "import-file"
  | "import-zip-entry"
  | "local-export-base"
  | "subscription";

export type CalendarBulkImportAction = "added" | "updated";

export interface CalendarBulkImportApplied {
  candidateId: string;
  eventId: string;
  action: CalendarBulkImportAction;
}

export interface CalendarBulkImportResult {
  added: number;
  updated: number;
  skippedOlder: number;
  warnings: string[];
  applied: CalendarBulkImportApplied[];
}

export interface CalendarBulkImportPayload {
  targetCalendarId: string;
  now: string;
  preservation: CalendarBulkImportPreservation | null;
  events: CalendarBulkImportEvent[];
}

export interface CalendarBulkImportPreservation {
  sourceKind: CalendarImportSourceKind;
  sourceName: string;
  sourceFingerprint: string;
  objects: CalendarBulkImportPreservedObject[];
}

export interface CalendarBulkImportPreservedObject {
  id: string;
  prodid: string | null;
  version: string | null;
  method: string | null;
  calendarScale: string | null;
  rawJcal: string;
  diagnostics: string;
  components: CalendarBulkImportPreservedComponent[];
}

export interface CalendarBulkImportPreservedComponent {
  id: string;
  componentType: string;
  uid: string | null;
  recurrenceId: string | null;
  recurrenceIdValueType: string | null;
  sequence: number | null;
  dtstartKey: string | null;
  rawJcal: string;
  preservationStatus: IcsPreservationStatus;
  projectionWarnings: string;
  components: CalendarBulkImportPreservedComponent[];
}

export interface CalendarBulkImportEvent {
  candidateId: string;
  icalendarComponentId: string | null;
  title: string;
  startTime: string;
  endTime: string;
  timezone: string;
  color: number | null;
  description: string;
  rrule: string | null;
  notifications: string | null;
  exceptions: string | null;
  repeatUntil: string | null;
  allDay: boolean;
  location: string;
  url: string;
  transparency: string;
  status: string;
  sourceUid: string | null;
  visibility: string;
  priority: number | null;
  categories: string | null;
  geo: string | null;
  sequence: number;
  rdate: string | null;
  extendedProperties: string | null;
  organizer: string | null;
  meetingEnabled: boolean;
  guestCanModify: boolean;
  guestCanInviteOthers: boolean;
  guestCanSeeOtherGuests: boolean;
  attendees: CalendarBulkImportAttendee[];
  alarms: CalendarBulkImportAlarm[];
  overrides: CalendarBulkImportOverride[];
}

export interface CalendarBulkImportAttendee {
  id: string;
  icalendarComponentId: string | null;
  icalendarPropertyIndex: number | null;
  name: string | null;
  email: string;
  role: string;
  status: string;
  rsvp: boolean;
}

export interface CalendarBulkImportAlarm {
  id: string;
  icalendarComponentId: string | null;
  action: string;
  triggerType: string;
  triggerValue: string;
  description: string | null;
}

export interface CalendarBulkImportOverride {
  id: string;
  icalendarComponentId: string | null;
  recurrenceId: string;
  recurrenceRange: "this-and-future" | null;
  title: string | null;
  startTime: string | null;
  endTime: string | null;
  description: string | null;
  location: string | null;
  url: string | null;
  color: number | null;
  status: string | null;
  transparency: string | null;
  visibility: string | null;
  extendedProperties: string | null;
}

interface CalendarBulkImportBuildPreservation {
  payload: CalendarBulkImportPreservation;
  links: PreservationLinkIndex;
}

interface PreservationLinkIndex {
  eventsByUid: Map<string, PreservationEventLink>;
  overridesByUid: Map<string, PreservationEventLink[]>;
}

interface PreservationEventLink {
  componentId: string;
  sequence: number | null;
  attendeePropertyIndexes: number[];
  alarmComponentIds: string[];
}

interface BuiltPreservedComponent {
  component: CalendarBulkImportPreservedComponent;
  link: PreservationEventLink | null;
}

export function buildBulkImportPayload(
  events: CalendarEvent[],
  targetCalendarId: string,
  now: string,
  fallbackZone: string,
  newId: () => string,
  preservation: IcsPreservationPayload | null = null,
  sourceName = "",
  sourceKind: CalendarImportSourceKind = "import-file",
): CalendarBulkImportPayload {
  const builtPreservation = buildPreservationPayload(
    preservation,
    sourceName,
    sourceKind,
    newId,
  );
  return {
    targetCalendarId,
    now,
    preservation: builtPreservation?.payload ?? null,
    events: events.map((event) => {
      const link = event.sourceUid
        ? builtPreservation?.links.eventsByUid.get(event.sourceUid) ?? null
        : null;
      const overrideLinks = event.sourceUid
        ? builtPreservation?.links.overridesByUid.get(event.sourceUid) ?? []
        : [];
      return buildBulkImportEvent(
        event,
        newId(),
        fallbackZone,
        link,
        [...overrideLinks],
      );
    }),
  };
}

function buildPreservationPayload(
  preservation: IcsPreservationPayload | null,
  sourceName: string,
  sourceKind: CalendarImportSourceKind,
  newId: () => string,
): CalendarBulkImportBuildPreservation | null {
  if (!preservation || preservation.objects.length === 0) return null;
  const links: PreservationLinkIndex = {
    eventsByUid: new Map(),
    overridesByUid: new Map(),
  };
  return {
    payload: {
      sourceKind,
      sourceName,
      sourceFingerprint: preservation.sourceFingerprint,
      objects: preservation.objects.map((object) =>
        buildPreservedObject(object, newId, links),
      ),
    },
    links,
  };
}

function buildPreservedObject(
  object: IcsPreservedObject,
  newId: () => string,
  links: PreservationLinkIndex,
): CalendarBulkImportPreservedObject {
  return {
    id: newId(),
    prodid: object.prodid ?? null,
    version: object.version ?? null,
    method: object.method ?? null,
    calendarScale: object.calendarScale ?? null,
    rawJcal: JSON.stringify(object.rawJcal),
    diagnostics: JSON.stringify(object.diagnostics),
    components: object.components.map((component) =>
      buildPreservedComponent(component, newId, links).component,
    ),
  };
}

function buildPreservedComponent(
  component: IcsPreservedComponent,
  newId: () => string,
  links: PreservationLinkIndex,
): BuiltPreservedComponent {
  const id = newId();
  const children = component.components.map((child) =>
    buildPreservedComponent(child, newId, links),
  );
  const built: CalendarBulkImportPreservedComponent = {
    id,
    componentType: component.componentType,
    uid: component.uid ?? null,
    recurrenceId: component.recurrenceId ?? null,
    recurrenceIdValueType: component.recurrenceIdValueType ?? null,
    sequence: component.sequence ?? null,
    dtstartKey: component.dtstartKey ?? null,
    rawJcal: JSON.stringify(component.rawJcal),
    preservationStatus: component.preservationStatus,
    projectionWarnings: JSON.stringify(component.projectionWarnings),
    components: children.map((child) => child.component),
  };

  const link = buildComponentProjectionLink(component, built, children);
  if (component.componentType === "vevent" && component.uid && link) {
    if (component.recurrenceId) {
      const uidLinks = links.overridesByUid.get(component.uid) ?? [];
      uidLinks.push(link);
      links.overridesByUid.set(component.uid, uidLinks);
    } else if (isPreferredEventLink(link, links.eventsByUid.get(component.uid))) {
      links.eventsByUid.set(component.uid, link);
    }
  }
  return { component: built, link };
}

function buildComponentProjectionLink(
  source: IcsPreservedComponent,
  built: CalendarBulkImportPreservedComponent,
  children: BuiltPreservedComponent[],
): PreservationEventLink | null {
  if (source.componentType !== "vevent") return null;
  return {
    componentId: built.id,
    sequence: source.sequence ?? null,
    attendeePropertyIndexes: attendeePropertyIndexes(source.rawJcal),
    alarmComponentIds: children
      .map((child) => child.component)
      .filter((child) => child.componentType === "valarm")
      .map((child) => child.id),
  };
}

function isPreferredEventLink(
  candidate: PreservationEventLink,
  existing: PreservationEventLink | undefined,
): boolean {
  if (!existing) return true;
  return (candidate.sequence ?? 0) > (existing.sequence ?? 0);
}

function attendeePropertyIndexes(rawJcal: unknown): number[] {
  if (!Array.isArray(rawJcal) || !Array.isArray(rawJcal[1])) return [];
  return rawJcal[1].flatMap((property, index) => {
    if (!Array.isArray(property) || typeof property[0] !== "string") return [];
    return property[0].toLowerCase() === "attendee" ? [index] : [];
  });
}

function buildBulkImportEvent(
  event: CalendarEvent,
  candidateId: string,
  fallbackZone: string,
  link: PreservationEventLink | null,
  overrideLinks: PreservationEventLink[],
): CalendarBulkImportEvent {
  const homeZone = event.timezone || fallbackZone;
  const guestPermissions = event.guestPermissions;
  const meetingEnabled = event.meetingEnabled ?? hasMeetingState(event);
  return {
    candidateId,
    icalendarComponentId: link?.componentId ?? event.icalendarComponentId ?? null,
    title: event.title,
    startTime: toDbTime(event.start, fallbackZone, event.allDay),
    endTime: toDbTime(event.end, fallbackZone, event.allDay),
    timezone: homeZone,
    color: event.color ?? null,
    description: sanitizeCalendarDescriptionHtml(event.description ?? ""),
    rrule: event.recurrence ? recurrenceToRrule(event.recurrence) : null,
    notifications: jsonOrNull(event.notifications),
    exceptions: jsonOrNull(event.exceptions),
    repeatUntil: event.recurrence?.end.type === "until"
      ? event.recurrence.end.date
      : null,
    allDay: event.allDay ?? false,
    location: event.location ?? "",
    url: event.url ?? "",
    transparency: event.transparency ?? "opaque",
    status: event.status ?? "confirmed",
    sourceUid: event.sourceUid ?? null,
    visibility: event.visibility ?? "public",
    priority: event.priority ?? null,
    categories: jsonOrNull(event.categories),
    geo: jsonOrNull(event.geo),
    sequence: event.sequence ?? 0,
    rdate: jsonOrNull(event.rdate),
    extendedProperties: jsonOrNull(event.extendedProperties),
    organizer: jsonOrNull(event.organizer),
    meetingEnabled,
    guestCanModify: guestPermissions?.canModify ?? false,
    guestCanInviteOthers: guestPermissions?.canInviteOthers ?? true,
    guestCanSeeOtherGuests: guestPermissions?.canSeeOtherGuests ?? true,
    attendees: buildAttendees(event.attendees, link),
    alarms: buildAlarms(event.alarms, link),
    overrides: buildOverrides(
      event.overrides,
      fallbackZone,
      event.allDay === true,
      overrideLinks,
    ),
  };
}

function buildAttendees(
  attendees: CalendarEvent["attendees"],
  link: PreservationEventLink | null,
): CalendarBulkImportAttendee[] {
  return (attendees ?? []).map((attendee: EventAttendee, index) => ({
    id: attendee.id,
    icalendarComponentId: link?.componentId ?? attendee.icalendarComponentId ?? null,
    icalendarPropertyIndex:
      link?.attendeePropertyIndexes[index] ?? attendee.icalendarPropertyIndex ?? null,
    name: attendee.name ?? null,
    email: attendee.email,
    role: attendee.role,
    status: attendee.status,
    rsvp: attendee.rsvp,
  }));
}

function buildAlarms(
  alarms: CalendarEvent["alarms"],
  link: PreservationEventLink | null,
): CalendarBulkImportAlarm[] {
  return (alarms ?? []).map((alarm: EventAlarm, index) => ({
    id: alarm.id,
    icalendarComponentId: link?.alarmComponentIds[index] ?? alarm.icalendarComponentId ?? null,
    action: alarm.action,
    triggerType: alarm.triggerType,
    triggerValue: alarm.triggerValue,
    description: alarm.description ?? null,
  }));
}

function buildOverrides(
  overrides: CalendarEvent["overrides"],
  zone: string,
  allDay: boolean,
  overrideLinks: PreservationEventLink[],
): CalendarBulkImportOverride[] {
  return (overrides ?? []).map((overrideRow: EventOverride, index) => ({
    id: overrideRow.id,
    icalendarComponentId:
      overrideLinks[index]?.componentId ?? overrideRow.icalendarComponentId ?? null,
    recurrenceId: overrideRow.recurrenceId,
    recurrenceRange: overrideRow.recurrenceRange ?? null,
    title: overrideRow.title ?? null,
    startTime: overrideRow.start
      ? toDbTime(overrideRow.start, zone, allDay)
      : null,
    endTime: overrideRow.end
      ? toDbTime(overrideRow.end, zone, allDay)
      : null,
    description: overrideRow.description === undefined || overrideRow.description === null
      ? null
      : sanitizeCalendarDescriptionHtml(overrideRow.description),
    location: overrideRow.location ?? null,
    url: overrideRow.url ?? null,
    color: overrideRow.color ?? null,
    status: overrideRow.status ?? null,
    transparency: overrideRow.transparency ?? null,
    visibility: overrideRow.visibility ?? null,
    extendedProperties: jsonOrNull(overrideRow.extendedProperties),
  }));
}

function jsonOrNull(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  if (Array.isArray(value) && value.length === 0) return null;
  if (typeof value === "object" && Object.keys(value).length === 0) return null;
  return JSON.stringify(value);
}
