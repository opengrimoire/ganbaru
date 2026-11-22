// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { loadProjectsWorkspace, refreshProjectsWorkspace } from "$lib/api/projects";
import { notifyProjectsSyncChanged } from "$lib/projects/sync-events";
import type { Project, ProjectsSnapshot, ProjectsWorkspaceSnapshot } from "$lib/projects/types";

vi.mock("$lib/api/projects", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/projects")>();
  return { ...actual, loadProjectsWorkspace: vi.fn(), refreshProjectsWorkspace: vi.fn() };
});

const project = (id: string): Project => ({
  id,
  groupId: "group-1",
  name: id,
  icon: "lucide:folder",
  color: 8,
  orderKey: id,
  status: "active",
  defaultEventName: null,
  defaultEventTimeMode: "timed",
  defaultEventDurationMinutes: 60,
  defaultPomodoroMode: "preset",
  defaultPomodoroPresetKey: "adaptive",
  defaultIdleSettingsSource: "global",
  defaultIdlePauseEnabled: true,
  defaultIdleThresholdMinutes: 5,
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
});

function workspace(resolvedProjectId: string): ProjectsWorkspaceSnapshot {
  const snapshot: ProjectsSnapshot = {
    groups: [{
      id: "group-1", name: "Group", icon: "lucide:folder", orderKey: "0", collapsed: false,
      createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
    }],
    projects: [project("project-1"), project("project-2"), project("project-3"), project("project-4")],
    sections: [], statuses: [], priorities: [], tasks: [], checklistItems: [], tags: [], taskTagLinks: [],
    customFields: [], customFieldOptions: [], customFieldValues: [], customFieldOptionValues: [], dependencies: [],
    eventLinks: [], taskChangeEvents: [], viewPreferences: [], views: [], customEmojis: [],
  };
  return { resolvedProjectId, snapshot };
}

/** Waits for pending promise callbacks. */
async function settle(): Promise<void> {
  for (let pass = 0; pass < 20; pass += 1) await Promise.resolve();
}

describe("Projects selection", () => {
  it("selects a project whose load was in flight when another window's change arrived", async () => {
    vi.mocked(loadProjectsWorkspace).mockResolvedValueOnce(workspace("project-1"));
    const { getProjects } = await import("./projects.svelte");
    const projects = getProjects();
    await projects.ensureLoaded();
    expect(projects.selectedProjectId).toBe("project-1");

    let finishSelection!: (value: ProjectsWorkspaceSnapshot) => void;
    vi.mocked(refreshProjectsWorkspace)
      .mockImplementationOnce(() => new Promise((resolve) => { finishSelection = resolve; }))
      .mockImplementation(async (projectId) => workspace(projectId ?? "project-1"));
    const selecting = projects.selectProject("project-2");
    notifyProjectsSyncChanged();
    await settle();
    finishSelection(workspace("project-2"));
    await selecting;
    await settle();

    expect(projects.selectedProjectId).toBe("project-2");
  });

  it("selects a project in one pane whose load the other pane's selection superseded", async () => {
    const { getProjects, getProjectsStore } = await import("./projects.svelte");
    const primary = getProjects();
    const secondary = getProjectsStore("secondary");
    await primary.ensureLoaded();

    let finishSelection!: (value: ProjectsWorkspaceSnapshot) => void;
    vi.mocked(refreshProjectsWorkspace)
      .mockImplementationOnce(() => new Promise((resolve) => { finishSelection = resolve; }))
      .mockImplementation(async (projectId) => workspace(projectId ?? "project-1"));
    const selecting = secondary.selectProject("project-3");
    await primary.selectProject("project-4");
    finishSelection(workspace("project-3"));
    await selecting;

    expect(secondary.selectedProjectId).toBe("project-3");
    expect(primary.selectedProjectId).toBe("project-4");
  });
});
