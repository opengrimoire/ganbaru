// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectTask } from "$lib/projects/types";
import ProjectTaskDetailPanel from "./ProjectTaskDetailPanel.svelte";

const store = vi.hoisted(() => ({
  tasks: [] as ProjectTask[],
  archiveTasks: vi.fn((_tasks: ProjectTask[]) => Promise.resolve()),
}));

vi.mock("$lib/stores/projects.svelte", () => {
  const known: Record<string, unknown> = {
    taskById: (id: string) => store.tasks.find((task) => task.id === id),
    archiveTasks: store.archiveTasks,
    projectById: () => undefined,
    statusById: () => undefined,
    defaultStatus: () => undefined,
    defaultSection: () => undefined,
    customFieldValueForTask: () => undefined,
  };
  // Every other read the panel makes is a list query, which is empty in these tests.
  const projects = new Proxy(known, {
    get: (target, property: string) => target[property] ?? (() => []),
  });
  return { getProjects: () => projects, useProjects: () => projects };
});

vi.mock("$lib/stores/calendar.svelte", () => ({ getCalendar: () => ({ sourceEvents: [] }) }));
vi.mock("$lib/stores/theme.svelte", () => ({ getTheme: () => ({ current: "light" }) }));
vi.mock("$lib/stores/mobile-back-stack.svelte", () => ({
  getMobileBackStack: () => ({ activate: () => () => undefined }),
}));
vi.mock("$lib/stores/project-conflicts.svelte", () => ({
  projectConflicts: { revision: 0, read: () => undefined, ensure: () => Promise.resolve(), invalidate: () => undefined },
}));

function task(id: string, title: string): ProjectTask {
  return {
    id,
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title,
    description: "",
    priority: "none",
    taskType: "task",
    sectionOrderKey: "0",
    statusOrderKey: "0",
    milestone: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    detailLoaded: true,
  };
}

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function archiveButton(): HTMLButtonElement {
  const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
    .find((entry) => entry.textContent?.trim() === "Archive task");
  if (!button) throw new Error("No archive button");
  return button;
}

beforeEach(() => {
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: false, media: query, addEventListener: () => undefined, removeEventListener: () => undefined }));
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  store.tasks = [task("task-a", "Alpha"), task("task-b", "Beta")];
  store.archiveTasks.mockReset();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  target.remove();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("ProjectTaskDetailPanel", () => {
  it("keeps an archive of the previous task off a task opened in the panel while it ran", async () => {
    let failArchive: (error: Error) => void = () => undefined;
    store.archiveTasks.mockImplementation(() => new Promise<void>((_resolve, reject) => { failArchive = reject; }));
    const props = $state({
      taskId: "task-a" as string | null,
      showArchivedTasks: false,
      showInactiveSections: false,
      onClose: () => undefined,
      onOpenTask: () => undefined,
      onShowArchivedTasks: () => undefined,
    });
    component = mount(ProjectTaskDetailPanel, { target, props });
    flushSync();

    archiveButton().click();
    flushSync();
    expect(archiveButton().disabled).toBe(true);

    props.taskId = "task-b";
    flushSync();
    expect(archiveButton().disabled).toBe(false);

    failArchive(new Error("archive failed"));
    await Promise.resolve();
    await Promise.resolve();
    flushSync();
    expect(document.body.textContent).not.toContain("archive failed");
    expect(document.body.textContent).not.toContain("Could not archive task.");
    expect(archiveButton().disabled).toBe(false);
  });
});
