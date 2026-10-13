// @vitest-environment jsdom

import { mount, tick, unmount, type ComponentProps } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectCustomFieldOption } from "$lib/projects/types";
import type { ProjectToolbarPanel } from "$lib/projects/toolbar";
import { PROJECT_TASK_FILTER_DEFAULTS } from "$lib/projects/list/view";
import ProjectToolbarPanels from "./ProjectToolbarPanels.svelte";

const store = vi.hoisted(() => ({ options: [] as ProjectCustomFieldOption[] }));

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    customFieldOptionsForField: (fieldId: string) => store.options.filter((option) => option.fieldId === fieldId),
  }),
}));

vi.mock("$lib/stores/theme.svelte", async () => {
  const { lightTheme } = await import("$lib/themes");
  return { getTheme: () => ({ current: lightTheme }) };
});

vi.mock("$lib/components/projects/settings/ProjectSettingsPanel.svelte", () => ({
  // The draft editor owns separate behavior; this suite exercises its guarded shell.
  default: (_anchor: HTMLElement, _props: unknown) => undefined,
}));

type ToolbarProps = ComponentProps<typeof ProjectToolbarPanels>;

let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;
let trigger: HTMLButtonElement;

beforeEach(() => {
  target = document.createElement("div");
  target.style.overflow = "hidden";
  trigger = document.createElement("button");
  document.body.append(trigger, target);
  vi.stubGlobal("requestAnimationFrame", () => 1);
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  store.options = [];
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Supply canonical project query preferences and callbacks to the collection shell. */
function toolbarProps(panel: ProjectToolbarPanel, overrides: Partial<ToolbarProps> = {}): ToolbarProps {
  return {
    panel,
    projectId: "project-one",
    sections: [],
    priorities: [],
    projectTags: [],
    projectCustomFields: [],
    taskListColumnControls: [],
    archivedProjectTaskCount: 0,
    inactiveSectionCount: 0,
    taskFiltersActive: false,
    taskStatusFilter: PROJECT_TASK_FILTER_DEFAULTS.statusFilter,
    taskSectionFilter: PROJECT_TASK_FILTER_DEFAULTS.sectionFilter,
    taskPriorityFilter: PROJECT_TASK_FILTER_DEFAULTS.priorityFilter,
    taskDueFilter: PROJECT_TASK_FILTER_DEFAULTS.dueFilter,
    taskDueRangeStart: PROJECT_TASK_FILTER_DEFAULTS.dueRangeStart,
    taskDueRangeEnd: PROJECT_TASK_FILTER_DEFAULTS.dueRangeEnd,
    taskScheduleFilter: PROJECT_TASK_FILTER_DEFAULTS.scheduleFilter,
    taskDependencyFilter: PROJECT_TASK_FILTER_DEFAULTS.dependencyFilter,
    taskTagFilter: PROJECT_TASK_FILTER_DEFAULTS.tagFilter,
    taskCustomFieldFilters: [],
    taskGroupBy: PROJECT_TASK_FILTER_DEFAULTS.groupBy,
    taskSortMode: PROJECT_TASK_FILTER_DEFAULTS.sortMode,
    taskSortDirection: PROJECT_TASK_FILTER_DEFAULTS.sortDirection,
    showArchivedTasks: false,
    showInactiveSections: false,
    onClose: vi.fn(),
    onRevealInactive: vi.fn(),
    onProjectSettingsDirtyChange: vi.fn(),
    onClearTaskFilters: vi.fn(),
    onToggleTaskListColumn: vi.fn(),
    ...overrides,
  };
}

/** Open configuration from a real toolbar anchor outside the clipped content. */
async function open(props: ToolbarProps): Promise<void> {
  trigger.dataset.projectToolbarTrigger = props.panel ?? "";
  trigger.focus();
  component = mount(ProjectToolbarPanels, { target, props });
  await tick();
  if (props.panel !== "settings") {
    await vi.waitFor(() => {
      const firstControl = visiblePage().querySelector<HTMLButtonElement>("button:not(:disabled)");
      expect(firstControl).not.toBeNull();
      expect(document.activeElement).toBe(firstControl);
    });
  }
}

/** Resolve the one currently visible settings page. */
function visiblePage(): HTMLElement {
  const page = document.querySelector<HTMLElement>("[data-collection-settings-page]:not([hidden])");
  if (!page) throw new Error("The project settings page is not visible.");
  return page;
}

/** Select a visible option by its rendered label. */
async function choose(label: string): Promise<HTMLButtonElement> {
  const button = [...visiblePage().querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.textContent?.trim() === label);
  if (!button) throw new Error(`Project option is missing: ${label}`);
  button.click();
  await tick();
  await tick();
  return button;
}

/** Navigate through a configuration row while retaining one floating dialog. */
async function navigate(label: string): Promise<void> {
  const button = [...visiblePage().querySelectorAll<HTMLButtonElement>("button")]
    .find((candidate) => candidate.getAttribute("aria-label") === label);
  if (!button) throw new Error(`Project settings row is missing: ${label}`);
  button.click();
  await tick();
  await tick();
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
}

/** Return to the overview without closing collection configuration. */
async function back(): Promise<void> {
  const button = document.querySelector<HTMLButtonElement>('button[aria-label="Back"]');
  if (!button) throw new Error("The project settings Back control is missing.");
  button.click();
  await tick();
  await tick();
}

describe("Project collection configuration", () => {
  it("floats grouping outside clipped content and preserves selection before keyboard closure", async () => {
    const onClose = vi.fn();
    await open(toolbarProps("group", { onClose }));
    const panel = document.querySelector<HTMLElement>('[role="dialog"]');
    expect(panel?.parentElement).toBe(document.body);
    expect(target.querySelector('[role="dialog"]')).toBeNull();
    expect(panel?.style.position).toBe("fixed");
    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    expect(document.activeElement?.textContent?.trim()).toBe("Status");
    expect((await choose("Status")).getAttribute("aria-pressed")).toBe("true");
    expect(visiblePage().querySelector('[aria-pressed="true"]')?.textContent?.trim()).toBe("Status");

    document.activeElement?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(trigger);
  });

  it("retains status, custom field, sort, and direction choices across detail page navigation", async () => {
    const onClose = vi.fn();
    store.options = [{ id: "risk-high", fieldId: "risk", name: "High risk", sortOrder: 0, createdAt: "", updatedAt: "" }];
    await open(toolbarProps("filters", {
      onClose,
      projectCustomFields: [{ id: "risk", projectId: "project-one", name: "Risk", fieldType: "select", sortOrder: 0, icon: null, createdAt: "", updatedAt: "" }],
    }));
    await navigate("Status");
    await choose("Open");
    await back();
    expect(visiblePage().querySelector('[aria-label="Status"]')?.textContent).toContain("Open");
    await navigate("Risk");
    expect((await choose("High risk")).getAttribute("aria-pressed")).toBe("true");
    await back();
    expect(visiblePage().querySelector('[aria-label="Risk"]')?.textContent).toContain("High risk");
    await navigate("Sort");
    await choose("Due date");
    await choose("Asc");
    expect(visiblePage().textContent).toContain("Desc");
    await back();
    expect(visiblePage().querySelector('[aria-label="Sort"]')?.textContent).toContain("Due date");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("connects column writes to their project callback", async () => {
    const onToggleTaskListColumn = vi.fn();
    await open(toolbarProps("customize", {
      taskListColumnControls: [{ column: "status", label: "Status", visible: true }, { column: "due", label: "Due date", visible: false }],
      onToggleTaskListColumn,
    }));
    await navigate("Columns");
    await choose("Due date");
    expect(onToggleTaskListColumn).toHaveBeenCalledWith("due");
  });

  it("shows column save errors beside disabled pending controls", async () => {
    const onToggleTaskListColumn = vi.fn();
    await open(toolbarProps("customize", {
      listColumnsSaving: true,
      listColumnsError: "Could not save visible columns: Disk is full",
      taskListColumnControls: [{ column: "due", label: "Due date", visible: false }],
      onToggleTaskListColumn,
    }));
    await navigate("Columns");
    const column = [...visiblePage().querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "Due date")!;
    expect(column.disabled).toBe(true);
    column.click();
    expect(onToggleTaskListColumn).not.toHaveBeenCalled();
    expect(visiblePage().querySelector('[role="alert"]')?.textContent).toContain("Disk is full");
  });

  it("requests guarded project draft closure only after nested floating controls dismiss", async () => {
    const onClose = vi.fn();
    await open(toolbarProps("settings", { onClose }));
    const panel = document.querySelector<HTMLElement>('[role="dialog"]');
    if (!panel) throw new Error("The project draft shell is missing.");
    const picker = document.createElement("button");
    picker.dataset.appFloatingSurface = "";
    panel.append(picker);
    picker.focus();
    picker.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onClose).not.toHaveBeenCalled();
    picker.remove();
    panel.focus();
    panel.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
