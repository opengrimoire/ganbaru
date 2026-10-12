// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesPage } from "$lib/notes/types";
import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
import NotesDatabaseNumberCell from "./NotesDatabaseNumberCell.svelte";

const row: NotesPage = {
  object: "page", id: "row", created_time: "2026-10-01T00:00:00Z", last_edited_time: "2026-10-01T00:00:00Z",
  parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
  icon: null, cover: null, properties: { Budget: { id: "budget", type: "number", number: 1234.5 } },
  url: null, public_url: null, source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
};
const column: NotesDatabaseTableColumn = { id: "budget", name: "Budget", type: "number", numberFormat: "dollar", hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => { if (component) await unmount(component); component = undefined; document.body.replaceChildren(); });

describe("Notes formatted number editing", () => {
  it("shows formatted money but edits raw scalar text and retains a rejected correction", async () => {
    const onSave = vi.fn(async () => false);
    component = mount(NotesDatabaseNumberCell, { target: document.body, props: { row, column, rowIndex: 0, columnIndex: 1, mutating: false, onSave, onNavigate: vi.fn() } });
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Budget"]')!;
    expect(input.value).toBe("$1,234.50");
    input.focus(); await tick();
    expect(input.value).toBe("1234.5");
    input.value = "27.5"; input.dispatchEvent(new Event("input", { bubbles: true }));
    input.blur(); await tick(); await tick();
    expect(onSave).toHaveBeenCalledWith("27.5");
    expect(input.value).toBe("27.5");
  });
});
