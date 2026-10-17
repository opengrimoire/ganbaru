import { describe, expect, it, vi } from "vitest";
import type {
  ProjectSection,
  ProjectTask,
  ProjectTaskGroupMode,
  ProjectTaskSortMode,
} from "$lib/projects/types";
import { ProjectListRowDragController } from "./row-drag-controller.svelte";

function task(overrides: Partial<ProjectTask> = {}): ProjectTask {
  return {
    id: "task-a",
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title: "Task",
    description: "",
    priority: "normal",
    taskType: "task",
    sectionSortOrder: 1000,
    statusSortOrder: 1000,
    milestone: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
    ...overrides,
  };
}

function section(overrides: Partial<ProjectSection> = {}): ProjectSection {
  return {
    id: "section-a",
    projectId: "project-a",
    name: "Section",
    sortOrder: 1000,
    collapsed: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
    ...overrides,
  };
}

function controller(options: {
  groupBy?: ProjectTaskGroupMode;
  sortMode?: ProjectTaskSortMode;
  tasks?: ProjectTask[];
} = {}): ProjectListRowDragController {
  const tasks = options.tasks ?? [task()];
  return new ProjectListRowDragController({
    getTasks: () => tasks,
    getAllTasks: () => tasks,
    getSections: () => [section()],
    getGroupBy: () => options.groupBy ?? "section",
    getSortMode: () => options.sortMode ?? "manual",
    getSortDirection: () => "asc",
    updateTask: vi.fn(),
    updateSection: vi.fn(),
  });
}

describe("ProjectListRowDragController", () => {
  it("enforces manual section-only task dragging and archived restrictions", () => {
    expect(controller().canStartTask(task())).toBe(true);
    expect(controller({ sortMode: "status" }).canStartTask(task())).toBe(false);
    expect(controller({ groupBy: "status" }).canStartTask(task())).toBe(false);
    expect(controller().canStartTask(task({ archivedAt: "2026-06-21T00:00:00.000Z" }))).toBe(false);
    expect(controller().canStartSection(section({ archivedAt: "2026-06-21T00:00:00.000Z" }))).toBe(false);
  });

  it("clears every task drag marker when a drag ends", () => {
    const drag = controller();
    drag.draggingTaskId = "task-a";
    drag.dropPendingTaskId = "task-a";
    drag.dragOverSectionId = "section-a";
    drag.dragOverTaskId = "task-b";
    drag.dragOverPosition = "after";
    drag.rowGesture = {
      itemId: "task-a",
      pointerId: 1,
      startX: 0,
      startY: 0,
      startedAt: 0,
    };

    drag.handleTaskDragEnd();
    expect(drag.draggingTaskId).toBeNull();
    expect(drag.dropPendingTaskId).toBeNull();
    expect(drag.dragOverSectionId).toBeNull();
    expect(drag.dragOverTaskId).toBeNull();
    expect(drag.dragOverPosition).toBeNull();
    expect(drag.rowGesture).toBeNull();
  });
});
