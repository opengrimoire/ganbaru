// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { listNotesDataSourceRowPages } from "$lib/api/notes";
import type { NotesPage } from "$lib/notes/types";
import NotesDatabaseRowPicker from "./NotesDatabaseRowPicker.svelte";

vi.mock("$lib/api/notes", () => ({ listNotesDataSourceRowPages: vi.fn() }));
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.resetAllMocks();
});

/** Build a source row titled with its id. */
function row(id: string): NotesPage {
  return {
    object: "page", id, created_time: "2026-09-25T00:00:00Z", last_edited_time: "2026-09-25T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null, in_trash: false,
    icon: null, cover: null, properties: { Name: { type: "title", title: [{ plain_text: id }] } },
    url: null, public_url: null, source_provider: null, source_object_id: null,
    source_workspace_id: null, source_last_edited_time: null,
  };
}

function optionLabels(): string[] {
  return [...document.querySelectorAll('[role="option"]')].map((option) => option.textContent?.trim() ?? "");
}

describe("Notes row picker sources", () => {
  it("offers only rows of the current source when the source changes during a load", async () => {
    const pending = new Map<string, (rows: NotesPage[]) => void>();
    vi.mocked(listNotesDataSourceRowPages).mockImplementation((sourceId: string) =>
      new Promise((resolve) => { pending.set(sourceId, resolve); }));
    const anchor = document.createElement("button");
    const target = document.createElement("div");
    document.body.append(anchor, target);
    const props = $state({
      anchor, dataSourceId: "first-source", sourceName: "Tasks",
      onPick: vi.fn(async () => {}), onClose: vi.fn(),
    });
    component = mount(NotesDatabaseRowPicker, { target, props });
    flushSync();
    pending.get("first-source")?.([row("first-row")]);
    await tick();
    await tick();
    expect(optionLabels()).toEqual(["first-row"]);

    props.dataSourceId = "second-source";
    flushSync();
    expect(optionLabels()).toEqual([]);
    expect(document.querySelector('[role="listbox"]')?.getAttribute("aria-busy")).toBe("true");
    pending.get("second-source")?.([row("second-row")]);
    await tick();
    await tick();
    expect(optionLabels()).toEqual(["second-row"]);
  });
});

describe("Notes row picker closing", () => {
  it("does not close a later picker when a pick finishes after this picker closed", async () => {
    vi.mocked(listNotesDataSourceRowPages).mockResolvedValue([row("first-row")]);
    let finishPick: () => void = () => undefined;
    const onPick = vi.fn(() => new Promise<void>((resolve) => { finishPick = resolve; }));
    const onClose = vi.fn();
    const anchor = document.createElement("button");
    const target = document.createElement("div");
    document.body.append(anchor, target);
    component = mount(NotesDatabaseRowPicker, {
      target,
      props: { anchor, dataSourceId: "source", sourceName: "Tasks", closeOnPick: true, onPick, onClose },
    });
    flushSync();
    await vi.waitFor(() => expect(optionLabels()).toEqual(["first-row"]));

    document.querySelector<HTMLElement>('[role="option"]')?.click();
    expect(onPick).toHaveBeenCalledTimes(1);
    await unmount(component);
    component = undefined;
    finishPick();
    await tick();
    await tick();

    expect(onClose).not.toHaveBeenCalled();
  });
});
