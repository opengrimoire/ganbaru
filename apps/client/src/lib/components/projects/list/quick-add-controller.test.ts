import { describe, expect, it, vi } from "vitest";
import type { ProjectTask } from "$lib/projects/types";
import {
  ProjectListQuickAddController,
  type ProjectListQuickAddControllerContext,
} from "./quick-add-controller.svelte";

function controller(): ProjectListQuickAddController {
  return new ProjectListQuickAddController({
    projects: {} as ProjectListQuickAddControllerContext["projects"],
    getSelectedProjectId: () => "project-a",
    getGroupBy: () => "section",
    getStatuses: () => [],
    getPriorities: () => [],
    revealTask: vi.fn(),
    selectProjectFirstMessage: () => "Select a project",
    createFailedMessage: () => "Create failed",
  });
}

describe("ProjectListQuickAddController", () => {
  it("cancels only drafts outside their owning quick-add row", () => {
    const quickAdd = controller();
    quickAdd.activeSectionTaskDraftInputId = "section-a";
    quickAdd.sectionTaskDrafts = { "section-a": "Keep me" };
    const inside = {
      closest: (selector: string) => selector === "[data-section-task-add-row]"
        ? { getAttribute: () => "section-a" }
        : null,
    } as unknown as Element;

    quickAdd.cancelForOutsideTarget(inside);
    expect(quickAdd.sectionTaskDrafts["section-a"]).toBe("Keep me");
    expect(quickAdd.activeSectionTaskDraftInputId).toBe("section-a");

    const outside = { closest: () => null } as unknown as Element;
    quickAdd.cancelForOutsideTarget(outside);
    expect(quickAdd.sectionTaskDrafts["section-a"]).toBe("");
    expect(quickAdd.activeSectionTaskDraftInputId).toBeNull();
  });

  it("clears matching create errors when an active draft is cancelled", () => {
    const quickAdd = controller();
    quickAdd.activeGroupTaskDraftInputId = "status:active";
    quickAdd.groupTaskDrafts = { "status:active": "Draft" };
    quickAdd.errorTarget = "group:status:active";
    quickAdd.errorMessage = "Create failed";

    quickAdd.cancelActiveDrafts();
    expect(quickAdd.groupTaskDrafts["status:active"]).toBe("");
    expect(quickAdd.errorTarget).toBeNull();
    expect(quickAdd.errorMessage).toBeNull();
  });

  it("creates a subtask under its parent and keeps the row open for the next one", async () => {
    const parent = { id: "parent", projectId: "project-a", sectionId: "section-b" } as ProjectTask;
    const addTask = vi.fn(async () => ({ id: "child" }) as ProjectTask);
    const quickAdd = new ProjectListQuickAddController({
      projects: { addTask } as unknown as ProjectListQuickAddControllerContext["projects"],
      getSelectedProjectId: () => "project-a",
      getGroupBy: () => "section",
      getStatuses: () => [],
      getPriorities: () => [],
      revealTask: vi.fn(),
      selectProjectFirstMessage: () => "Select a project",
      createFailedMessage: () => "Create failed",
    });
    quickAdd.setSubtaskActive(parent, true);
    quickAdd.setSubtaskDraft(parent, "  Child  ");

    await quickAdd.submitSubtask(parent);
    expect(addTask).toHaveBeenCalledWith("project-a", "Child", "section-b", undefined, "parent");
    expect(quickAdd.subtaskDraft(parent)).toBe("");
    expect(quickAdd.subtaskActive(parent)).toBe(true);

    quickAdd.cancelForOutsideTarget({ closest: () => null } as unknown as Element);
    expect(quickAdd.subtaskActive(parent)).toBe(false);
  });
});
