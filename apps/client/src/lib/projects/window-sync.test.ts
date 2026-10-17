import { describe, expect, it, vi } from "vitest";
import type { ProjectMutation, ProjectsSnapshot, ProjectTask, ProjectView } from "$lib/projects/types";
import { projectMutationChangeKeys, PROJECTS_WORKSPACE_CHANGE_KEY } from "./window-sync";

vi.mock("$lib/windows/vault-change-channel", () => ({
  createVaultChangeChannel: () => ({ publish: vi.fn(), listen: vi.fn() }),
}));

function mutation(changed: Partial<ProjectsSnapshot>): ProjectMutation {
  return {
    changed: {
      groups: [], projects: [], sections: [], statuses: [], priorities: [], tasks: [],
      checklistItems: [], tags: [], taskTagLinks: [], customFields: [], customFieldOptions: [],
      customFieldValues: [], customFieldOptionValues: [], dependencies: [], eventLinks: [],
      taskChangeEvents: [], viewPreferences: [], views: [], customEmojis: [],
      ...changed,
    },
    removals: [],
    calendarEventProjectAssignments: [],
  };
}

describe("projectMutationChangeKeys", () => {
  it("names each project whose rows a commit changed once", () => {
    const tasks = [{ projectId: "a" }, { projectId: "b" }, { projectId: "a" }] as ProjectTask[];
    const views = [{ projectId: "c" }] as ProjectView[];

    expect(projectMutationChangeKeys(mutation({ tasks, views }), ["selected"]).sort()).toEqual(["a", "b", "c"]);
  });

  it("falls back to the projects the window shows for row-less commits such as removals", () => {
    expect(projectMutationChangeKeys(mutation({}), ["left", null, "right", "left"]).sort()).toEqual(["left", "right"]);
  });

  it("uses the workspace key when no pane shows a project", () => {
    expect(projectMutationChangeKeys(mutation({}), [null])).toEqual([PROJECTS_WORKSPACE_CHANGE_KEY]);
  });
});
