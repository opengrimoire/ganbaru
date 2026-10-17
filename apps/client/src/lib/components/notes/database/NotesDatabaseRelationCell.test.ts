// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { listNotesDataSourceRowPages } from "$lib/api/notes";
import type { NotesPage } from "$lib/notes/types";
import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
import NotesDatabaseRelationCell from "./NotesDatabaseRelationCell.svelte";

vi.mock("$lib/api/notes", () => ({ listNotesDataSourceRowPages: vi.fn() }));
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.resetAllMocks();
});

/** Build a relation row with one existing target and ordinary page metadata. */
function row(id: string): NotesPage {
  return {
    object: "page", id, created_time: "2026-09-25T00:00:00Z", last_edited_time: "2026-09-25T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
    icon: null, cover: null, properties: {
      Name: { type: "title", title: [{ plain_text: id }] },
      Related: { id: "related", type: "relation", relation: [{ id: "existing" }] },
    },
    url: null, public_url: null, source_provider: null, source_object_id: null,
    source_workspace_id: null, source_last_edited_time: null,
  };
}
const column: NotesDatabaseTableColumn = {
  id: "related", name: "Related", type: "relation", hidden: false, width: 180, options: [],
  relationDataSourceId: "target-source", buttonLabel: "", buttonRequiresConfirmation: false,
};

describe("Notes relation dropdown", () => {
  it("opens during target loading and preserves existing relations when an option arrives", async () => {
    let resolveTargets: (rows: NotesPage[]) => void = () => { throw new Error("Request not started"); };
    vi.mocked(listNotesDataSourceRowPages).mockReturnValue(new Promise((resolve) => { resolveTargets = resolve; }));
    const target = document.createElement("div");
    document.body.append(target);
    const onSave = vi.fn();
    component = mount(NotesDatabaseRelationCell, { target, props: {
      row: row("current"), column, rowIndex: 2, columnIndex: 1, mutating: false,
      onSave, onNavigate: vi.fn(),
    } });
    const trigger = target.querySelector<HTMLButtonElement>('[data-table-cell="true"]')!;
    trigger.focus();
    await tick();
    expect(listNotesDataSourceRowPages).toHaveBeenCalledWith("target-source");
    expect(trigger.disabled).toBe(false);
    trigger.click();
    await tick();
    expect(document.querySelector('[role="listbox"]')).not.toBeNull();
    resolveTargets([row("existing"), row("new-target")]);
    await tick();
    await tick();
    const options = [...document.querySelectorAll<HTMLButtonElement>('[role="option"]')];
    expect(options.some((option) => option.textContent?.includes("existing"))).toBe(false);
    const next = options.find((option) => option.textContent?.includes("new-target"));
    expect(next).toBeDefined();
    next?.click();
    expect(onSave).toHaveBeenCalledWith(["existing", "new-target"]);
    expect(document.activeElement).toBe(trigger);
  });
});
