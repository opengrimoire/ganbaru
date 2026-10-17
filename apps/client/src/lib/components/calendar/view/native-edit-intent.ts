import type { CalendarCreateIntent, CalendarEditIntent } from "$lib/api/calendar-edit";
import { prepareEventUpdatePayload, type CalendarUpdateField } from "$lib/stores/calendar/event-payloads";
import { fieldEqual, type EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import { recurrenceConfigsEqual, recurrenceToRrule } from "$lib/calendar/rrule";
import type { CalendarEvent, RecurringScope } from "$lib/calendar/types";
import { PENDING_CREATE_ID } from "$lib/components/calendar/display-events";

type EditState = Extract<EditSessionState, { mode: "edit" }>;
type EditableField = NonNullable<CalendarEditIntent["draft"]["fields"]>[number];

function isEditableField(field: CalendarUpdateField): field is EditableField {
  switch (field.field) {
    case "startTime": case "endTime": case "timezone": case "allDay": case "rrule":
    case "repeatUntil": case "exceptions": case "rdate": case "sourceUid": case "sequence":
      return false;
    default: return true;
  }
}

/** Serialize authored creation values without resolving time or expanding a series. */
export function buildNativeCalendarCreate(input: {
  start: string;
  end: string;
  changes: Partial<CalendarEvent>;
  renderZone: string;
}): CalendarCreateIntent {
  const { changes, renderZone } = input;
  const allDay = changes.allDay === true;
  const endpoint = (value: string) => allDay ? value.slice(0, 10) : value.replace(" ", "T");
  const { start: _start, end: _end, timezone: _timezone, recurrence: _recurrence, ...metadata } = changes;
  const { payload } = prepareEventUpdatePayload({ ...metadata, id: PENDING_CREATE_ID }, []);
  return { kind: "create", draft: {
    timing: { startTime: endpoint(changes.start ?? input.start), endTime: endpoint(changes.end ?? input.end),
      timezone: changes.timezone ?? renderZone, inputZone: renderZone, allDay },
    recurrence: changes.recurrence ? { kind: "set", value: recurrenceToRrule(changes.recurrence) } : { kind: "clear" },
    fields: payload.fields.filter(isEditableField), attendees: payload.attendees, alarms: payload.alarms,
    pomodoroConfig: payload.pomodoroConfig,
  } };
}

/** Serialize actual editor changes; scope decisions and civil-time resolution remain native. */
export function buildNativeCalendarEdit(input: {
  state: Pick<EditState, "instanceEvent" | "templateId">;
  baseline: Partial<CalendarEvent>;
  changes: Partial<CalendarEvent>;
  scope: RecurringScope;
  renderZone: string;
  action?: CalendarEditIntent["action"];
}): CalendarEditIntent {
  const { state, changes } = input;
  const selected = state.instanceEvent;
  if (!selected.recurrenceDate) throw new Error("Calendar selection has no native recurrence identity; refresh the window");
  const baseline = { ...selected, ...input.baseline };
  const metadata: Partial<CalendarEvent> = {};
  const present = (key: keyof CalendarEvent) => Object.prototype.hasOwnProperty.call(changes, key);
  for (const key of Object.keys(changes) as (keyof CalendarEvent)[]) {
    if (["start", "end", "timezone", "allDay", "recurrence", "exceptions", "rdate", "sourceUid", "sequence"].includes(key)) continue;
    if (!fieldEqual(changes[key], baseline[key])) {
      Object.assign(metadata, { [key]: changes[key] });
    }
  }
  const allDay = present("allDay") ? changes.allDay === true : selected.allDay === true;
  const changedDateKind = allDay !== (selected.allDay === true);
  const timing: NonNullable<CalendarEditIntent["draft"]["timing"]> = {};
  if (changedDateKind) timing.allDay = allDay;
  const endpoint = (value: string) => allDay ? value.slice(0, 10) : value.replace(" ", "T");
  // Compare geometry with the native selection, so a pre-mount drag cannot be
  // lost when the panel normalizes its own initial baseline.
  if (changedDateKind || (present("start") && changes.start !== selected.start)) {
    timing.startTime = endpoint(changes.start ?? selected.start);
  }
  if (input.action !== "end_now" && (changedDateKind || (present("end") && changes.end !== selected.end))) {
    timing.endTime = endpoint(changes.end ?? selected.end);
  }
  if (timing.startTime !== undefined || timing.endTime !== undefined) timing.inputZone = input.renderZone;
  if (present("timezone") && changes.timezone !== selected.timezone) timing.timezone = changes.timezone;
  const recurrence: CalendarEditIntent["draft"]["recurrence"] = present("recurrence")
    && !recurrenceConfigsEqual(changes.recurrence, baseline.recurrence)
    ? changes.recurrence ? { kind: "set", value: recurrenceToRrule(changes.recurrence) } : { kind: "clear" }
    : { kind: "unchanged" };
  const { payload } = prepareEventUpdatePayload({ ...metadata, id: state.templateId }, []);
  return {
    ...(input.action && input.action !== "save" ? { action: input.action } : {}),
    selection: { templateId: state.templateId, recurrenceDate: selected.recurrenceDate,
      scope: input.action && input.action !== "save" ? "this" : input.scope },
    draft: { timing, recurrence, fields: payload.fields.filter(isEditableField),
      attendees: payload.attendees, alarms: payload.alarms, pomodoroConfig: payload.pomodoroConfig },
  };
}
