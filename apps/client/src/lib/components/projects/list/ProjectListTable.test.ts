// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getLocalization } from "$lib/i18n/translator.svelte";
import type { ProjectCustomField } from "$lib/projects/types";
import { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
import ProjectListTableHarness from "./ProjectListTableHarness.test.svelte";

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({ customFieldOptionsForField: () => [] });
  return { getProjects, useProjects: getProjects };
});
type Input = ConstructorParameters<typeof ProjectTaskQueryController>[0];
let component: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

beforeEach(() => {
  target = document.createElement("div");
  document.body.append(target);
  vi.stubGlobal("requestAnimationFrame", () => 1);
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

/** Supply a real query controller with only its native persistence boundaries mocked. */
async function renderTable() {
  const field: ProjectCustomField = {id: "points", projectId: "project", name: "Points", fieldType: "number", orderKey: "100", icon: null, createdAt: "", updatedAt: ""};
  const savePresentation = vi.fn<Input["projects"]["saveTaskListPresentation"]>().mockResolvedValue(undefined);
  const addField = vi.fn<Input["projects"]["addCustomField"]>().mockResolvedValue({...field, id: "created", name: "Budget"});
  const saveColumns = vi.fn<Input["projects"]["saveTaskListColumns"]>().mockResolvedValue(undefined);
  const query = new ProjectTaskQueryController({projects: {
    selectedProject: {id: "project"}, activeView: "list", activeTaskView: "list", isProjectOptionalDataLoaded: () => true, viewPreferences: [], activeProjectView: {id: "view-list", projectId: "project", viewType: "list"}, customFieldsForProject: () => [field], prioritiesForProject: () => [],
    saveTaskListPresentation: savePresentation, addCustomField: addField, saveTaskListColumns: saveColumns,
    isProjectDataLoaded: () => true, sectionsForProject: () => [],
    loadTaskView: async () => undefined, taskViewLoading: false, taskViewError: null,
    taskViewPage: { projectId: "project", view: "list", tasks: [], columnCalculations: [{column: "custom:points", total: 2500, filled: 1200, sum: 9000}] },
  } as unknown as Input["projects"], calendar: {sourceEvents: []} as unknown as Input["calendar"], translate: getLocalization().t});
  query.syncViewQuery();
  query.listColumns = ["custom:points", "due"];
  component = mount(ProjectListTableHarness, { target, props: {query} });
  await tick();
  return {query, savePresentation, addField, saveColumns};
}

/** Resolve an actual labelled control, including controls portaled into the owner. */
function button(label: string, owner: ParentNode = document): HTMLButtonElement {
  const result = Array.from(owner.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.getAttribute("aria-label") === label || entry.textContent?.trim() === label);
  if (!result) throw new Error(`Missing button: ${label}`);
  return result;
}

/** Open the property panel after initial focus settles in its header name field. */
async function openPoints(): Promise<HTMLElement> {
  button("Points").click();
  await vi.waitFor(() => {
    const panel = document.querySelector('[data-app-floating-surface][aria-label="Points"]');
    expect(panel).not.toBeNull();
    expect(document.activeElement).toBe(panel?.querySelector('[aria-label="Property name"]'));
  });
  return document.querySelector<HTMLElement>('[data-app-floating-surface][aria-label="Points"]')!;
}

/** Open a draft property header from the column menu once focus has moved into its name field. */
async function openInsert(panel: HTMLElement, label: "Insert left" | "Insert right"): Promise<HTMLElement> {
  button(label, panel).click();
  await vi.waitFor(() => {
    const draft = document.querySelector("[data-collection-property-draft]");
    expect(draft).not.toBeNull();
    expect(document.activeElement).toBe(draft?.querySelector('[aria-label="Property name"]'));
  });
  return document.querySelector<HTMLElement>("[data-collection-property-draft]")!;
}

/** Name a draft property in its header and choose a type from the panel below it. */
async function createProperty(creator: HTMLElement, name: string, type: string): Promise<void> {
  const input = creator.querySelector<HTMLInputElement>('[aria-label="Property name"]')!;
  input.value = name;
  input.dispatchEvent(new Event("input", {bubbles: true}));
  await tick();
  creator.querySelector<HTMLButtonElement>(`[data-property-type="${type}"]`)!.click();
}

describe("Projects table property configuration", () => {
  it("keeps header popovers in their owner and restores failed wrapping preferences with an explicit error", async () => {
    const {query, savePresentation} = await renderTable();
    let rejectWrite!: (error: unknown) => void;
    savePresentation.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { rejectWrite = reject; }));
    const panel = await openPoints();
    expect(panel.parentElement?.closest("[data-floating-root]")).toBe(target.firstElementChild);
    button("Wrap content").click();
    await tick();
    expect(query.listPresentation.wrappedColumns).toEqual(["custom:points"]);
    expect(button("Points").disabled).toBe(false);
    expect(button("Wrap content", panel).disabled).toBe(true);
    rejectWrite(new Error("Disk full"));
    await vi.waitFor(() => expect(query.presentationSaving).toBe(false));
    expect(query.listPresentation.wrappedColumns).toEqual([]);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("Disk full");
    expect(button("Wrap content", panel).disabled).toBe(false);
  });

  it("creates the chosen type beside the invoking column and reports a failed creation", async () => {
    const {query, addField, saveColumns} = await renderTable();
    addField.mockRejectedValueOnce(new Error("Vault unavailable"));
    const draft = await openInsert(await openPoints(), "Insert right");
    await createProperty(draft, "Budget", "number");
    await vi.waitFor(() => expect(query.propertySaving).toBe(false));
    expect(addField).toHaveBeenCalledWith("project", "Budget", "number", null);
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("Vault unavailable");
    expect(draft.isConnected).toBe(true);
    expect(saveColumns).not.toHaveBeenCalled();
    await createProperty(draft, "Budget", "number");
    await vi.waitFor(() => expect(saveColumns).toHaveBeenCalledWith("view-list", ["custom:points", "custom:created", "due"]));
    expect(query.propertyError).toBeNull();
    await vi.waitFor(() => expect(draft.isConnected).toBe(false));
  });

  it("names a blank property after its type and inserts it to the left", async () => {
    const {addField, saveColumns} = await renderTable();
    await createProperty(await openInsert(await openPoints(), "Insert left"), "  ", "text");
    await vi.waitFor(() => expect(saveColumns).toHaveBeenCalledWith("view-list", ["custom:created", "custom:points", "due"]));
    expect(addField).toHaveBeenCalledWith("project", "Text", "text", null);
  });

  it("adds a property after the last column from the header add button", async () => {
    const {addField, saveColumns} = await renderTable();
    const add = button("Add property");
    add.click();
    await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('[data-collection-property-draft] [aria-label="Property name"]')));
    expect(add.getAttribute("aria-expanded")).toBe("true");
    await createProperty(document.querySelector<HTMLElement>("[data-collection-property-draft]")!, "Budget", "number");
    await vi.waitFor(() => expect(saveColumns).toHaveBeenCalledWith("view-list", ["custom:points", "due", "custom:created"]));
    expect(addField).toHaveBeenCalledWith("project", "Budget", "number", null);
    await vi.waitFor(() => expect(document.querySelector("[data-collection-property-draft]")).toBeNull());
    expect(document.activeElement).toBe(add);
  });

  it("shows complete native reductions instead of deriving totals from loaded rows", async () => {
    const {query} = await renderTable();
    query.listPresentation.calculations = {"custom:points": "sum"};
    query.loadCurrent([], null);
    await vi.waitFor(() => expect(target.textContent).toContain("Sum: 9,000"));
    query.search = "New filter";
    await tick();
    expect(target.textContent).not.toContain("Sum: 9,000");
    expect(target.querySelector("[data-loading='inline'][role='status']")).not.toBeNull();
  });

  it("replaces the column menu with the draft and locks the column until the property is saved", async () => {
    const {query, addField, saveColumns} = await renderTable();
    let finishCreation!: (field: ProjectCustomField) => void;
    addField.mockImplementationOnce(() => new Promise<ProjectCustomField>((resolve) => { finishCreation = resolve; }));
    const panel = await openPoints();
    const draft = await openInsert(panel, "Insert right");
    expect(panel.isConnected).toBe(false);
    await createProperty(draft, "Budget", "number");
    await vi.waitFor(() => expect(button("Points").disabled).toBe(true));
    expect(draft.querySelector<HTMLInputElement>('[aria-label="Property name"]')!.disabled).toBe(true);
    button("Points").click();
    await tick();
    expect(document.querySelector('[data-app-floating-surface][aria-label="Points"]')).toBeNull();
    finishCreation({id: "created", projectId: "project", name: "Budget", fieldType: "number", orderKey: "100", createdAt: "", updatedAt: "", icon: null});
    await vi.waitFor(() => expect(query.propertySaving).toBe(false));
    expect(saveColumns).toHaveBeenCalledExactlyOnceWith("view-list", ["custom:points", "custom:created", "due"]);
    await vi.waitFor(() => expect(draft.isConnected).toBe(false));
    expect(document.activeElement).toBe(button("Points"));
    expect(button("Points").disabled).toBe(false);
  });
});

describe("Projects table column dragging", () => {
  /** Dispatch pointer coordinates using jsdom's mouse event implementation. */
  function pointer(eventTarget: EventTarget, type: string, x: number): void {
    const event = new MouseEvent(type, {clientX: x, button: 0, bubbles: true, cancelable: true});
    Object.defineProperty(event, "pointerId", {value: 1});
    eventTarget.dispatchEvent(event);
  }

  function headerCell(column: string): HTMLElement {
    const cell = document.querySelector<HTMLElement>(`[role="columnheader"][data-collection-column-id="${column}"]`);
    expect(cell).not.toBeNull();
    return cell!;
  }

  /** Place the column header cells side by side from x = 0 at their template widths, since jsdom has no layout. */
  function layOutHeader(): void {
    const widths: Record<string, number> = {name: 384, "custom:points": 128, due: 128};
    const row = headerCell("name").closest<HTMLElement>('[role="row"]')!;
    let left = 0;
    for (const cell of row.querySelectorAll<HTMLElement>("[data-collection-column-id]")) {
      const rect = new DOMRect(left, 0, widths[cell.getAttribute("data-collection-column-id") ?? ""] ?? 0, 32);
      cell.getBoundingClientRect = () => rect;
      left += rect.width;
    }
    const rowRect = new DOMRect(0, 0, left, 32);
    row.getBoundingClientRect = () => rowRect;
  }

  it("saves the dropped column order and keeps the release from opening the column menu", async () => {
    const {query, saveColumns} = await renderTable();
    layOutHeader();
    pointer(button("Due date"), "pointerdown", 600);
    pointer(window, "pointermove", 500);
    await tick();
    expect(headerCell("due").getAttribute("style")).toContain("translateX(-100px)");
    expect(headerCell("custom:points").getAttribute("style")).toContain("translateX(128px)");
    expect(saveColumns).not.toHaveBeenCalled();
    pointer(window, "pointerup", 500);
    button("Due date").click();
    await vi.waitFor(() => expect(query.listColumnsSaving).toBe(false));
    expect(saveColumns).toHaveBeenCalledExactlyOnceWith("view-list", ["due", "custom:points"]);
    expect(query.listColumns).toEqual(["due", "custom:points"]);
    expect(document.querySelector('[data-app-floating-surface][aria-label="Due date"]')).toBeNull();
  });

  it("keeps the name column in place", async () => {
    const {saveColumns} = await renderTable();
    layOutHeader();
    pointer(button("Name"), "pointerdown", 100);
    pointer(window, "pointermove", 600);
    await tick();
    expect(headerCell("name").hasAttribute("data-collection-column-dragged")).toBe(false);
    pointer(window, "pointerup", 600);
    expect(saveColumns).not.toHaveBeenCalled();
  });

  it("opens a column menu with a right click", async () => {
    await renderTable();
    const menu = new MouseEvent("contextmenu", {bubbles: true, cancelable: true, button: 2});
    button("Points").dispatchEvent(menu);
    await vi.waitFor(() => expect(document.querySelector('[data-app-floating-surface][aria-label="Points"]')).not.toBeNull());
    expect(menu.defaultPrevented).toBe(true);
  });
});
