import { describe, expect, it, vi } from "vitest";
import type { ProjectTask } from "$lib/projects/types";
import { ProjectListInteractionController } from "./interaction-controller.svelte";

function task(id: string): ProjectTask {
  return {
    id,
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title: id,
    description: "",
    priority: "normal",
    taskType: "task",
    sectionOrderKey: "1000",
    statusOrderKey: "1000",
    milestone: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

describe("ProjectListInteractionController", () => {
  it("keeps row edit menus mutually exclusive", () => {
    const controller = new ProjectListInteractionController({
      getSelectedTaskIds: () => [],
      selectedTaskIdsChanged: vi.fn(),
      dateMenusAreDialogs: false,
    });

    controller.toggleTaskMenu("status", "task-a");
    expect(controller.statusMenuTaskId).toBe("task-a");
    controller.toggleTaskMenu("due", "task-b");
    expect(controller.statusMenuTaskId).toBeNull();
    expect(controller.dueDateMenuTaskId).toBe("task-b");
    controller.toggleTaskMenu("due", "task-b");
    expect(controller.dueDateMenuTaskId).toBeNull();
  });

  it("updates individual and grouped selections without dropping unrelated ids", () => {
    let selectedIds = ["outside", "task-a"];
    const changed = vi.fn((next: string[]) => { selectedIds = next; });
    const controller = new ProjectListInteractionController({
      getSelectedTaskIds: () => selectedIds,
      selectedTaskIdsChanged: changed,
      dateMenusAreDialogs: false,
    });
    const first = task("task-a");
    const second = task("task-b");

    controller.toggleTaskSelection(first);
    expect(selectedIds).toEqual(["outside"]);
    controller.toggleTaskGroupSelection([first, second]);
    expect(selectedIds).toEqual(["outside", "task-a", "task-b"]);
    controller.toggleTaskGroupSelection([first, second]);
    expect(selectedIds).toEqual(["outside"]);
  });

  it("closes only menus whose owning surface was clicked outside", () => {
    const controller = new ProjectListInteractionController({
      getSelectedTaskIds: () => [],
      selectedTaskIdsChanged: vi.fn(),
      dateMenusAreDialogs: false,
    });
    controller.sectionOptionsMenuId = "section-a";
    controller.toggleTaskMenu("priority", "task-a");
    const child = {
      closest: (selector: string) => selector === "[data-list-priority-menu-root='true']" ? {} : null,
    } as unknown as Element;

    controller.handleOutsidePointerTarget(child);
    expect(controller.priorityMenuTaskId).toBe("task-a");
    expect(controller.sectionOptionsMenuId).toBeNull();
  });

  it("closes anchored date menus on an outside press but leaves dialog date menus to their own backdrop", () => {
    const outside = { closest: () => null } as unknown as Element;
    const anchored = new ProjectListInteractionController({
      getSelectedTaskIds: () => [],
      selectedTaskIdsChanged: vi.fn(),
      dateMenusAreDialogs: false,
    });
    anchored.toggleTaskMenu("due", "task-a");
    anchored.handleOutsidePointerTarget(outside);
    expect(anchored.dueDateMenuTaskId).toBeNull();

    const dialog = new ProjectListInteractionController({
      getSelectedTaskIds: () => [],
      selectedTaskIdsChanged: vi.fn(),
      dateMenusAreDialogs: true,
    });
    dialog.toggleTaskMenu("start", "task-a");
    dialog.handleOutsidePointerTarget(outside);
    expect(dialog.startDateMenuTaskId).toBe("task-a");
  });

  it("expands tasks with subtasks by default and opens childless tasks on request", () => {
    const controller = new ProjectListInteractionController({
      getSelectedTaskIds: () => [],
      selectedTaskIdsChanged: vi.fn(),
      dateMenusAreDialogs: false,
    });
    const parent = task("parent");
    const leaf = task("leaf");

    expect(controller.taskExpanded(parent, true)).toBe(true);
    expect(controller.taskExpanded(leaf, false)).toBe(false);
    controller.toggleTaskExpanded(parent, true);
    controller.toggleTaskExpanded(leaf, false);
    expect(controller.taskExpanded(parent, true)).toBe(false);
    expect(controller.taskExpanded(leaf, false)).toBe(true);
  });
});
