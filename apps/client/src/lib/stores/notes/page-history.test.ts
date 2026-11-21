import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NotesLoadedPage, NotesPaginatedBlockList } from "$lib/notes/types";
import type { NotesPostMutationResult } from "$lib/notes/post-mutation";

const backend = vi.hoisted(() => ({
  restores: [] as Array<{ pageId: string; resolve: (page: NotesLoadedPage) => void; reject: (error: Error) => void }>,
  copies: [] as Array<{ pageId: string; resolve: (list: NotesPaginatedBlockList) => void; reject: (error: Error) => void }>,
}));

vi.mock("$lib/api/notes", () => ({
  copyNotesPageHistoryBlocks: (pageId: string) => new Promise((resolve, reject) => backend.copies.push({ pageId, resolve, reject })),
  getNotesPageHistorySettings: vi.fn(),
  listNotesPageHistorySnapshots: vi.fn(),
  loadNotesPageHistorySnapshot: vi.fn(),
  restoreNotesPageHistorySnapshot: (pageId: string) => (
    new Promise((resolve, reject) => backend.restores.push({ pageId, resolve, reject }))
  ),
  updateNotesPageHistorySettings: vi.fn(),
}));

function loadedPage(pageId: string): NotesLoadedPage {
  return { page: { id: pageId } } as NotesLoadedPage;
}

const emptyList = { results: [] } as unknown as NotesPaginatedBlockList;

async function setup(initialPageId = "page-a") {
  const { createNotesPageHistoryController } = await import("./page-history.svelte");
  let selectedPageId: string | null = initialPageId;
  const applyPostMutation = vi.fn<(result: NotesPostMutationResult) => void>();
  const requestPageLoadFocus = vi.fn();
  const setLoadError = vi.fn<(message: string) => void>();
  const controller = createNotesPageHistoryController({
    readSelectedPageId: () => selectedPageId,
    applyPostMutation,
    requestPageLoadFocus,
    flushPendingBlockSaves: async () => undefined,
    setLoadError,
  });
  return {
    controller,
    applyPostMutation,
    requestPageLoadFocus,
    setLoadError,
    select(pageId: string | null) {
      selectedPageId = pageId;
      controller.resetPageState();
    },
  };
}

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("page history actions", () => {
  beforeEach(() => {
    backend.restores.length = 0;
    backend.copies.length = 0;
  });

  it("applies a restore to the page it was started on", async () => {
    const h = await setup();
    const restoring = h.controller.restoreVersion("snapshot");
    await settle();
    expect(h.controller.actionLoading).toBe(true);
    backend.restores[0]?.resolve(loadedPage("page-a"));
    await restoring;
    expect(h.applyPostMutation).toHaveBeenCalledWith(expect.objectContaining({ loadedPage: loadedPage("page-a") }));
    expect(h.requestPageLoadFocus).toHaveBeenCalledOnce();
    expect(h.controller.actionLoading).toBe(false);
  });

  it("sends one restore for a double click", async () => {
    const h = await setup();
    const first = h.controller.restoreVersion("snapshot");
    const second = h.controller.restoreVersion("snapshot");
    await settle();
    expect(backend.restores).toHaveLength(1);
    backend.restores[0]?.resolve(loadedPage("page-a"));
    await Promise.all([first, second]);
    expect(h.applyPostMutation).toHaveBeenCalledOnce();
  });

  it("does not open a restored version over another page selected meanwhile", async () => {
    const h = await setup();
    const restoring = h.controller.restoreVersion("snapshot");
    await settle();
    h.select("page-b");
    backend.restores[0]?.resolve(loadedPage("page-a"));
    await restoring;
    expect(h.applyPostMutation).not.toHaveBeenCalledWith(expect.objectContaining({ loadedPage: expect.anything() }));
    expect(h.applyPostMutation).toHaveBeenCalledWith({ pages: [loadedPage("page-a").page], sidebarImpact: "visible-metadata" });
    expect(h.requestPageLoadFocus).not.toHaveBeenCalled();
  });

  it("lets a new page start an action while an earlier page's restore is still running", async () => {
    const h = await setup();
    const restoring = h.controller.restoreVersion("snapshot");
    await settle();
    h.select("page-b");
    const copying = h.controller.copyBlocks("snapshot-b");
    await settle();
    expect(backend.copies).toHaveLength(1);
    backend.restores[0]?.resolve(loadedPage("page-a"));
    await restoring;
    expect(h.controller.actionLoading).toBe(true);
    backend.copies[0]?.resolve(emptyList);
    await copying;
    expect(h.controller.actionLoading).toBe(false);
    expect(h.requestPageLoadFocus).toHaveBeenCalledOnce();
  });

  it("drops a copy result and its failure once the page changed", async () => {
    const h = await setup();
    const copying = h.controller.copyBlocks("snapshot");
    await settle();
    h.select("page-b");
    backend.copies[0]?.reject(new Error("Copy failed"));
    await copying;
    expect(h.controller.actionError).toBeNull();
    expect(h.setLoadError).not.toHaveBeenCalled();
    expect(h.applyPostMutation).not.toHaveBeenCalled();
  });

  it("reports a failure of the current action", async () => {
    const h = await setup();
    const copying = h.controller.copyBlocks("snapshot");
    await settle();
    backend.copies[0]?.reject(new Error("Copy failed"));
    await copying;
    expect(h.controller.actionError).toBe("Copy failed");
    expect(h.setLoadError).toHaveBeenCalledWith("Copy failed");
    expect(h.controller.actionLoading).toBe(false);
  });
});
