// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadProjectsWorkspace, refreshProjectsWorkspace } from "$lib/api/projects";
import type { Project, ProjectsSnapshot, ProjectsWorkspaceSnapshot } from "$lib/projects/types";

const deferred = vi.hoisted(() => new Promise<never>(() => undefined));

vi.mock("$lib/components/projects/component-registry", () => ({
  loadProjectView: () => deferred,
  retryProjectView: () => deferred,
  loadProjectOptionalComponent: () => deferred,
  retryProjectOptionalComponent: () => deferred,
}));

vi.mock("./sidebar/ProjectsSidebar.svelte", async () => ({
  default: (await import("./ProjectsViewSidebarHarness.test.svelte")).default,
}));

vi.mock("@tauri-apps/api/window", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/window")>()),
  getCurrentWindow: () => ({ label: "main" }),
}));

vi.mock("@tauri-apps/api/event", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/event")>()),
  listen: () => Promise.resolve(() => undefined),
}));

vi.mock("$lib/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/api/projects")>()),
  loadProjectsWorkspace: vi.fn(),
  refreshProjectsWorkspace: vi.fn(),
}));

const project = (id: string): Project => ({
  id,
  groupId: null,
  name: id,
  icon: "lucide:folder",
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
    groups: [],
    projects: [project("project-1"), project("project-2"), project("project-3")],
    sections: [], statuses: [], priorities: [], tasks: [], checklistItems: [], tags: [], taskTagLinks: [],
    customFields: [], customFieldOptions: [], customFieldValues: [], customFieldOptionValues: [], dependencies: [],
    eventLinks: [], taskChangeEvents: [], viewPreferences: [], views: [], customEmojis: [],
  };
  return { resolvedProjectId, snapshot };
}

/** Whether the project header shows its settings panel as open. */
function settingsPanelOpen(target: HTMLElement): boolean {
  return target.querySelector('[data-project-toolbar-trigger="settings"][aria-expanded="true"]') !== null;
}

describe("ProjectsView project settings", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  it("does not open settings on another project when a newer selection supersedes the settings target", async () => {
    vi.mocked(loadProjectsWorkspace).mockResolvedValue(workspace("project-1"));
    let finishSettingsLoad!: (value: ProjectsWorkspaceSnapshot) => void;
    vi.mocked(refreshProjectsWorkspace)
      .mockImplementationOnce(() => new Promise((resolve) => { finishSettingsLoad = resolve; }))
      .mockImplementation(async (projectId) => workspace(projectId ?? "project-1"));
    target = document.createElement("div");
    document.body.append(target);
    const { default: ProjectsView } = await import("./ProjectsView.svelte");
    component = mount(ProjectsView, { target });
    const root = target;
    await vi.waitFor(() => expect(root.querySelector("[data-open-settings]")).not.toBeNull());

    root.querySelector<HTMLButtonElement>("[data-open-settings]")?.click();
    flushSync();
    root.querySelector<HTMLButtonElement>("[data-select-project]")?.click();
    finishSettingsLoad(workspace("project-2"));
    await vi.waitFor(() => expect(root.textContent).toContain("project-3"));
    for (let pass = 0; pass < 20; pass += 1) await Promise.resolve();
    flushSync();

    expect(root.textContent).not.toContain("project-2");
    expect(settingsPanelOpen(root)).toBe(false);
  }, 15_000);
});
