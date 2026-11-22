// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getQuickNote, listQuickNotes, listQuickNoteTags } from "$lib/api/quick-notes";
import type { QuickNote } from "$lib/quick-notes/types";
import QuickNotesPanel from "./QuickNotesPanel.svelte";

vi.mock("$lib/api/quick-notes", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/quick-notes")>(),
  getQuickNote: vi.fn(),
  listQuickNotes: vi.fn(),
  listQuickNoteTags: vi.fn(),
}));

vi.mock("$lib/quick-notes/window-sync", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/quick-notes/window-sync")>(),
  listenForQuickNotesChanges: () => Promise.resolve(() => undefined),
}));

const note: QuickNote = {
  id: "note-a",
  title: "Groceries",
  bodyPlainText: "Milk",
  runs: [],
  previewTruncated: false,
  images: [],
  color: 0,
  tagId: null,
  pinned: false,
  archived: false,
  trashedAt: null,
  revision: 1,
  createdAt: "2026-11-20T10:00:00.000Z",
  updatedAt: "2026-11-20T10:00:00.000Z",
  hasConflict: false,
};

describe("QuickNotesPanel", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    });
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  function buttonWithText(text: string): HTMLButtonElement | null {
    return [...document.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes(text)) ?? null;
  }

  it("keeps a new note editor open when a note opened earlier finishes reading", async () => {
    vi.mocked(listQuickNoteTags).mockResolvedValue([]);
    vi.mocked(listQuickNotes).mockResolvedValue({ notes: [note], nextCursor: null });
    let finishRead: (read: QuickNote) => void = () => undefined;
    vi.mocked(getQuickNote).mockImplementation(() => new Promise<QuickNote>((resolve) => { finishRead = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(QuickNotesPanel, { target, props: { onClose: vi.fn() } });

    await vi.waitFor(() => expect(buttonWithText("Groceries")).not.toBeNull());
    buttonWithText("Groceries")?.click();
    expect(getQuickNote).toHaveBeenCalledOnce();
    buttonWithText("Take a quick note")?.click();
    await vi.waitFor(() => expect(document.querySelector('[role="dialog"][aria-label="New quick note"]')).not.toBeNull());

    finishRead(note);
    await tick();
    await tick();

    expect(document.querySelector('[role="dialog"][aria-label="New quick note"]')).not.toBeNull();
    expect(document.querySelector('[role="dialog"][aria-label="Edit quick note"]')).toBeNull();
  });
});
