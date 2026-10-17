import { Temporal } from "@js-temporal/polyfill";
import type { CalendarEvent } from "$lib/calendar/types";
import {
  projectDefaultIdleTimeoutMinutes,
  projectDefaultPomodoroConfig,
  type ProjectGlobalIdleDefaults,
} from "$lib/projects/default-pomodoro";
import type { Project } from "$lib/projects/types";
import { effectiveProjectDefaultEventName } from "$lib/projects/system-defaults";

export interface ProjectScheduleStart {
  date: string;
  time: string;
}

export interface ProjectScheduleWindow {
  start: string;
  end: string;
}

export interface ProjectCalendarCreateDefaultsInput {
  project: Project | null | undefined;
  start: string;
  end: string;
  allDay?: boolean;
  globalIdleDefaults: ProjectGlobalIdleDefaults;
}

/**
 * Returns the canonical event id used when matching generated recurrence instances.
 */
export function projectCalendarEventRootId(event: Pick<CalendarEvent, "id" | "recurringParentId">): string {
  return event.recurringParentId ?? event.id.split("::")[0] ?? event.id;
}

export function projectDefaultScheduleStart(
  now: Temporal.PlainDateTime = Temporal.Now.plainDateTimeISO(),
  stepMinutes = 15,
): ProjectScheduleStart {
  const safeStep = Number.isFinite(stepMinutes) && stepMinutes > 0 ? Math.round(stepMinutes) : 15;
  const dayStart = now.with({
    hour: 0,
    minute: 0,
    second: 0,
    millisecond: 0,
    microsecond: 0,
    nanosecond: 0,
  });
  const totalMinutes = now.hour * 60 + now.minute;
  const roundedMinutes = Math.ceil(totalMinutes / safeStep) * safeStep;
  const start = dayStart.add({ minutes: roundedMinutes });
  return {
    date: start.toPlainDate().toString(),
    time: projectScheduleTimeLabel(start),
  };
}

export function projectScheduleTimeLabel(value: Temporal.PlainDateTime): string {
  return `${String(value.hour).padStart(2, "0")}:${String(value.minute).padStart(2, "0")}`;
}

export function formatProjectScheduleWindowStart(
  start: Temporal.PlainDateTime,
  durationMinutes: number,
): ProjectScheduleWindow {
  const end = start.add({ minutes: durationMinutes });
  return {
    start: `${start.toPlainDate().toString()} ${projectScheduleTimeLabel(start)}`,
    end: `${end.toPlainDate().toString()} ${projectScheduleTimeLabel(end)}`,
  };
}

export function projectScheduleWindowFor(
  date: string,
  startTime: string,
  durationMinutes: number,
): ProjectScheduleWindow | null {
  const duration = Math.round(Number(durationMinutes));
  if (duration <= 0 || !date || !startTime) return null;
  try {
    return formatProjectScheduleWindowStart(Temporal.PlainDateTime.from(`${date}T${startTime}`), duration);
  } catch {
    return null;
  }
}

/**
 * Builds calendar event defaults from the selected project's scheduling settings.
 */
export function projectCalendarCreateDefaults(
  input: ProjectCalendarCreateDefaultsInput,
): Partial<CalendarEvent> {
  const project = input.project;
  if (!project) return {};
  const usesProjectAllDayDefault = !input.allDay && project.defaultEventTimeMode === "all_day";
  const allDay = input.allDay || usesProjectAllDayDefault;
  let start = input.start;
  let end = input.end;
  if (usesProjectAllDayDefault) {
    const startDate = input.start.split(" ")[0] ?? "";
    if (startDate) {
      start = `${startDate} 00:00`;
      end = `${startDate} 00:00`;
    }
  } else if (!allDay && project.defaultEventDurationMinutes !== null) {
    const startDate = start.split(" ")[0] ?? "";
    const startTime = input.start.split(" ")[1] ?? "";
    const nextWindow = projectScheduleWindowFor(
      startDate,
      startTime,
      project.defaultEventDurationMinutes,
    );
    if (nextWindow) end = nextWindow.end;
  }
  return {
    title: effectiveProjectDefaultEventName(project) ?? "",
    start,
    end,
    allDay: allDay || undefined,
    projectId: project.id,
    color: project.color,
    environmentId: project.workEnvironmentId,
    playlistId: project.focusPlaylistId,
    pomodoroConfig: allDay
      ? undefined
      : projectDefaultPomodoroConfig(
        project,
        projectDefaultIdleTimeoutMinutes(project, input.globalIdleDefaults),
      ),
  };
}

export function projectEventDurationMinutes(event: Pick<CalendarEvent, "start" | "end">): number {
  try {
    const start = Temporal.PlainDateTime.from(event.start.replace(" ", "T"));
    const end = Temporal.PlainDateTime.from(event.end.replace(" ", "T"));
    return Math.max(0, Math.round(start.until(end).total({ unit: "minutes" })));
  } catch {
    return 0;
  }
}

export function projectEventDurationMinutesInDateRange(
  events: readonly Pick<CalendarEvent, "start" | "end">[],
  startDate: string,
  endDate: string,
): number {
  return events
    .filter((event) => {
      const eventDate = event.start.slice(0, 10);
      return eventDate >= startDate && eventDate <= endDate;
    })
    .reduce((total, event) => total + projectEventDurationMinutes(event), 0);
}
