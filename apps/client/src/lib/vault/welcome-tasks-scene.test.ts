import { describe, expect, it } from "vitest";
import { welcomeCursorFrame, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";
import { PROJECTS_PRIORITY_ORDER, PROJECTS_STATUS_ORDER } from "./welcome-projects-scene";
import {
  TASKS_BEATS,
  TASKS_CURSOR,
  TASKS_NEW_PRIORITY,
  TASKS_NEW_STATUS,
  TASKS_NEW_TASK,
  TASKS_STILL,
  TASKS_SUBTASKS,
  tasksAddActive,
  tasksAdded,
  tasksCaretOn,
  tasksCellHighlighted,
  tasksCellMenu,
  tasksDraft,
  tasksExpanded,
  tasksListScroll,
  tasksNewPriority,
  tasksNewStatus,
  tasksOptionHovered,
  tasksSubtaskAddActive,
  tasksSubtaskCaretOn,
  tasksSubtaskCount,
  tasksSubtaskDraft,
  tasksTouches,
} from "./welcome-tasks-scene";

const B = TASKS_BEATS;
const TITLE = "Write chapter 2";
const SUBTASK_TITLES = ["Outline the scenes", "Write the first draft"] as const;

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

  it("closes the add field with the tap on the new task's chevron, which expands the task", () => {
    expect(tasksAddActive(B.expandTap - 1)).toBe(true);
    expect(tasksExpanded(B.expandTap - 1)).toBe(false);
    expect(tasksAddActive(B.expandTap)).toBe(false);
    expect(tasksCaretOn(B.expandTap)).toBe(false);
    expect(tasksExpanded(B.expandTap)).toBe(true);
    expect(tasksExpanded(TASKS_STILL)).toBe(true);
  });

  it("types each subtask into the add-subtask row and adds it with Enter, keeping the field open until the status tap", () => {
    expect(TASKS_SUBTASKS).toHaveLength(SUBTASK_TITLES.length);
    expect(tasksSubtaskAddActive(B.subtaskAddTap - 1)).toBe(false);
    expect(tasksSubtaskAddActive(B.subtaskAddTap)).toBe(true);
    expect(tasksSubtaskDraft(B.subtaskAddTap, SUBTASK_TITLES)).toBe("");
    expect(tasksSubtaskDraft(B.firstSubtaskTypeEnd, SUBTASK_TITLES)).toBe(SUBTASK_TITLES[0]);
    expect(tasksSubtaskCount(B.firstSubtaskSubmit - 1)).toBe(0);
    expect(tasksSubtaskCount(B.firstSubtaskSubmit)).toBe(1);
    expect(tasksSubtaskDraft(B.firstSubtaskSubmit, SUBTASK_TITLES)).toBe("");
    expect(tasksSubtaskDraft((B.secondSubtaskTypeStart + B.secondSubtaskTypeEnd) / 2, SUBTASK_TITLES).length).toBeGreaterThan(0);
    expect(tasksSubtaskDraft(B.secondSubtaskTypeEnd, SUBTASK_TITLES)).toBe(SUBTASK_TITLES[1]);
    expect(tasksSubtaskCount(B.secondSubtaskSubmit)).toBe(2);
    expect(tasksSubtaskDraft(B.secondSubtaskSubmit, SUBTASK_TITLES)).toBe("");
    expect(tasksSubtaskCaretOn((B.firstSubtaskTypeStart + B.firstSubtaskTypeEnd) / 2)).toBe(true);
    expect(tasksSubtaskCaretOn((B.secondSubtaskTypeStart + B.secondSubtaskTypeEnd) / 2)).toBe(true);
    expect(tasksSubtaskAddActive(B.statusTap - 1)).toBe(true);
    expect(tasksSubtaskAddActive(B.statusTap)).toBe(false);
    expect(tasksSubtaskCaretOn(B.statusTap)).toBe(false);
    expect(tasksSubtaskCount(TASKS_STILL)).toBe(2);
  });

  it("never shows both add fields open at once", () => {
    for (const elapsed of moments()) expect(tasksAddActive(elapsed) && tasksSubtaskAddActive(elapsed)).toBe(false);
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

  it("clicks with the desktop pointer on the beats the touches land on, resting on each control through its press", () => {
    expect(welcomeCursorScriptOrdered(TASKS_CURSOR)).toBe(true);
    const clicks = TASKS_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press?.end])).toEqual([
      ["tasks-add", B.addTap],
      ["tasks-expand", B.expandTap],
      ["tasks-subtask-add", B.subtaskAddTap],
      ["tasks-status", B.statusTap],
      ["tasks-status-option", B.statusPick],
      ["tasks-priority", B.priorityTap],
      ["tasks-priority-option", B.priorityPick],
    ]);
    for (const stop of clicks) {
      const press = stop.press;
      if (!press) continue;
      expect(press.start).toBe(press.end - FILM_TAP_MS);
      for (let elapsed = press.start; elapsed <= press.end; elapsed += 10) expect(welcomeCursorHovered(TASKS_CURSOR, elapsed)).toBe(stop.target);
    }
  });

  it("types into each add row under a text beam, hiding while the subtasks are typed, and reaches each cell only once the list has scrolled to it", () => {
    expect(TASKS_CURSOR.find((stop) => stop.target === "tasks-add")?.glyph).toBe("text");
    expect(TASKS_CURSOR.find((stop) => stop.target === "tasks-expand")?.glyph).toBe("pointer");
    expect(TASKS_CURSOR.find((stop) => stop.target === "tasks-subtask-add")?.glyph).toBe("text");
    expect(TASKS_CURSOR.find((stop) => stop.target === "tasks-expand")?.depart).toBeGreaterThanOrEqual(B.taskSubmit);
    const status = TASKS_CURSOR.find((stop) => stop.target === "tasks-status");
    const priority = TASKS_CURSOR.find((stop) => stop.target === "tasks-priority");
    expect(status?.glyph).toBe("pointer");
    expect(priority?.glyph).toBe("pointer");
    expect(status?.depart).toBeGreaterThan(B.statusScrollStart);
    expect(priority?.depart).toBeGreaterThan(B.priorityScrollStart);
    expect(welcomeCursorFrame(TASKS_CURSOR, B.secondSubtaskTypeStart).away).toBe(true);
    expect(welcomeCursorHovered(TASKS_CURSOR, B.statusScrollStart)).toBe("tasks-subtask-add");
  });
});
