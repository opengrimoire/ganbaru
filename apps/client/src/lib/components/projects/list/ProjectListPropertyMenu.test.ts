// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectCustomField } from "$lib/projects/types";
import ProjectListPropertyMenuHarness from "./ProjectListPropertyMenuHarness.test.svelte";

const store = vi.hoisted(() => ({updateCustomField: vi.fn<(field: ProjectCustomField, update: {name?: string; icon?: string | null}) => Promise<void>>() }));
vi.mock("$lib/stores/projects.svelte", () => ({getProjects: () => ({customFieldOptionsForField: () => [], customEmojis: [], updateCustomField: store.updateCustomField})}));
vi.mock("$lib/vault/config", () => ({ensureConfigLoaded: async () => undefined, getConfigKey: <T>(_key: string, fallback: T) => fallback, setConfigKey: () => undefined}));

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
