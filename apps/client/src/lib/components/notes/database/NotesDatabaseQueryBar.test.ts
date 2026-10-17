// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDatabaseTableFilter } from "$lib/notes/types";
import type { NotesDatabaseQueryProperty } from "$lib/notes/database/query-controls";
import NotesDatabaseQueryBar from "./NotesDatabaseQueryBar.svelte";
import NotesDatabaseQueryBarHarness from "./NotesDatabaseQueryBarHarness.test.svelte";

const properties: NotesDatabaseQueryProperty[] = [
  { id: "title", name: "Name", type: "title" },
  { id: "done", name: "Done", type: "checkbox" },
  { id: "estimate", name: "Estimate", type: "number" },
  { id: "due", name: "Due", type: "date" },
];

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
});

describe("Notes database applied query controls", () => {
  it("shows one chip per property and edits only that property's predicate", async () => {
    const filters: NotesDatabaseTableFilter[] = [
      { property_id: "title", condition: "contains", value: "Budget" },
      { property_id: "done", condition: "checked", value: null },
    ];
    const onFiltersChange = vi.fn<(filters: NotesDatabaseTableFilter[]) => void>();
    component = mount(NotesDatabaseQueryBar, { target: document.body, props: {
      properties, filters, sorts: [], onFiltersChange, onSortsChange: vi.fn(),
    } });
    expect(document.querySelectorAll("[data-notes-database-query-bar] button")).toHaveLength(2);
    document.querySelector<HTMLButtonElement>('button[aria-label="Filter: Name"]')!.click();
    await tick();
    await tick();
    const input = document.querySelector<HTMLInputElement>('[role="dialog"][aria-label="Name"] input[aria-label="Filter value"]')!;
    input.value = "Planning";
    input.dispatchEvent(new FocusEvent("blur"));
    expect(onFiltersChange).toHaveBeenCalledWith([
      { property_id: "title", condition: "contains", value: "Planning" }, filters[1],
    ]);
  });

  it("removes the active strip when its final filter is cleared", async () => {
    const filters: NotesDatabaseTableFilter[] = [{ property_id: "title", condition: "contains", value: "Budget" }];
    component = mount(NotesDatabaseQueryBarHarness, { target: document.body, props: {
      properties, initialFilters: filters,
    } });
    expect(document.querySelector("[data-notes-database-query-bar]")).not.toBeNull();
    document.querySelector<HTMLButtonElement>('button[aria-label="Filter: Name"]')!.click();
    await tick();
    await tick();
    document.querySelector<HTMLButtonElement>('[role="dialog"] button[aria-label="Remove filter"]')!.click();
    await tick();
    expect(document.querySelector("[data-notes-database-query-bar]")).toBeNull();
  });

  it("edits a numeric predicate inside a combined group chip without flattening its OR structure", async () => {
    const filters: NotesDatabaseTableFilter[] = [{ type: "or", filters: [
      { property_id: "estimate", condition: "greater_than", value: 10 },
      { property_id: "done", condition: "checked", value: null },
    ] }];
    const onFiltersChange = vi.fn<(filters: NotesDatabaseTableFilter[]) => void>();
    component = mount(NotesDatabaseQueryBar, { target: document.body, props: {
      properties, filters, sorts: [], onFiltersChange, onSortsChange: vi.fn(),
    } });
    expect(document.querySelectorAll("[data-notes-database-query-bar] button")).toHaveLength(1);
    document.querySelector<HTMLButtonElement>('button[aria-label="Filters: Estimate and Done"]')!.click();
    await tick(); await tick();
    expect(document.querySelector('[data-notes-filter-group="or"]')).not.toBeNull();
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Filter value"]')!;
    expect(input.type).toBe("number");
    input.value = "25.5";
    input.dispatchEvent(new FocusEvent("blur"));
    expect(onFiltersChange).toHaveBeenCalledWith([{ type: "or", filters: [
      { property_id: "estimate", condition: "greater_than", value: 25.5 },
      { property_id: "done", condition: "checked", value: null },
    ] }]);
    onFiltersChange.mockClear();
    input.value = "";
    input.dispatchEvent(new FocusEvent("blur"));
    expect(onFiltersChange).not.toHaveBeenCalled();
    expect(input.validationMessage).toBe("Enter a valid number.");
  });

  it("creates nested groups and removes only their selected leaf", async () => {
    component = mount(NotesDatabaseQueryBarHarness, { target: document.body, props: {
      properties, initialFilters: [{ type: "or", filters: [
        { property_id: "title", condition: "contains", value: "Budget" },
        { property_id: "done", condition: "checked", value: null },
      ] }],
    } });
    document.querySelector<HTMLButtonElement>('button[aria-label="Filters: Name and Done"]')!.click();
    await tick(); await tick();
    const group = document.querySelector('[data-notes-filter-group="or"]')!;
    Array.from(group.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.includes("Add group"))!.click();
    await tick();
    expect(document.querySelector('[data-notes-filter-group="or"] [data-notes-filter-group="and"]')).not.toBeNull();
    document.querySelector<HTMLButtonElement>('[data-notes-filter-group="and"] button[aria-label="Remove filter"]')!.click();
    await tick();
    expect(document.querySelector('[data-notes-filter-group="and"]')).toBeNull();
    expect(document.querySelectorAll("[data-notes-filter-predicate]")).toHaveLength(2);
    expect(document.querySelector('[data-notes-filter-group="or"]')).not.toBeNull();
  });
});
