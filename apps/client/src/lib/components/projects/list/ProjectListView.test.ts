// @vitest-environment jsdom

import { flushSync, mount, unmount, type ComponentProps } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectSection, ProjectStatus, ProjectTask } from "$lib/projects/types";
import ProjectListView from "./ProjectListView.svelte";

const store = vi.hoisted(() => ({
  setTasksStatus: vi.fn<(tasks: unknown[], statusId: string) => Promise<void>>(),
}));

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({
    setTasksStatus: store.setTasksStatus,
    customFields: [],
    subtasksForTask: () => [],
    subtasksForTaskIncludingArchived: () => [],
    statusById: () => undefined,
    dependenciesBlockingTask: () => [],
    dependenciesBlockedByTask: () => [],
    tagsForTask: () => [],
    customFieldOptionsForField: () => [],
    customFieldValueForTask: () => undefined,
    customFieldOptionValuesForTask: () => [],
    eventLinksForTask: () => [],
  });
  return { getProjects, useProjects: getProjects };
});

vi.mock("$lib/stores/calendar.svelte", () => ({
  getCalendar: () => ({ events: [] }),
}));

vi.mock("$lib/stores/theme.svelte", async () => {
  const { lightTheme } = await import("$lib/themes");
  return { getTheme: () => ({ current: lightTheme }) };
});

vi.mock("$lib/vault/config", () => ({ensureConfigLoaded: async () => undefined, getConfigKey: <T>(_key: string, fallback: T) => fallback, setConfigKey: () => undefined, configKeyChanged: () => false, onConfigChangedElsewhere: () => () => undefined}));

type ListProps = ComponentProps<typeof ProjectListView>;

const PROJECT_ID = "project";

function status(id: string, name: string): ProjectStatus {
  return { id, projectId: PROJECT_ID, name, category: "active", color: 0, orderKey: id, terminal: false, createdAt: "", updatedAt: "" };
}

function task(id: string): ProjectTask {
  return {
    id, projectId: PROJECT_ID, sectionId: "section", statusId: "todo", title: id, description: "", priority: "none",
    taskType: "task", sectionOrderKey: id, statusOrderKey: id, milestone: false, createdAt: "", updatedAt: "",
  };
}

const section: ProjectSection = { id: "section", projectId: PROJECT_ID, name: "Section", orderKey: "1000", createdAt: "", updatedAt: "" } as ProjectSection;
const statuses = [status("todo", "To do"), status("done", "Done")];
const tasks = [task("task-a"), task("task-b")];

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function render(): void {
  const noop = () => undefined;
  const props: ListProps = {
    selectedProjectId: PROJECT_ID,
    sections: [section],
    statuses,
    priorities: [],
    tasks,
    allProjectTasks: tasks,
    listTaskGroups: [],
    taskGroupBy: "section",
    taskSortMode: "manual",
    taskSortDirection: "asc",
    taskListColumns: ["status"],
    taskListColumnWidths: {},
    projectCustomFields: [],
    selectedTaskId: null,
    selectedTaskIds: [],
    showArchivedTasks: false,
    onOpenTask: noop,
    onSelectedTaskIdsChange: noop,
    onTaskListColumnWidthsChange: async () => undefined,
    onNeedMore: noop,
  };
  component = mount(ProjectListView, { target, props });
  flushSync();
}

/** The status cell trigger of one task row, in row order. */
function statusTrigger(taskId: string): HTMLButtonElement {
  const triggers = target.querySelectorAll<HTMLButtonElement>('[data-list-status-menu-root="true"] button[aria-haspopup="menu"]');
  const button = triggers[tasks.findIndex((item) => item.id === taskId)];
  if (!button) throw new Error(`No status trigger for ${taskId}`);
  return button;
}

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  store.setTasksStatus.mockReset();
  vi.stubGlobal("ResizeObserver", class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  });
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe("ProjectListView cell menus", () => {
  it("keeps another row's status menu open when an earlier status save finishes", async () => {
    let finish: () => void = () => undefined;
    store.setTasksStatus.mockReturnValue(new Promise<void>((resolve) => { finish = resolve; }));
    render();

    statusTrigger("task-a").click();
    flushSync();
    const done = [...target.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')].find((item) => item.textContent?.includes("Done"));
    done?.click();
    flushSync();
    expect(store.setTasksStatus).toHaveBeenCalledOnce();

    statusTrigger("task-b").click();
    flushSync();
    expect(statusTrigger("task-b").getAttribute("aria-expanded")).toBe("true");

    finish();
    await Promise.resolve();
    await Promise.resolve();
    flushSync();

    expect(statusTrigger("task-b").getAttribute("aria-expanded")).toBe("true");
  });
});
