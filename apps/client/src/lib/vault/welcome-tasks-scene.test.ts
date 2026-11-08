import { describe, expect, it } from "vitest";
import { FILM_TAP_MS } from "./welcome-film";
import { PROJECTS_PRIORITY_ORDER, PROJECTS_STATUS_ORDER } from "./welcome-projects-scene";
import {
  TASKS_BEATS,
  TASKS_NEW_PRIORITY,
  TASKS_NEW_STATUS,
  TASKS_NEW_TASK,
  TASKS_STILL,
  tasksAddActive,
  tasksAdded,
  tasksCaretOn,
  tasksCellHighlighted,
  tasksCellMenu,
  tasksDraft,
  tasksListScroll,
  tasksNewPriority,
  tasksNewStatus,
  tasksOptionHovered,
  tasksTouches,
} from "./welcome-tasks-scene";

const B = TASKS_BEATS;
const TITLE = "Draft chapter 2";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome tasks scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const scroll = tasksListScroll(elapsed);
      for (const value of [scroll.status, scroll.priority, ...Object.values(tasksTouches(elapsed))]) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("picks a status and a priority the menus list, other than the ones a new task starts with", () => {
    expect(PROJECTS_STATUS_ORDER).toContain(TASKS_NEW_STATUS);
    expect(PROJECTS_PRIORITY_ORDER).toContain(TASKS_NEW_PRIORITY);
    expect(TASKS_NEW_TASK.status).not.toBe(TASKS_NEW_STATUS);
    expect(TASKS_NEW_TASK.priority).not.toBe(TASKS_NEW_PRIORITY);
  });

  it("types the task into the add row, adds it with Enter, and keeps the field open and empty", () => {
    expect(tasksAddActive(B.addTap - 1)).toBe(false);
    expect(tasksAddActive(B.addTap)).toBe(true);
    expect(tasksDraft(B.taskTypeStart, TITLE)).toBe("");
    expect(tasksDraft((B.taskTypeStart + B.taskTypeEnd) / 2, TITLE).length).toBeGreaterThan(0);
    expect(tasksDraft(B.taskTypeEnd, TITLE)).toBe(TITLE);
    expect(tasksAdded(B.taskSubmit - 1)).toBe(false);
    expect(tasksDraft(B.taskSubmit - 1, TITLE)).toBe(TITLE);
    expect(tasksAdded(B.taskSubmit)).toBe(true);
    expect(tasksDraft(B.taskSubmit, TITLE)).toBe("");
    expect(tasksAddActive(B.taskSubmit)).toBe(true);
    expect(tasksCaretOn((B.taskTypeStart + B.taskTypeEnd) / 2)).toBe(true);
  });

  it("closes the add field with the tap on the new task's status", () => {
    expect(tasksAddActive(B.statusTap - 1)).toBe(true);
    expect(tasksAddActive(B.statusTap)).toBe(false);
    expect(tasksCaretOn(B.statusTap)).toBe(false);
  });

  it("adds the task as a new task starts, then sets its status and its priority from their menus", () => {
    expect(tasksNewStatus(B.statusPick - 1)).toBe(TASKS_NEW_TASK.status);
    expect(tasksNewStatus(B.statusPick)).toBe(TASKS_NEW_STATUS);
    expect(tasksNewPriority(B.priorityPick - 1)).toBe(TASKS_NEW_TASK.priority);
    expect(tasksNewPriority(B.priorityPick)).toBe(TASKS_NEW_PRIORITY);
  });

  it("opens one cell menu at a time, highlighting its cell from the hover before the tap", () => {
    expect(tasksCellMenu(B.statusTap - 1)).toBeNull();
    expect(tasksCellHighlighted(B.statusTap - 1, "status")).toBe(true);
    expect(tasksCellMenu(B.statusTap)).toBe("status");
    expect(tasksOptionHovered(B.statusPick - 1, "status")).toBe(true);
    expect(tasksCellMenu(B.statusPick)).toBeNull();
    expect(tasksCellHighlighted(B.statusPick, "status")).toBe(false);
    expect(tasksCellHighlighted(B.statusTap, "priority")).toBe(false);
    expect(tasksCellMenu(B.priorityTap)).toBe("priority");
    expect(tasksCellMenu(B.priorityPick)).toBeNull();
    expect(tasksTouches(B.statusTap - FILM_TAP_MS / 2).status).toBeGreaterThan(0);
  });

  it("scrolls the list to each cell before its tap", () => {
    expect(tasksListScroll(B.statusScrollStart)).toEqual({ status: 0, priority: 0 });
    expect(tasksListScroll(B.statusTap)).toEqual({ status: 1, priority: 0 });
    expect(tasksListScroll(B.priorityTap)).toEqual({ status: 1, priority: 1 });
  });

  it("shows the added task with its new status and priority, every menu closed, in its still frame", () => {
    expect(tasksAdded(TASKS_STILL)).toBe(true);
    expect(tasksNewStatus(TASKS_STILL)).toBe(TASKS_NEW_STATUS);
    expect(tasksNewPriority(TASKS_STILL)).toBe(TASKS_NEW_PRIORITY);
    expect(tasksCellMenu(TASKS_STILL)).toBeNull();
    expect(Object.values(tasksTouches(TASKS_STILL)).every((touch) => touch === 0)).toBe(true);
  });
});
