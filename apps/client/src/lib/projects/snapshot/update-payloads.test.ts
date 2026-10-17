import { describe, expect, it } from "vitest";
import type { ProjectTask } from "$lib/projects/types";
import { taskUpdatePayload } from "./update-payloads";

function task(overrides: Partial<ProjectTask> = {}): ProjectTask {
  return {
    revision: 1,
    id: "task-a",
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title: "Ship",
    description: "",
    priority: "medium",
    taskType: "task",
    sectionSortOrder: 1000,
    statusSortOrder: 1000,
    dueDate: "2026-10-20",
    dueTime: "17:30",
    dueReminderMinutes: 15,
    startDate: "2026-10-19",
    startReminderMinutes: 1440,
    milestone: false,
    createdAt: "2026-10-11T00:00:00Z",
    updatedAt: "2026-10-11T00:00:00Z",
    detailLoaded: true,
    ...overrides,
  };
}

describe("taskUpdatePayload reminders", () => {
  it("keeps reminders that their dates still offer", () => {
    const payload = taskUpdatePayload(task(), { title: "Ship it" });
    expect(payload.dueReminderMinutes).toBe(15);
    expect(payload.startReminderMinutes).toBe(1440);
  });

  it("drops a reminder with its date", () => {
    const payload = taskUpdatePayload(task(), { dueDate: undefined, startDate: undefined });
    expect(payload.dueReminderMinutes).toBeNull();
    expect(payload.dueTime).toBeNull();
    expect(payload.startReminderMinutes).toBeNull();
  });

  it("moves a minute offset to the date itself when the time is cleared", () => {
    expect(taskUpdatePayload(task(), { dueTime: undefined }).dueReminderMinutes).toBe(0);
    expect(taskUpdatePayload(task({ dueReminderMinutes: 2880 }), { dueTime: undefined }).dueReminderMinutes).toBe(2880);
  });

  it("sends no reminder for a date that never had one", () => {
    expect(taskUpdatePayload(task({ dueReminderMinutes: undefined }), {}).dueReminderMinutes).toBeNull();
  });
});
