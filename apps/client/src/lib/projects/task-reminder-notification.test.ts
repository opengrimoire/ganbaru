import { describe, expect, it } from "vitest";
import type { ProjectTaskReminder } from "$lib/api/project-task-reminders";
import { projectTaskReminderNotificationText, type ProjectTaskReminderNotificationLabels } from "./task-reminder-notification";

const labels: ProjectTaskReminderNotificationLabels = {
  startTitle: (task) => `Starts: ${task}`,
  dueTitle: (task) => `Due: ${task}`,
  untitledTask: "Untitled task",
  body: (project, when) => `${project}, ${when}`,
};

function reminder(overrides: Partial<ProjectTaskReminder> = {}): ProjectTaskReminder {
  return {
    taskId: "task-a",
    projectId: "project-a",
    projectName: "Launch",
    title: "Ship",
    anchor: "due",
    date: "2026-10-20",
    time: null,
    minutesBefore: 0,
    ...overrides,
  };
}

describe("projectTaskReminderNotificationText", () => {
  it("names the anchor and the task, and the project and date without a time", () => {
    expect(projectTaskReminderNotificationText(reminder(), labels, "en", "24h")).toEqual({
      title: "Due: Ship",
      body: "Launch, Tue, Oct 20",
    });
    expect(projectTaskReminderNotificationText(reminder({ anchor: "start" }), labels, "en", "24h").title)
      .toBe("Starts: Ship");
  });

  it("adds the wall time in the chosen hour cycle without shifting the date", () => {
    const late = reminder({ date: "2026-10-20", time: "23:30" });
    expect(projectTaskReminderNotificationText(late, labels, "en", "24h").body).toBe("Launch, Tue, Oct 20, 23:30");
    expect(projectTaskReminderNotificationText(late, labels, "en", "12h").body).toBe("Launch, Tue, Oct 20, 11:30 PM");
  });

  it("falls back to a placeholder for a blank title", () => {
    expect(projectTaskReminderNotificationText(reminder({ title: "  " }), labels, "en", "24h").title)
      .toBe("Due: Untitled task");
  });
});
