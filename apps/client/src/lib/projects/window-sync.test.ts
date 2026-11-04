import { describe, expect, it, vi } from "vitest";
import type { ProjectMutation, ProjectsSnapshot, ProjectTask, ProjectView } from "$lib/projects/types";
import {
  listenForProjectChanges, projectMutationChangeKeys, projectsStillLoaded, PROJECTS_SYNC_CHANGE_KEY,
  PROJECTS_WORKSPACE_CHANGE_KEY, touchesProjects,
} from "./window-sync";

const channel = vi.hoisted(() => ({ listen: vi.fn() }));
const sync = vi.hoisted(() => ({ listenSyncApplied: vi.fn() }));

vi.mock("$lib/windows/vault-change-channel", () => ({
  createVaultChangeChannel: () => ({ publish: vi.fn(), listen: channel.listen }),
}));
vi.mock("$lib/api/sync", () => sync);

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

describe("listenForProjectChanges", () => {
  it("marks every project as changed when sync applied Projects rows", async () => {
    channel.listen.mockResolvedValueOnce(() => undefined);
    let applied: (tables: string[]) => void = () => undefined;
    sync.listenSyncApplied.mockImplementationOnce((listener: (tables: string[]) => void) => {
      applied = listener;
      return Promise.resolve(() => undefined);
    });
    const onChange = vi.fn();
    await listenForProjectChanges(onChange);

    applied(["quick_notes"]);
    expect(onChange).not.toHaveBeenCalled();

    applied(["project_tasks"]);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(new Set([PROJECTS_SYNC_CHANGE_KEY]));
  });

  it("stops both sources together", async () => {
    const stopWindows = vi.fn();
    const stopReplicated = vi.fn();
    channel.listen.mockResolvedValueOnce(stopWindows);
    sync.listenSyncApplied.mockResolvedValueOnce(stopReplicated);

    (await listenForProjectChanges(vi.fn()))();

    expect(stopWindows).toHaveBeenCalledOnce();
    expect(stopReplicated).toHaveBeenCalledOnce();
  });

  it("does not keep the window listener when the sync subscription fails", async () => {
    const stopWindows = vi.fn();
    channel.listen.mockResolvedValueOnce(stopWindows);
    sync.listenSyncApplied.mockRejectedValueOnce(new Error("no service"));

    await expect(listenForProjectChanges(vi.fn())).rejects.toThrow("no service");

    expect(stopWindows).toHaveBeenCalledOnce();
  });
});

describe("touchesProjects", () => {
  it("matches replicated Projects tables only", () => {
    expect(touchesProjects(["quick_notes", "project_task_change_events"])).toBe(true);
    expect(touchesProjects(["calendar_events"])).toBe(false);
  });
});

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

describe("projectsStillLoaded", () => {
  const loaded = ["shown", "changed", "kept"];

  it("drops changed projects that no pane shows", () => {
    expect(projectsStillLoaded(loaded, new Set(["shown"]), new Set(["shown", "changed"]))).toEqual(["shown", "kept"]);
  });

  it("keeps every loaded project for a workspace change", () => {
    expect(projectsStillLoaded(loaded, new Set(), new Set([PROJECTS_WORKSPACE_CHANGE_KEY]))).toEqual(loaded);
  });

  it("drops every project no pane shows when sync applied Projects rows", () => {
    expect(projectsStillLoaded(loaded, new Set(["shown"]), new Set([PROJECTS_SYNC_CHANGE_KEY]))).toEqual(["shown"]);
  });
});
