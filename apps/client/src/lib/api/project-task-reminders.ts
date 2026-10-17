import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import {
  readArray,
  readEnum,
  readIdentifier,
  readNonNegativeSafeInteger,
  readNullable,
  readRecord,
  readString,
} from "$lib/utils/readers";

const PROJECT_TASK_REMINDER_ANCHORS = ["start", "due"] as const;
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;
const TIME_PATTERN = /^\d{2}:\d{2}$/;

/** Which task date a reminder fires before. */
export type ProjectTaskReminderAnchor = (typeof PROJECT_TASK_REMINDER_ANCHORS)[number];

/** One upcoming reminder before a task's start or due date, as floating wall values. */
export interface ProjectTaskReminder {
  taskId: string;
  projectId: string;
  projectName: string;
  title: string;
  anchor: ProjectTaskReminderAnchor;
  /** The `YYYY-MM-DD` date. */
  date: string;
  /** The `HH:MM` time, or null for a date without a time. */
  time: string | null;
  minutesBefore: number;
}

function readPattern(value: unknown, pattern: RegExp, label: string): string {
  const text = readString(value, label);
  if (!pattern.test(text)) throw new Error(`${label} is malformed`);
  return text;
}

/**
 * Validate one reminder row returned by the backend.
 *
 * @param value The untrusted row.
 * @param label The path used in validation errors.
 */
export function parseProjectTaskReminder(value: unknown, label: string): ProjectTaskReminder {
  const row = readRecord(value, label);
  return {
    taskId: readIdentifier(row.task_id, `${label}.task_id`),
    projectId: readIdentifier(row.project_id, `${label}.project_id`),
    projectName: readString(row.project_name, `${label}.project_name`),
    title: readString(row.title, `${label}.title`),
    anchor: readEnum(row.anchor, PROJECT_TASK_REMINDER_ANCHORS, `${label}.anchor`),
    date: readPattern(row.date, DATE_PATTERN, `${label}.date`),
    time: readNullable(row.time, `${label}.time`, (time, timeLabel) => readPattern(time, TIME_PATTERN, timeLabel)),
    minutesBefore: readNonNegativeSafeInteger(row.minutes_before, `${label}.minutes_before`),
  };
}

/**
 * List the reminders of open tasks whose date is on or after a day, earliest first.
 *
 * @param fromDate The first `YYYY-MM-DD` date to include.
 */
export async function listUpcomingProjectTaskReminders(fromDate: string): Promise<ProjectTaskReminder[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<unknown>("projects_list_upcoming_task_reminders", { dbUrl, fromDate });
  return readArray(rows, "projectTaskReminders", parseProjectTaskReminder);
}
