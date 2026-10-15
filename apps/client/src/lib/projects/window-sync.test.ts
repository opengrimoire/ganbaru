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

    expect(projectMutationChangeKeys(mutation({ tasks, views }), "selected").sort()).toEqual(["a", "b", "c"]);
  });

  it("falls back to the selected project for row-less commits such as removals", () => {
    expect(projectMutationChangeKeys(mutation({}), "selected")).toEqual(["selected"]);
  });

  it("uses the workspace key when no project is selected", () => {
    expect(projectMutationChangeKeys(mutation({}), null)).toEqual([PROJECTS_WORKSPACE_CHANGE_KEY]);
  });
});
