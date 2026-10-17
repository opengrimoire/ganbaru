import { describe, expect, it } from "vitest";
import {
  projectListDropSortOrder,
  projectListDropPositionFromPoint,
  projectListPointerDragGestureReady,
  projectListSectionDropSortOrder,
  projectListSectionDragAllowed,
  projectListSectionDropAllowed,
  projectListTaskDragAllowed,
  projectListTaskDropAllowed,
  type ProjectListDraggableSection,
  type ProjectListDraggableTask,
  type ProjectListDragSection,
  type ProjectListDragTask,
  type ProjectListPointerDragGesture,
} from "./drag";

function task(id: string, sectionSortOrder: number): ProjectListDragTask {
  return { id, sectionSortOrder };
}

function section(id: string, sortOrder: number): ProjectListDragSection {
  return { id, sortOrder };
}

function dragGesture(overrides: Partial<ProjectListPointerDragGesture> = {}): ProjectListPointerDragGesture {
  return {
    itemId: "item",
    pointerId: 1,
    startX: 10,
    startY: 20,
    startedAt: 1000,
    ...overrides,
  };
}

function draggableTask(overrides: Partial<ProjectListDraggableTask> = {}): ProjectListDraggableTask {
  return {
    id: "task",
    projectId: "project",
    ...overrides,
  };
}

function draggableSection(overrides: Partial<ProjectListDraggableSection> = {}): ProjectListDraggableSection {
  return {
    id: "section",
    projectId: "project",
    ...overrides,
  };
}

describe("projectListDropSortOrder", () => {
  it("places a task between visual neighbors in ascending order", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("a", 1000), task("b", 2000), task("c", 3000)],
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "asc",
    })).toBe(2500);
  });

  it("places a task before the first visual task in ascending order", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("a", 1000), task("b", 2000)],
      draggedTaskId: "dragged",
      overTaskId: "a",
      position: "before",
      sortDirection: "asc",
    })).toBe(500);
  });

  it("appends a task in ascending order when no target task is supplied", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("a", 1000), task("b", 2000)],
      draggedTaskId: "dragged",
      sortDirection: "asc",
    })).toBe(3000);
  });

  it("places a task between visual neighbors in descending order", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("c", 3000), task("b", 2000), task("a", 1000)],
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "desc",
    })).toBe(1500);
  });

  it("places a task before the first visual task in descending order", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("b", 2000), task("a", 1000)],
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "before",
      sortDirection: "desc",
    })).toBe(3000);
  });

  it("ignores the dragged task when calculating a same-section insertion", () => {
    expect(projectListDropSortOrder({
      orderedTasks: [task("a", 1000), task("dragged", 2000), task("b", 3000)],
      draggedTaskId: "dragged",
      overTaskId: "b",
      position: "after",
      sortDirection: "asc",
    })).toBe(4000);
  });
});

describe("projectListPointerDragGestureReady", () => {
  it("rejects missing and mismatched gestures", () => {
    expect(projectListPointerDragGestureReady({
      gesture: null,
      itemId: "item",
      clientX: 14,
      clientY: 20,
      now: 1000,
    })).toBe(false);
    expect(projectListPointerDragGestureReady({
      gesture: dragGesture({ itemId: "other" }),
      itemId: "item",
      clientX: 14,
      clientY: 20,
      now: 1000,
    })).toBe(false);
  });

  it("starts after the movement threshold", () => {
    expect(projectListPointerDragGestureReady({
      gesture: dragGesture(),
      itemId: "item",
      clientX: 14,
      clientY: 20,
      now: 1000,
    })).toBe(true);
  });

  it("starts after the hold delay with small movement", () => {
    expect(projectListPointerDragGestureReady({
      gesture: dragGesture(),
      itemId: "item",
      clientX: 11,
      clientY: 20,
      now: 1120,
    })).toBe(true);
  });

  it("waits while movement and hold time are below the thresholds", () => {
    expect(projectListPointerDragGestureReady({
      gesture: dragGesture(),
      itemId: "item",
      clientX: 11,
      clientY: 20,
      now: 1119,
    })).toBe(false);
  });
});

describe("projectListTaskDragAllowed", () => {
  it("allows top-level active tasks without pending drops", () => {
    expect(projectListTaskDragAllowed({
      dragEnabled: true,
      task: draggableTask(),
      dropPendingTaskId: null,
    })).toBe(true);
  });

  it("rejects disabled, archived, child, and pending tasks", () => {
    expect(projectListTaskDragAllowed({
      dragEnabled: false,
      task: draggableTask(),
      dropPendingTaskId: null,
    })).toBe(false);
    expect(projectListTaskDragAllowed({
      dragEnabled: true,
      task: draggableTask({ archivedAt: "2026-01-01T00:00:00Z" }),
      dropPendingTaskId: null,
    })).toBe(false);
    expect(projectListTaskDragAllowed({
      dragEnabled: true,
      task: draggableTask({ parentTaskId: "parent" }),
      dropPendingTaskId: null,
    })).toBe(false);
    expect(projectListTaskDragAllowed({
      dragEnabled: true,
      task: draggableTask(),
      dropPendingTaskId: "task",
    })).toBe(false);
  });
});

describe("projectListSectionDragAllowed", () => {
  it("allows active visible sections without pending drops", () => {
    expect(projectListSectionDragAllowed({
      dragEnabled: true,
      section: draggableSection(),
      dropPendingSectionId: null,
    })).toBe(true);
  });

  it("rejects disabled, archived, hidden, and pending sections", () => {
    expect(projectListSectionDragAllowed({
      dragEnabled: false,
      section: draggableSection(),
      dropPendingSectionId: null,
    })).toBe(false);
    expect(projectListSectionDragAllowed({
      dragEnabled: true,
      section: draggableSection({ archivedAt: "2026-01-01T00:00:00Z" }),
      dropPendingSectionId: null,
    })).toBe(false);
    expect(projectListSectionDragAllowed({
      dragEnabled: true,
      section: draggableSection({ hiddenAt: "2026-01-01T00:00:00Z" }),
      dropPendingSectionId: null,
    })).toBe(false);
    expect(projectListSectionDragAllowed({
      dragEnabled: true,
      section: draggableSection(),
      dropPendingSectionId: "section",
    })).toBe(false);
  });
});

describe("projectListTaskDropAllowed", () => {
  it("allows top-level active tasks in active sections from the same project", () => {
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask(),
      targetSection: draggableSection(),
    })).toBe(true);
  });

  it("rejects disabled and missing tasks", () => {
    expect(projectListTaskDropAllowed({
      dragEnabled: false,
      task: draggableTask(),
      targetSection: draggableSection(),
    })).toBe(false);
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: undefined,
      targetSection: draggableSection(),
    })).toBe(false);
  });

  it("rejects archived, child, inactive, and cross-project drops", () => {
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask({ archivedAt: "2026-01-01T00:00:00Z" }),
      targetSection: draggableSection(),
    })).toBe(false);
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask({ parentTaskId: "parent" }),
      targetSection: draggableSection(),
    })).toBe(false);
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask(),
      targetSection: draggableSection({ archivedAt: "2026-01-01T00:00:00Z" }),
    })).toBe(false);
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask(),
      targetSection: draggableSection({ hiddenAt: "2026-01-01T00:00:00Z" }),
    })).toBe(false);
    expect(projectListTaskDropAllowed({
      dragEnabled: true,
      task: draggableTask({ projectId: "other" }),
      targetSection: draggableSection(),
    })).toBe(false);
  });
});

describe("projectListSectionDropAllowed", () => {
  it("allows active visible sections from the same project", () => {
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ id: "dragged" }),
      targetSection: draggableSection({ id: "target" }),
    })).toBe(true);
  });

  it("rejects disabled and missing sections", () => {
    expect(projectListSectionDropAllowed({
      dragEnabled: false,
      draggedSection: draggableSection({ id: "dragged" }),
      targetSection: draggableSection({ id: "target" }),
    })).toBe(false);
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: undefined,
      targetSection: draggableSection({ id: "target" }),
    })).toBe(false);
  });

  it("rejects inactive and cross-project section drops", () => {
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ archivedAt: "2026-01-01T00:00:00Z" }),
      targetSection: draggableSection({ id: "target" }),
    })).toBe(false);
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ hiddenAt: "2026-01-01T00:00:00Z" }),
      targetSection: draggableSection({ id: "target" }),
    })).toBe(false);
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ id: "dragged" }),
      targetSection: draggableSection({ id: "target", archivedAt: "2026-01-01T00:00:00Z" }),
    })).toBe(false);
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ id: "dragged" }),
      targetSection: draggableSection({ id: "target", hiddenAt: "2026-01-01T00:00:00Z" }),
    })).toBe(false);
    expect(projectListSectionDropAllowed({
      dragEnabled: true,
      draggedSection: draggableSection({ id: "dragged", projectId: "other" }),
      targetSection: draggableSection({ id: "target" }),
    })).toBe(false);
  });
});

describe("projectListDropPositionFromPoint", () => {
  it("uses the vertical midpoint as the before or after boundary", () => {
    const rect = { top: 10, height: 20 };
    expect(projectListDropPositionFromPoint(19, rect)).toBe("before");
    expect(projectListDropPositionFromPoint(20, rect)).toBe("after");
  });
});

describe("projectListSectionDropSortOrder", () => {
  it("places a section between visual neighbors", () => {
    expect(projectListSectionDropSortOrder({
      orderedSections: [section("a", 1000), section("b", 2000), section("c", 3000)],
      draggedSectionId: "dragged",
      overSectionId: "b",
      position: "after",
    })).toBe(2500);
  });

  it("places a section before the first section", () => {
    expect(projectListSectionDropSortOrder({
      orderedSections: [section("a", 1000), section("b", 2000)],
      draggedSectionId: "dragged",
      overSectionId: "a",
      position: "before",
    })).toBe(500);
  });

  it("appends a section when no target section is supplied", () => {
    expect(projectListSectionDropSortOrder({
      orderedSections: [section("a", 1000), section("b", 2000)],
      draggedSectionId: "dragged",
    })).toBe(3000);
  });

  it("ignores the dragged section when calculating same-list insertion", () => {
    expect(projectListSectionDropSortOrder({
      orderedSections: [section("a", 1000), section("dragged", 2000), section("b", 3000)],
      draggedSectionId: "dragged",
      overSectionId: "b",
      position: "after",
    })).toBe(4000);
  });
});
