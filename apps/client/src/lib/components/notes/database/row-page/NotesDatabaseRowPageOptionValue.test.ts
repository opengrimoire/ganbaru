// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDataSourceSchemaOptionDraft } from "$lib/notes/database/data-source-schema";
import type { NotesDataSourceOptionEdit } from "$lib/notes/database/property-options";
import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
import type { NotesPage } from "$lib/notes/types";
import NotesDatabaseRowPageOptionValue from "./NotesDatabaseRowPageOptionValue.svelte";

type OptionColumnType = "select" | "status" | "multi_select";
type SaveValue = string | string[] | null;

const options: NotesDataSourceSchemaOptionDraft[] = [
  { id: "done", name: "Done", color: "green", group: "Complete" },
  { id: "idea", name: "Idea", color: "blue", group: "To-do" },
  { id: "doing", name: "Doing", color: "orange", group: "In progress" },
];

let component: ReturnType<typeof mount> | undefined;
afterEach(async () => { if (component) await unmount(component); component = undefined; document.body.replaceChildren(); });

function columnOf(type: OptionColumnType): NotesDatabaseTableColumn {
  return { id: "stage", name: "Stage", type, hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
}

function rowWith(type: OptionColumnType, names: string[]): NotesPage {
  const payload = type === "multi_select" ? names.map((name) => ({ name })) : names[0] ? { name: names[0] } : null;
  return {
    object: "page", id: "row", created_time: "2026-10-01T00:00:00Z", last_edited_time: "2026-10-01T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
    icon: null, cover: null, properties: { Stage: { id: "stage", type, [type]: payload } },
    url: null, public_url: null, source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
  };
}

function trigger(): HTMLButtonElement {
  return document.querySelector<HTMLButtonElement>('button[aria-label="Stage"][aria-haspopup="dialog"]')!;
}

function panel(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[role="dialog"]');
}

function optionNamed(name: string): HTMLElement {
  const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((element) => element.textContent?.includes(name));
  if (!option) throw new Error(`No option named ${name}`);
  return option;
}

/** Let the ordered save chain settle. */
async function settle(): Promise<void> {
  for (let step = 0; step < 4; step += 1) await tick();
}

/** Mount the value for a saved choice and open its option list. */
async function open(
  type: OptionColumnType,
  names: string[],
  onSave: (value: SaveValue) => Promise<boolean>,
  onCreate?: (name: string) => Promise<NotesDataSourceOptionEdit["status"]>,
): Promise<void> {
  component = mount(NotesDatabaseRowPageOptionValue, {
    target: document.body,
    props: { row: rowWith(type, names), column: columnOf(type), rowIndex: 0, columnIndex: 0, options, onSave, onCreate, onNavigate: vi.fn() },
  });
  trigger().click();
  await tick(); await tick();
}

async function search(text: string): Promise<void> {
  const input = document.querySelector<HTMLInputElement>('[role="combobox"]')!;
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await tick();
}

describe("Notes database row page option value", () => {
  it("saves a single choice by name and closes the list", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    await open("select", [], onSave);
    optionNamed("Doing").click();
    await settle();
    expect(onSave).toHaveBeenCalledWith("Doing");
    expect(panel()).toBeNull();
    expect(document.activeElement).toBe(trigger());
  });

  it("clears a single choice when its chosen option is picked again", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    await open("select", ["Idea"], onSave);
    optionNamed("Idea").click();
    await settle();
    expect(onSave).toHaveBeenCalledWith(null);
  });

  it("toggles multi-select options in pick order while the list stays open", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    await open("multi_select", ["Idea"], onSave);
    optionNamed("Done").click();
    await tick();
    optionNamed("Idea").click();
    await tick();
    expect(optionNamed("Done").getAttribute("aria-selected")).toBe("true");
    expect(optionNamed("Idea").getAttribute("aria-selected")).toBe("false");
    await settle();
    expect(onSave.mock.calls).toEqual([[["Idea", "Done"]], [["Done"]]]);
    expect(panel()).not.toBeNull();
  });

  it("creates a typed option and then chooses it", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    const onCreate = vi.fn(async (_name: string): Promise<NotesDataSourceOptionEdit["status"]> => "changed");
    await open("multi_select", [], onSave, onCreate);
    await search("  Blocked ");
    optionNamed("Create").click();
    await settle();
    expect(onCreate).toHaveBeenCalledWith("Blocked");
    expect(onSave).toHaveBeenCalledWith(["Blocked"]);
  });

  it("offers no create row for a name that matches an option in another case", async () => {
    const onCreate = vi.fn(async (_name: string): Promise<NotesDataSourceOptionEdit["status"]> => "changed");
    await open("select", [], vi.fn(async (_value: SaveValue) => true), onCreate);
    await search("doing");
    const labels = [...document.querySelectorAll('[role="option"]')].map((element) => element.textContent?.trim());
    expect(labels).toEqual(["Doing"]);
  });

  it("explains the option limit without saving", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    await open("select", [], onSave, async () => "limit");
    await search("Later");
    optionNamed("Create").click();
    await settle();
    expect(onSave).not.toHaveBeenCalled();
    expect(document.querySelector('[role="alert"]')?.textContent).toMatch(/\d/);
  });

  it("restores the saved names when the newest save fails", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => false);
    await open("multi_select", ["Idea"], onSave);
    optionNamed("Done").click();
    await settle();
    expect(trigger().textContent).toContain("Idea");
    expect(trigger().textContent).not.toContain("Done");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("could not be saved");
  });

  it("lists status options under their groups without a create row", async () => {
    await open("status", ["Doing"], vi.fn(async (_value: SaveValue) => true));
    await search("Brand new");
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(0);
    await search("");
    const list = document.querySelector('[role="listbox"]')!;
    const labels = [...list.children].map((element) => element.textContent?.trim());
    expect(labels).toEqual(["To-do", "Idea", "In progress", "Doing", "Complete", "Done"]);
  });

  it("removes the last multi-select choice with Backspace in an empty search", async () => {
    const onSave = vi.fn(async (_value: SaveValue) => true);
    await open("multi_select", ["Idea", "Done"], onSave);
    document.querySelector('[role="combobox"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true }));
    await settle();
    expect(onSave).toHaveBeenCalledWith(["Idea"]);
  });
});
