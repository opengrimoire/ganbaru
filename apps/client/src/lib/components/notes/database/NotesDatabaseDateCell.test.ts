// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesPage } from "$lib/notes/types";
import type { NotesDatabaseTableColumn } from "$lib/notes/database/table";
import type { NotesDatabaseDateValue } from "$lib/notes/database/date";
import NotesDatabaseDateCell from "./NotesDatabaseDateCell.svelte";

const column: NotesDatabaseTableColumn = { id: "when", name: "When", type: "date", hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => { if (component) await unmount(component); component = undefined; document.body.replaceChildren(); });

function rowWith(date: NotesDatabaseDateValue | null): NotesPage {
  return {
    object: "page", id: "row", created_time: "2026-10-01T00:00:00Z", last_edited_time: "2026-10-01T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
    icon: null, cover: null, properties: date ? { When: { id: "when", type: "date", date } } : {},
    url: null, public_url: null, source_provider: null, source_object_id: null, source_workspace_id: null, source_last_edited_time: null,
  };
}

function trigger(): HTMLButtonElement {
  return document.querySelector<HTMLButtonElement>('button[aria-label="When"][aria-haspopup="dialog"]')!;
}

/** Mount the cell for a saved value and open its editor. */
async function open(date: NotesDatabaseDateValue | null, onSave: (value: NotesDatabaseDateValue | null) => Promise<boolean>): Promise<void> {
  component = mount(NotesDatabaseDateCell, { target: document.body, props: { row: rowWith(date), column, rowIndex: 0, columnIndex: 1, mutating: false, onSave, onNavigate: vi.fn() } });
  trigger().click();
  await tick(); await tick();
}

async function pickDay(day: string): Promise<void> {
  document.querySelector<HTMLButtonElement>(`[role="dialog"] [data-date="${day}"]`)!.click();
  await tick(); await tick();
}

describe("Notes database date editor", () => {
  it("starts a single date from the first picked day", async () => {
    const onSave = vi.fn(async () => true);
    await open(null, onSave);
    const day = document.querySelector<HTMLButtonElement>('[role="dialog"] [data-date]')!.dataset.date!;
    await pickDay(day);
    expect(onSave).toHaveBeenCalledWith({ start: day, end: null, time_zone: null });
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it("picks a range start and then its end, saving each pick in order", async () => {
    const onSave = vi.fn(async () => true);
    await open({ start: "2026-10-01", end: "2026-10-03", time_zone: null }, onSave);
    expect(document.querySelector('[aria-pressed="true"]')?.textContent).toContain("Start");
    await pickDay("2026-10-02");
    expect(document.querySelector('[aria-pressed="true"]')?.textContent).toContain("End");
    await pickDay("2026-10-05");
    expect(onSave.mock.calls).toEqual([
      [{ start: "2026-10-02", end: "2026-10-03", time_zone: null }],
      [{ start: "2026-10-02", end: "2026-10-05", time_zone: null }],
    ]);
  });

  it("swaps the boundaries when the end is picked before the start", async () => {
    const onSave = vi.fn(async () => true);
    await open({ start: "2026-10-05", end: "2026-10-07", time_zone: null }, onSave);
    document.querySelectorAll<HTMLButtonElement>("[aria-pressed]")[1]!.click();
    await tick();
    await pickDay("2026-10-02");
    expect(onSave).toHaveBeenCalledWith({ start: "2026-10-02", end: "2026-10-05", time_zone: null });
  });

  it("turns a single timed date into a range ending at the same moment", async () => {
    const onSave = vi.fn(async () => true);
    const date = { start: "2026-10-01T09:00", end: null, time_zone: "America/Monterrey" };
    await open(date, onSave);
    document.querySelector<HTMLButtonElement>('[role="switch"][aria-label="End date"]')!.click();
    await tick();
    expect(onSave).toHaveBeenCalledWith({ ...date, end: "2026-10-01T09:00" });
  });

  it("restores the saved value and explains a rejected save", async () => {
    const onSave = vi.fn(async () => false);
    await open({ start: "2026-10-01", end: null, time_zone: null }, onSave);
    const before = trigger().textContent;
    await pickDay("2026-10-09");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("The date could not be saved.");
    expect(trigger().textContent).toBe(before);
  });

  it("clears the value and closes the editor", async () => {
    const onSave = vi.fn(async () => true);
    await open({ start: "2026-10-01", end: null, time_zone: null }, onSave);
    document.querySelector<HTMLButtonElement>('[role="dialog"] button[aria-label="Clear date"]')!.click();
    await tick(); await tick();
    expect(onSave).toHaveBeenCalledWith(null);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it("closes on Escape without saving", async () => {
    const onSave = vi.fn(async () => true);
    await open({ start: "2026-10-01", end: null, time_zone: null }, onSave);
    document.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(onSave).not.toHaveBeenCalled();
  });
});
