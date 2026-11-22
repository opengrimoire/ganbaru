// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Project, ProjectTask } from "$lib/projects/types";
import ProjectBulkActionController from "./ProjectBulkActionController.svelte";

const store = vi.hoisted(() => ({
  archiveTasks: vi.fn<(tasks: ProjectTask[]) => Promise<void>>(),
  setActiveView: vi.fn<(view: string) => void>(),
}));

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({
    archiveTasks: store.archiveTasks,
    set activeView(view: string) { store.setActiveView(view); },
  });
  return { getProjects, useProjects: getProjects };
});

vi.mock("$lib/stores/calendar.svelte", () => ({
  getCalendar: () => ({ acceptNativeEdit: vi.fn(), refreshCurrentWindow: vi.fn() }),
}));

vi.mock("$lib/stores/preferences.svelte", () => ({
  getPreferences: () => ({ focusIdlePauseOnEventCreate: false, focusIdleThresholdMinutes: 5 }),
}));

vi.mock("$lib/projects/scheduling/controller", () => ({
  getProjectSchedulingController: () => ({
    recoverable: false,
    schedule: vi.fn(),
    retry: vi.fn(),
    acknowledge: vi.fn(),
  }),
}));

vi.mock("$lib/vault/active-vault", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/vault/active-vault")>()),
  requireActiveVaultIdentity: () => "vault-a",
}));

const ACTION_FAILED = "The action could not be applied to the selected tasks.";

/** Only the fields the bulk controller reads. */
function project(id: string): Project {
  return { id, name: id, defaultEventDurationMinutes: null } as Project;
}

function task(id: string, projectId: string): ProjectTask {
  return {
    id,
    projectId,
    sectionId: "section",
    statusId: "status",
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

const tasks = [task("task-a", "project-a"), task("task-b", "project-b")];
const view = $state({ projectId: "project-a", selectedTaskIds: ["task-a"] });

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

/** Mount the controller the way the Projects view does, with the selection bound to the route state. */
function render(): void {
  const selectedTasks = () => tasks.filter((item) => item.projectId === view.projectId && view.selectedTaskIds.includes(item.id));
  component = mount(ProjectBulkActionController, {
    target,
    props: {
      get selectedProject() { return project(view.projectId); },
      priorities: [],
      get selectedTasks() { return selectedTasks(); },
      get selectableTasks() { return tasks.filter((item) => item.projectId === view.projectId); },
      get selectedActiveTaskCount() { return selectedTasks().length; },
      selectedArchivedTaskCount: 0,
      statuses: [],
      get selectedTaskIds() { return view.selectedTaskIds; },
      set selectedTaskIds(ids: string[]) { view.selectedTaskIds = ids; },
      showArchivedTasks: false,
    },
  });
  flushSync();
}

function archiveButton(): HTMLButtonElement {
  const button = target.querySelector<HTMLButtonElement>('button[aria-label="Archive"]');
  if (!button) throw new Error("No archive button");
  return button;
}

/** Switch to project B and select one of its tasks while the project A action is still in flight. */
function switchToProjectB(): void {
  view.projectId = "project-b";
  view.selectedTaskIds = [...view.selectedTaskIds, "task-b"];
  flushSync();
}

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  view.projectId = "project-a";
  view.selectedTaskIds = ["task-a"];
  store.archiveTasks.mockReset();
  store.setActiveView.mockReset();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("ProjectBulkActionController", () => {
  it("keeps a selection made in another project when an earlier bulk action finishes", async () => {
    let finish: () => void = () => undefined;
    store.archiveTasks.mockReturnValue(new Promise<void>((resolve) => { finish = resolve; }));
    render();

    archiveButton().click();
    flushSync();
    switchToProjectB();
    finish();
    await vi.waitFor(() => expect(archiveButton().disabled).toBe(false));

    expect(view.selectedTaskIds).toContain("task-b");
  });

  it("shows a bulk action error only in the project the action ran in", async () => {
    let fail: (error: Error) => void = () => undefined;
    store.archiveTasks.mockReturnValue(new Promise<void>((_resolve, reject) => { fail = reject; }));
    render();

    archiveButton().click();
    flushSync();
    switchToProjectB();
    fail(new Error("archive failed"));
    await vi.waitFor(() => expect(archiveButton().disabled).toBe(false));

    expect(target.textContent).not.toContain(ACTION_FAILED);

    view.projectId = "project-a";
    flushSync();
    expect(target.textContent).toContain(ACTION_FAILED);
  });
});
