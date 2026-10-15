// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectCustomField, ProjectCustomFieldOption } from "$lib/projects/types";
import ProjectListPropertyMenuHarness from "./ProjectListPropertyMenuHarness.test.svelte";

const store = vi.hoisted(() => ({
  options: [] as ProjectCustomFieldOption[],
  updateCustomField: vi.fn<(field: ProjectCustomField, update: {name?: string; icon?: string | null}) => Promise<void>>(),
  removeCustomField: vi.fn<(fieldId: string) => Promise<void>>(),
  addCustomFieldOption: vi.fn<(fieldId: string, name: string) => Promise<ProjectCustomFieldOption | undefined>>(),
  updateCustomFieldOption: vi.fn<(option: ProjectCustomFieldOption, patch: {name?: string}) => Promise<void>>(),
  moveCustomFieldOption: vi.fn<(option: ProjectCustomFieldOption, direction: -1 | 1) => Promise<void>>(),
  removeCustomFieldOption: vi.fn<(optionId: string) => Promise<void>>(),
}));
vi.mock("$lib/stores/projects.svelte", () => ({getProjects: () => ({
  customFieldOptionsForField: (fieldId: string) => store.options.filter((option) => option.fieldId === fieldId),
  customEmojis: [],
  updateCustomField: store.updateCustomField,
  removeCustomField: store.removeCustomField,
  addCustomFieldOption: store.addCustomFieldOption,
  updateCustomFieldOption: store.updateCustomFieldOption,
  moveCustomFieldOption: store.moveCustomFieldOption,
  removeCustomFieldOption: store.removeCustomFieldOption,
})}));
vi.mock("$lib/vault/config", () => ({ensureConfigLoaded: async () => undefined, getConfigKey: <T>(_key: string, fallback: T) => fallback, setConfigKey: () => undefined, configKeyChanged: () => false, onConfigChangedElsewhere: () => () => undefined}));

let component: ReturnType<typeof mount> | undefined;
const initialField: ProjectCustomField = {id: "points", projectId: "project", name: "Points", fieldType: "number", sortOrder: 100, icon: null, createdAt: "", updatedAt: ""};

beforeEach(() => {
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
  vi.resetAllMocks();
  store.options = [];
  vi.unstubAllGlobals();
});

/** Resolve a labelled control in its actual floating owner. */
function button(label: string, owner: ParentNode = document): HTMLButtonElement {
  const result = Array.from(owner.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.getAttribute("aria-label") === label || entry.textContent?.trim() === label);
  if (!result) throw new Error(`Missing button: ${label}`);
  return result;
}

/** Open the actual property menu and return its header name field after initial focus settles there. */
async function openName(label: string): Promise<HTMLInputElement> {
  button(label).click();
  await vi.waitFor(() => {
    const panel = document.querySelector(`[data-app-floating-surface][aria-label="${label}"]`);
    const input = panel?.querySelector<HTMLInputElement>('[aria-label="Property name"]');
    expect(input).toBeTruthy();
    expect(document.activeElement).toBe(input);
  });
  return document.querySelector<HTMLInputElement>('[aria-label="Property name"]')!;
}

describe("Projects property-name drafts", () => {
  it("refreshes a pristine closed draft after a canonical rename and preserves an active edit", async () => {
    const instance = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField}});
    component = instance;
    await tick();
    instance.setField({...initialField, name: "Renamed"});
    await tick();
    const input = await openName("Renamed");
    expect(input.value).toBe("Renamed");
    input.value = "Local correction";
    input.dispatchEvent(new Event("input", {bubbles: true}));
    await tick();
    instance.setField({...initialField, name: "Another canonical name"});
    await tick();
    expect(input.value).toBe("Local correction");
  });

  it("retains a rejected draft across canonical updates and resets when the actual property changes", async () => {
    store.updateCustomField.mockRejectedValueOnce(new Error("Disk full"));
    const instance = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField}});
    component = instance;
    await tick();
    const input = await openName("Points");
    input.value = "Unsaved correction";
    input.dispatchEvent(new Event("input", {bubbles: true}));
    await tick();
    input.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter", bubbles: true, cancelable: true}));
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toBe("Could not rename property: Disk full"));
    expect(store.updateCustomField).toHaveBeenCalledWith(expect.objectContaining({id: "points"}), {name: "Unsaved correction"});
    instance.setField({...initialField, name: "Renamed elsewhere"});
    await tick();
    expect(input.value).toBe("Unsaved correction");
    instance.setField({...initialField, id: "budget", name: "Budget"});
    await tick();
    expect(input.value).toBe("Budget");
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });
});

describe("Projects property icons", () => {
  it("removes a custom icon from a nested picker without closing the property menu", async () => {
    store.updateCustomField.mockResolvedValue(undefined);
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField: {...initialField, icon: "lucide:star"}}});
    await tick();
    await openName("Points");
    const menu = document.querySelector<HTMLElement>('[data-app-floating-surface][aria-label="Points"]')!;
    const iconButton = button("Property icon", menu);
    expect(iconButton.disabled).toBe(false);
    iconButton.click();
    // The picker loads on first use and replaces the placeholder trigger with its own.
    await vi.waitFor(() => expect(button("Property icon", menu).getAttribute("aria-expanded")).toBe("true"));
    button("Remove").click();
    await vi.waitFor(() => expect(store.updateCustomField).toHaveBeenCalledWith(expect.objectContaining({id: "points"}), {icon: null}));
    expect(menu.isConnected).toBe(true);
  });

  it("keeps built-in column names and icons fixed", async () => {
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField, column: "due"}});
    await tick();
    const input = await openName("due");
    expect(input.readOnly).toBe(true);
    const iconButton = button("Property icon", input.closest<HTMLElement>("[data-app-floating-surface]")!);
    expect(iconButton.disabled).toBe(true);
    expect(iconButton.hasAttribute("aria-haspopup")).toBe(true);
    iconButton.click();
    await tick();
    expect(iconButton.getAttribute("aria-expanded")).toBe("false");
    expect(store.updateCustomField).not.toHaveBeenCalled();
  });
});

/** Open a submenu of a floating panel by its trigger name and return the submenu panel, which may carry a shorter name. */
async function submenu(panel: ParentNode, label: string, panelLabel = label): Promise<HTMLElement> {
  button(label, panel).click();
  await tick();
  await tick();
  const opened = document.querySelector<HTMLElement>(`[role="dialog"][aria-label="${panelLabel}"]`);
  if (!opened) throw new Error(`Missing submenu: ${panelLabel}`);
  return opened;
}

/** Open a column menu and return its floating panel. */
async function openMenu(label: string): Promise<HTMLElement> {
  const input = await openName(label);
  return input.closest<HTMLElement>("[data-app-floating-surface]")!;
}

/** Type into a field and press Enter the way a person saves it. */
async function enter(input: HTMLInputElement, value: string): Promise<void> {
  input.value = value;
  input.dispatchEvent(new Event("input", {bubbles: true}));
  await tick();
  input.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter", bubbles: true, cancelable: true}));
  await tick();
}

describe("Projects property settings", () => {
  const stage: ProjectCustomField = {...initialField, id: "stage", name: "Stage", fieldType: "select"};
  const option = (id: string, name: string, sortOrder: number): ProjectCustomFieldOption => ({id, fieldId: "stage", name, sortOrder, createdAt: "", updatedAt: ""});

  it("adds options under Edit property and rejects a name another option already has", async () => {
    store.options = [option("idea", "Idea", 100)];
    store.addCustomFieldOption.mockResolvedValue(undefined);
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField: stage}});
    await tick();
    const edit = await submenu(await openMenu("Stage"), "Edit property");
    button("Add an option", edit).click();
    await tick();
    const input = edit.querySelector<HTMLInputElement>('input[aria-label="Option name"]')!;
    await enter(input, "Review");
    await vi.waitFor(() => expect(store.addCustomFieldOption).toHaveBeenCalledWith("stage", "Review"));
    await enter(input, " idea ");
    expect(edit.querySelector('[role="alert"]')?.textContent).toBe("An option with that name already exists.");
    expect(store.addCustomFieldOption).toHaveBeenCalledTimes(1);
    expect(input.value).toBe(" idea ");
  });
  it("renames, reorders, and deletes an option without a save step", async () => {
    store.options = [option("idea", "Idea", 100), option("done", "Done", 200)];
    store.updateCustomFieldOption.mockResolvedValue(undefined);
    store.moveCustomFieldOption.mockImplementation(async (moved, direction) => {
      const from = store.options.findIndex((candidate) => candidate.id === moved.id);
      const next = [...store.options];
      [next[from], next[from + direction]] = [next[from + direction], next[from]];
      store.options = next;
    });
    store.removeCustomFieldOption.mockResolvedValue(undefined);
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField: stage}});
    await tick();
    const edit = await submenu(await openMenu("Stage"), "Edit property");
    button("Reorder Done", edit).dispatchEvent(new KeyboardEvent("keydown", {key: "ArrowUp", bubbles: true, cancelable: true}));
    await vi.waitFor(() => expect(store.moveCustomFieldOption).toHaveBeenCalledWith(expect.objectContaining({id: "done"}), -1));
    await vi.waitFor(() => expect(document.activeElement).toBe(button("Reorder Done", edit)));
    expect(store.moveCustomFieldOption).toHaveBeenCalledTimes(1);
    expect(edit.querySelector('[role="alert"]')).toBeNull();
    const optionMenu = await submenu(edit, "Edit Idea", "Idea");
    await enter(optionMenu.querySelector<HTMLInputElement>("input")!, "Draft");
    await vi.waitFor(() => expect(store.updateCustomFieldOption).toHaveBeenCalledWith(expect.objectContaining({id: "idea"}), {name: "Draft"}));
    button("Delete option", optionMenu).click();
    await vi.waitFor(() => expect(store.removeCustomFieldOption).toHaveBeenCalledWith("idea"));
  });

  it("keeps date and time formats of a date column under Edit property", async () => {
    const instance = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField, column: "due"}});
    component = instance;
    await tick();
    const menu = await openMenu("due");
    expect(Array.from(menu.querySelectorAll("button")).some((entry) => entry.getAttribute("aria-label") === "Date format")).toBe(false);
    const edit = await submenu(menu, "Edit property");
    button("Time format", edit);
    const formats = await submenu(edit, "Date format");
    button("ISO date", formats).click();
    await vi.waitFor(() => expect(instance.taskQuery().listPresentation.dateFormats.due).toBe("iso"));
  });

  it("offers no Edit property for a column without settings", async () => {
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField: {...initialField, fieldType: "text"}}});
    await tick();
    const menu = await openMenu("Points");
    expect(() => button("Edit property", menu)).toThrow();
  });

  it("groups tasks by a column and returns to sections from the same column", async () => {
    const instance = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField, column: "status"}});
    component = instance;
    await tick();
    button("Group", await openMenu("status")).click();
    expect(instance.taskQuery().groupBy).toBe("status");
    await vi.waitFor(() => expect(document.querySelector('[data-app-floating-surface][aria-label="status"]')).toBeNull());
    button("Remove grouping", await openMenu("status")).click();
    expect(instance.taskQuery().groupBy).toBe("section");
  });

  it("offers no grouping for a custom field", async () => {
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField}});
    await tick();
    const menu = await openMenu("Points");
    expect(() => button("Group", menu)).toThrow();
  });

  it("confirms before deleting a custom field and shows a rejection", async () => {
    store.removeCustomField.mockRejectedValueOnce(new Error("Locked"));
    const instance = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField}});
    component = instance;
    await tick();
    button("Delete property", await openMenu("Points")).click();
    await vi.waitFor(() => expect(document.body.textContent).toContain("Delete Points?"));
    expect(store.removeCustomField).not.toHaveBeenCalled();
    const confirm = Array.from(document.querySelectorAll<HTMLButtonElement>("button")).find((entry) => entry.textContent?.trim().startsWith("Delete field"));
    confirm!.click();
    await vi.waitFor(() => expect(instance.taskQuery().propertyError).toBe("Could not delete custom field: Locked"));
    expect(store.removeCustomField).toHaveBeenCalledWith("points");
  });

  it("offers no delete for a built-in column", async () => {
    component = mount(ProjectListPropertyMenuHarness, {target: document.body, props: {initialField, column: "due"}});
    await tick();
    const menu = await openMenu("due");
    expect(() => button("Delete property", menu)).toThrow();
  });
});
