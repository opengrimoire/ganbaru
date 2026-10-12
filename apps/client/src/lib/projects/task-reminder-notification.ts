import type { ProjectTaskReminder } from "$lib/api/project-task-reminders";
import type { AppLocale } from "$lib/i18n/locales";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";

/** Localized wording of a task date reminder notification. */
export interface ProjectTaskReminderNotificationLabels {
  startTitle: (task: string) => string;
  dueTitle: (task: string) => string;
  untitledTask: string;
  body: (project: string, when: string) => string;
}

export interface ProjectTaskReminderNotificationText {
  title: string;
  body: string;
}

/** The task date as a wall value, read back in UTC so the device time zone cannot shift it. */
function wallDate(date: string, time: string | null): Date {
  const [year, month, day] = date.split("-").map(Number);
  const [hour, minute] = time === null ? [0, 0] : time.split(":").map(Number);
  return new Date(Date.UTC(year, month - 1, day, hour, minute));
}

/**
 * The title and body of a notification that a task date is coming up.
 *
 * @param reminder The reminder being delivered.
 * @param labels Localized wording.
 * @param locale The locale dates and times are formatted in.
 * @param timeFormat Whether a task time reads in 12 or 24 hours.
 */
export function projectTaskReminderNotificationText(
  reminder: ProjectTaskReminder,
  labels: ProjectTaskReminderNotificationLabels,
  locale: AppLocale,
  timeFormat: CalendarTimeFormat,
): ProjectTaskReminderNotificationText {
  const task = reminder.title.trim() || labels.untitledTask;
  const when = new Intl.DateTimeFormat(locale, {
    timeZone: "UTC",
    weekday: "short",
    month: "short",
    day: "numeric",
    ...(reminder.time === null
      ? {}
      : { hour: "numeric", minute: "2-digit", hourCycle: timeFormat === "12h" ? "h12" : "h23" }),
  }).format(wallDate(reminder.date, reminder.time));
  return {
    title: reminder.anchor === "start" ? labels.startTitle(task) : labels.dueTitle(task),
    body: labels.body(reminder.projectName, when),
  };
}
