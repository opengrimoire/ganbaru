// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesBlockUpdate, NotesLoadedPage, NotesPage, NotesPageCreate, NotesPageOpenResponse, NotesWorkspaceShell } from "$lib/notes/types";

const rootId = "00000000-0000-4000-8000-000000000001";
const childId = "00000000-0000-4000-8000-000000000002";
const otherId = "00000000-0000-4000-8000-000000000003";
const now = "2026-09-28T12:00:00.000Z";

const backend = vi.hoisted(() => ({
  pages: new Map<string, NotesPage>(),
  openCalls: [] as string[],
  deferredOpenPageId: null as string | null,
  pendingOpens: [] as Array<{ reject: (error: Error) => void }>,
  pendingCreates: [] as Array<{
    pageId: string;
    resolve: (page: NotesLoadedPage) => void;
    reject: (error: Error) => void;
  }>,
  pendingTrashes: [] as Array<{
    resolve: (page: NotesPage) => void;
    reject: (error: Error) => void;
  }>,
  pendingBlockSaves: [] as Array<{
    resolve: (block: NotesBlock) => void;
    reject: (error: Error) => void;
  }>,
}));

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (_key: string, fallback: unknown) => fallback,
  setConfigKey: vi.fn(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    loadNotesWorkspaceShell: async (): Promise<NotesWorkspaceShell> => ({
      pages: [...backend.pages.values()],
      folders: [],
      navigation_pages: [],
      navigation_folders: [],
      navigation_page_ids_with_children: [],
      navigation_databases: [],
      page_ids_with_children: [],
      missing_parent_page_ids: [],
      trashed_parent_page_ids: [],
      resolved_selected_page_id: null,
      total_page_count: backend.pages.size,
      total_folder_count: 0,
      next_page_cursor: null,
      next_folder_cursor: null,
    }),
    openNotesPage: (pageId: string): Promise<NotesPageOpenResponse> => {
      backend.openCalls.push(pageId);
      if (pageId === backend.deferredOpenPageId) {
        return new Promise((_, reject) => backend.pendingOpens.push({ reject }));
      }
      const page = backend.pages.get(pageId);
      if (!page) return Promise.reject(new Error("notes page not found"));
      return Promise.resolve(openResponse(page));
    },
    createNotesPage: (request: NotesPageCreate): Promise<NotesLoadedPage> => (
      new Promise((resolve, reject) => backend.pendingCreates.push({ pageId: request.id, resolve, reject }))
    ),
    getNotesPageBreadcrumb: async () => [],
    getNotesBlockOutlineFrontier: async () => [],
    updateNotesBlock: (_blockId: string, _update: NotesBlockUpdate): Promise<NotesBlock> => (
      new Promise((resolve, reject) => backend.pendingBlockSaves.push({ resolve, reject }))
    ),
    saveNotesUndoState: async () => undefined,
    isNotesPageActive: async (pageId: string) => backend.pages.has(pageId),
    trashNotesPage: (_pageId: string, _inTrash: boolean): Promise<NotesPage> => (
      new Promise((resolve, reject) => backend.pendingTrashes.push({ resolve, reject }))
    ),
  };
});

function page(id: string, parent: NotesPage["parent"] = { type: "workspace", workspace: true }): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent,
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: { title: { id: "title", type: "title", title: [], plain_text: id } },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function openResponse(pageValue: NotesPage): NotesPageOpenResponse {
  const write = createBlockWrite(`block-${pageValue.id}`, "paragraph", "Text");
  if (write.type !== "paragraph") throw new Error("expected paragraph");
  const block: NotesBlock = {
    object: "block",
    id: write.id,
    parent: { type: "page_id", page_id: pageValue.id },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
  return {
    page: pageValue,
    blocks: { object: "list", type: "block", block: {}, results: [block], next_cursor: null, has_more: false },
    breadcrumb: pageValue.id === childId
      ? [
        { id: rootId, title: rootId, current: false, status: "active" },
        { id: childId, title: childId, current: true, status: "active" },
      ]
      : [],
    outlines: [],
  };
}

describe("Notes page lifecycle", () => {
  beforeEach(() => {
    vi.resetModules();
    backend.pages.clear();
    backend.pages.set(rootId, page(rootId));
    backend.pages.set(childId, page(childId, { type: "page_id", page_id: rootId }));
    backend.pages.set(otherId, page(otherId));
    backend.openCalls.length = 0;
    backend.deferredOpenPageId = null;
    backend.pendingOpens.length = 0;
    backend.pendingCreates.length = 0;
    backend.pendingTrashes.length = 0;
    backend.pendingBlockSaves.length = 0;
  });

  it("opens rapidly created notes from their local pages without a missing-page request", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.createPage("First");
    const firstId = notes.selectedPageId;
    if (!firstId) throw new Error("missing first provisional page");
    await notes.createPage("Second");
    const secondId = notes.selectedPageId;
    if (!secondId) throw new Error("missing second provisional page");
    expect(backend.pendingCreates.map((entry) => entry.pageId)).toEqual([firstId, secondId]);

    await notes.selectPage(firstId);
    expect(notes.loadedPage?.id).toBe(firstId);
    expect(notes.primaryContentReady).toBe(true);
    expect(notes.loadError).toBeNull();
    expect(backend.openCalls).not.toContain(firstId);

    for (const id of [secondId, firstId]) {
      const created = page(id);
      backend.pages.set(id, created);
      const response = openResponse(created);
      backend.pendingCreates.find((entry) => entry.pageId === id)?.resolve({
        page: response.page,
        blocks: response.blocks,
      });
    }
    await vi.waitFor(() => expect(notes.pageCreationPending).toBe(false));
    await vi.waitFor(() => expect(notes.allPages.find((item) => item.id === secondId)?.last_edited_time).toBe(now));

    await notes.selectPage(secondId);
    expect(notes.loadedPage?.id).toBe(secondId);
    expect(notes.primaryContentReady).toBe(true);
    expect(notes.loadError).toBeNull();
    expect(backend.openCalls).not.toContain(secondId);
  });

  it("reopens a failed local creation as a draft with a retry error", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.createPage("First");
    const firstId = notes.selectedPageId;
    if (!firstId) throw new Error("missing first provisional page");
    await notes.createPage("Second");
    const secondId = notes.selectedPageId;
    if (!secondId) throw new Error("missing second provisional page");
    await notes.selectPage(firstId);
    backend.pendingCreates.find((entry) => entry.pageId === firstId)?.reject(new Error("disk full"));
    await vi.waitFor(() => expect(notes.pageCreationError).toBe("disk full"));

    await notes.selectPage(secondId);
    await notes.selectPage(firstId);
    expect(notes.loadedPage?.id).toBe(firstId);
    expect(notes.loadError).toBeNull();
    expect(notes.pageCreationError).toBe("disk full");
    expect(backend.openCalls).not.toContain(firstId);
  });

  it("settles edits on the current note before creating the next note", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(rootId);
    await notes.updateBlockText(`block-${rootId}`, "Changed");

    const creation = notes.createPage("Next");
    expect(notes.selectedPageId).toBe(rootId);
    await vi.waitFor(() => expect(backend.pendingBlockSaves).toHaveLength(1));
    expect(backend.pendingCreates).toHaveLength(0);
    const root = backend.pages.get(rootId);
    if (!root) throw new Error("missing root fixture");
    const savedBlock = openResponse(root).blocks.results[0];
    if (!savedBlock) throw new Error("missing root block fixture");
    backend.pendingBlockSaves[0]?.resolve(savedBlock);
    await creation;

    expect(notes.selectedPageId).not.toBe(rootId);
    expect(backend.pendingCreates).toHaveLength(1);
    expect(notes.editorSaveError).toBeNull();
  });

  it("retains an edited main note while a preview is open and reads it again after leaving the document", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.createPage("First");
    const firstId = notes.selectedPageId;
    if (!firstId) throw new Error("missing first provisional page");
    const created = page(firstId);
    backend.pages.set(firstId, created);
    const response = openResponse(created);
    backend.pendingCreates[0]?.resolve({ page: response.page, blocks: response.blocks });
    await vi.waitFor(() => expect(notes.pageCreationPending).toBe(false));

    const firstBlock = Object.values(notes.blocksById).find((block) => (
      block.parent.type === "page_id" && block.parent.page_id === firstId
    ));
    if (!firstBlock) throw new Error("missing first block");
    await notes.updateBlockText(firstBlock.id, "Edited");
    const nextCreation = notes.createPage("Second");
    await vi.waitFor(() => expect(backend.pendingBlockSaves).toHaveLength(1));
    backend.pendingBlockSaves[0]?.resolve(firstBlock);
    await nextCreation;

    await notes.selectPage(firstId);
    expect(backend.openCalls).not.toContain(firstId);
    expect(notes.loadedPage?.id).toBe(firstId);
    await notes.selectPage(otherId, { openMode: "full" });
    await notes.selectPage(firstId, { openMode: "full" });
    expect(backend.openCalls).toContain(firstId);
    expect(notes.loadedPage?.id).toBe(firstId);
  });

  it("hides the page subtree immediately and ignores a stale page open", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(rootId);
    backend.deferredOpenPageId = childId;
    const staleOpen = notes.selectPage(childId);
    await vi.waitFor(() => expect(backend.pendingOpens).toHaveLength(1));

    const deletion = notes.trashPage(rootId);
    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    expect(notes.isPagePendingRemoval(childId)).toBe(true);
    await notes.selectPage(rootId);
    expect(backend.openCalls.filter((id) => id === rootId)).toHaveLength(1);

    backend.pendingOpens[0]?.reject(new Error("notes page not found"));
    await staleOpen;
    expect(notes.loadError).toBeNull();
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    const trashed = backend.pages.get(rootId);
    if (!trashed) throw new Error("missing root fixture");
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0]?.resolve({ ...trashed, in_trash: true });
    await deletion;

    expect(notes.selectedPageId).toBe(otherId);
    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    expect(notes.loadError).toBeNull();
  });

  it("waits for a new note to exist before moving it to Trash", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.createPage("New note");
    const newId = notes.selectedPageId;
    if (!newId) throw new Error("missing provisional page");

    const deletion = notes.trashPage(newId);
    expect(notes.navigationPages.some((item) => item.id === newId)).toBe(false);
    expect(backend.pendingTrashes).toHaveLength(0);
    const created = page(newId);
    backend.pages.set(newId, created);
    const createdPage = openResponse(created);
    backend.pendingCreates[0]?.resolve({ page: createdPage.page, blocks: createdPage.blocks });
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    backend.pages.delete(newId);
    backend.pendingTrashes[0]?.resolve({ ...created, in_trash: true });
    await deletion;

    expect(notes.navigationPages.some((item) => item.id === newId)).toBe(false);
    expect(notes.selectedPageId).not.toBe(newId);
    expect(notes.trashActionError).toBeNull();
  });

  it("discards a new local note if its creation fails", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.createPage("New note");
    const newId = notes.selectedPageId;
    if (!newId) throw new Error("missing provisional page");
    const draftBlock = Object.values(notes.blocksById).find((block) => (
      block.parent.type === "page_id" && block.parent.page_id === newId
    ));
    if (!draftBlock) throw new Error("missing provisional block");
    await notes.updateBlockText(draftBlock.id, "Local draft");

    const deletion = notes.trashPage(newId);
    backend.pendingCreates[0]?.reject(new Error("disk full"));
    await deletion;

    expect(backend.pendingTrashes).toHaveLength(0);
    expect(backend.pendingBlockSaves).toHaveLength(0);
    expect(notes.navigationPages.some((item) => item.id === newId)).toBe(false);
    expect(notes.selectedPageId).not.toBe(newId);
    expect(notes.trashActionError).toBeNull();
    expect(notes.editorSaveError).toBeNull();
  });

  it("restores navigation when the Trash operation fails", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    const deletion = notes.trashPage(rootId);
    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    backend.pendingTrashes[0]?.reject(new Error("database unavailable"));
    await expect(deletion).rejects.toThrow("database unavailable");
    expect(notes.navigationPages.map((item) => item.id)).toEqual([rootId, childId, otherId]);
    expect(notes.trashActionError).toBe("database unavailable");
  });

  it("removes a stale note when the database confirms it is already inactive", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(rootId);
    const deletion = notes.trashPage(rootId);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0]?.reject(new Error("notes page not found"));
    await deletion;

    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    expect(notes.selectedPageId).toBe(otherId);
    expect(notes.trashActionError).toBeNull();
    expect(notes.editorSaveError).toBeNull();
    await notes.selectPage(null);
    await notes.selectPage(otherId);
    expect(notes.selectedPageId).toBe(otherId);
  });

  it("removes a stale note when its pending save reports that the page is gone", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(rootId);
    await notes.updateBlockText(`block-${rootId}`, "Changed");

    const deletion = notes.trashPage(rootId);
    await vi.waitFor(() => expect(backend.pendingBlockSaves).toHaveLength(1));
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingBlockSaves[0]?.reject(new Error("notes page not found"));
    await deletion;

    expect(backend.pendingTrashes).toHaveLength(0);
    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    expect(notes.selectedPageId).toBe(otherId);
    expect(notes.trashActionError).toBeNull();
    expect(notes.editorSaveError).toBeNull();
    await notes.selectPage(null);
    await notes.selectPage(otherId);
    expect(notes.selectedPageId).toBe(otherId);
  });

  it("keeps another open note selected when deleting an unselected note", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(otherId);
    const deletion = notes.trashPage(rootId);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    const trashed = backend.pages.get(rootId);
    if (!trashed) throw new Error("missing root fixture");
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0]?.resolve({ ...trashed, in_trash: true });
    await deletion;

    expect(notes.selectedPageId).toBe(otherId);
    expect(backend.openCalls).toEqual([otherId]);
  });

  it("does not wait for another note's pending save before moving a note to Trash", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(otherId);
    await notes.updateBlockText(`block-${otherId}`, "Changed");

    const deletion = notes.trashPage(rootId);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    const trashed = backend.pages.get(rootId);
    if (!trashed) throw new Error("missing root fixture");
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0]?.resolve({ ...trashed, in_trash: true });
    await deletion;

    const save = notes.flushPendingWrites();
    await vi.waitFor(() => expect(backend.pendingBlockSaves).toHaveLength(1));
    const other = backend.pages.get(otherId);
    if (!other) throw new Error("missing other page fixture");
    const savedBlock = openResponse(other).blocks.results[0];
    if (!savedBlock) throw new Error("missing block fixture");
    backend.pendingBlockSaves[0]?.resolve(savedBlock);
    await save;
    expect(notes.selectedPageId).toBe(otherId);
  });

  it("closes an open descendant whose parent is a block in the deleted note", async () => {
    backend.pages.set(childId, page(childId, { type: "block_id", block_id: "parent-block" }));
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(childId);

    const deletion = notes.trashPage(rootId);
    expect(notes.navigationPages.map((item) => item.id)).toEqual([otherId]);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    const trashed = backend.pages.get(rootId);
    if (!trashed) throw new Error("missing root fixture");
    backend.pages.delete(rootId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0]?.resolve({ ...trashed, in_trash: true });
    await deletion;

    expect(notes.selectedPageId).toBe(otherId);
  });

  it.each([childId, rootId])("closes affected preview panes when removing %s and keeps a valid main selection", async (removedId) => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    await notes.load();
    await notes.selectPage(rootId, { openMode: "full" });
    const main = notes.editorPanes[0].store;
    await notes.selectPage(childId, { openMode: "side" });
    const deletion = notes.trashPage(removedId);
    await vi.waitFor(() => expect(backend.pendingTrashes).toHaveLength(1));
    const removed = backend.pages.get(removedId)!;
    backend.pages.delete(removedId);
    backend.pages.delete(childId);
    backend.pendingTrashes[0].resolve({ ...removed, in_trash: true });
    await deletion;
    expect(notes.previewPane).toBeNull();
    expect(notes.editorPanes[0].store).toBe(main);
    expect(notes.selectedPageId).toBe(removedId === rootId ? otherId : rootId);
  });
});
