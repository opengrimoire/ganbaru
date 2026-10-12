import { afterEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockPlainText, createBlockUpdate } from "$lib/notes/blocks/factory";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import type { NotesBlock, NotesBlockUpdate, NotesChildPageFromBlockCreate, NotesLoadedPage } from "$lib/notes/types";
import { createNotesBlockActions } from "./block-actions";
import { createNotesPageActions } from "./page-actions";
import { createNotesBlockPersistence } from "./persistence";
import { NotesPageSessionController } from "./page-session.svelte";

const api = vi.hoisted(() => ({ updateNotesBlock: vi.fn(), createNotesChildPageFromBlock: vi.fn() }));
vi.mock("$lib/api/notes", () => api);

const pageId = "00000000-0000-4000-8000-000000000001";
const blockId = "00000000-0000-4000-8000-000000000002";

/** Connect lifecycle, editing, session, and persistence controllers around delayed native writes. */
function editor(text = "/Note") {
  const initial = createProvisionalNotesPage({
    id: pageId, first_block_id: blockId, title: "Parent",
    parent: { type: "workspace", workspace: true }, folder_id: null,
  });
  const original = applyBlockUpdate(initial.blocks.results[0], createBlockUpdate("paragraph", text));
  const stored = new Map<string, NotesBlock>([[blockId, original]]);
  let blocks: Record<string, NotesBlock> = { [blockId]: original };
  let loaded = initial;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const writes: string[] = [];
  const error = vi.fn();
  const focus = vi.fn();
  const resetUndoHistory = vi.fn();
  const session = new NotesPageSessionController({
    initialSelectedPageId: pageId, persistSelectedPageId: vi.fn(), recordRecentPage: vi.fn(),
  });
  const persistence = createNotesBlockPersistence({
    readBlock: (id) => blocks[id], beforeSave: async () => undefined,
    replaceBlock: (block) => { blocks = { ...blocks, [block.id]: block }; },
    setLoadError: error, debounceMs: 250,
  });
  api.updateNotesBlock.mockImplementation(async (id: string, update: NotesBlockUpdate) => {
    await gate;
    const current = stored.get(id);
    if (!current) throw new Error("Missing source block");
    const saved = applyBlockUpdate(current, update);
    if ((saved.type === "child_page") !== (current.type === "child_page")) throw new Error("Invalid page identity change");
    stored.set(id, saved);
    writes.push(`update:${saved.type}`);
    return saved;
  });
  api.createNotesChildPageFromBlock.mockImplementation(async (id: string, request: NotesChildPageFromBlockCreate) => {
    await gate;
    const source = stored.get(id);
    if (!source) throw new Error("Missing source block");
    const title = request.title ?? blockPlainText(source);
    stored.set(id, applyBlockUpdate(source, createBlockUpdate("child_page", title)));
    writes.push("create:child_page");
    return createProvisionalNotesPage({
      id, first_block_id: request.first_block_id, title, parent: source.parent,
      folder_id: null, properties: request.properties,
    });
  });
  const pageActions = createNotesPageActions({
    ...persistence, resetUndoHistory,
    readSelectedPageId: () => session.selectedPageId,
    readPages: () => [initial.page], readAllPages: () => [initial.page], readLoadedPage: () => loaded.page,
    readFolders: () => [], readBlocksById: () => blocks, defaultOpenMode: () => "center",
    activateReturnedPage: async (next: NotesLoadedPage) => {
      session.open(next.page.id, "center");
      loaded = next;
      blocks = Object.fromEntries(next.blocks.results.map((block) => [block.id, block]));
    },
    activateProvisionalPage: vi.fn(), beginPageCreation: vi.fn(), awaitPageReady: async () => undefined,
    awaitPageCreationAttempt: async () => "ready", discardFailedPageCreation: vi.fn(),
    activateRestoredPage: async () => undefined, applyPostMutation: vi.fn(),
    selectPage: async () => undefined, selectPageAfterRemoval: async () => undefined,
    discardRemovedPageWrites: vi.fn(), reloadPageBreadcrumb: async () => undefined,
    flushPendingWrites: persistence.flushPendingBlockSaves,
    requestBlockFocus: focus, requestTitleFocus: vi.fn(), requestPageLoadFocus: vi.fn(),
    queueDescendantHydration: vi.fn(), setFolderCollapsed: vi.fn(), prependArchivedPage: vi.fn(),
    prependTrashedPage: vi.fn(), removeArchivedPages: vi.fn(), removeTrashedPages: vi.fn(),
    removePagesFromActiveCollections: vi.fn(), removeSidebarPageIds: vi.fn(), scheduleHierarchyRefresh: vi.fn(),
    beginPageRemoval: () => null, endPageRemoval: vi.fn(), setTrashActionError: vi.fn(),
  });
  const actions = createNotesBlockActions({
    ...persistence,
    readSelectedPageId: () => session.selectedPageId, awaitSelectedPageReady: async () => undefined,
    readBlocksById: () => blocks, blockById: (id) => blocks[id],
    readPageRootBlockIds: () => [blockId], readChildIdsByParentId: () => ({ [pageId]: [blockId] }),
    treeState: () => ({ blocksById: blocks, childIdsByParentId: { [pageId]: [blockId] } }),
    outlineSubtreeIds: (ids) => [...ids], flatBlockItemsForBlockContext: () => [],
    tableRowsForBlock: () => [], columnItemsForBlock: () => [], tabItemsForBlock: () => [],
    requestBlockFocus: focus,
    createChildPageFromBlock: pageActions.createChildPageFromBlock,
    createChildPageAfterBlock: pageActions.createChildPageAfterBlock,
    loadPageTree: async () => undefined, refreshOpenLinks: async () => undefined,
    applyPostMutation: vi.fn(), localInsertBlockAfter: vi.fn(), localInsertBlockBefore: vi.fn(),
    localRemoveLeafBlock: () => false, createUndoSnapshot: () => null,
    createUndoSnapshotForBlocks: () => null, recordUndo: vi.fn(),
  });
  return { actions, persistence, stored, writes, release, session, error, resetUndoHistory, block: () => blocks[blockId] };
}

afterEach(() => vi.resetAllMocks());

describe("child-note creation through the editor queue", () => {
  it("changes the row immediately, saves prior typing first, and ignores repeated commands and stale text events", async () => {
    const h = editor();
    await h.actions.updateBlockText(blockId, "/Note draft");
    const creation = h.actions.convertBlock(blockId, "child_page", true);
    expect(h.block()).toMatchObject({ type: "child_page", child_page: { title: "" } });
    expect(h.resetUndoHistory).toHaveBeenCalledExactlyOnceWith(pageId);
    expect(h.session.selectedPageId).toBe(pageId);
    await h.actions.convertBlock(blockId, "child_page", true);
    await h.actions.convertBlock(blockId, "paragraph", true);
    await h.actions.updateBlockText(blockId, "/Note again");
    await h.actions.updateBlockRichText(blockId, []);
    h.release();
    await creation;
    await h.persistence.flushPendingBlockSaves();
    expect(h.writes).toEqual(["update:paragraph", "create:child_page"]);
    expect(api.createNotesChildPageFromBlock).toHaveBeenCalledOnce();
    expect(h.stored.get(blockId)).toMatchObject({ type: "child_page", child_page: { title: "" } });
    expect(h.session.selectedPageId).toBe(blockId);
    expect(h.error).not.toHaveBeenCalled();
  });

  it("waits for the source block's queued insertion and retains its title for a non-slash conversion", async () => {
    const h = editor("Project details");
    const source = h.stored.get(blockId)!;
    h.stored.delete(blockId);
    const insertion = h.persistence.enqueueEditorMutation(async () => {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      h.stored.set(blockId, source);
      h.writes.push("insert:paragraph");
    });
    const creation = h.actions.convertBlock(blockId, "child_page");
    h.release();
    await Promise.all([insertion, creation]);
    expect(h.writes).toEqual(["insert:paragraph", "create:child_page"]);
    expect(h.stored.get(blockId)).toMatchObject({ child_page: { title: "Project details" } });
  });

  it("rejects stale paragraph edits after native conversion commits but before its response arrives", async () => {
    const h = editor();
    const nativeCreate = api.createNotesChildPageFromBlock.getMockImplementation()!;
    let deliverResponse!: () => void;
    const response = new Promise<void>((resolve) => { deliverResponse = resolve; });
    api.createNotesChildPageFromBlock.mockImplementation(async (id: string, request: NotesChildPageFromBlockCreate) => {
      const loaded = await nativeCreate(id, request);
      await response;
      return loaded;
    });
    h.release();
    const creation = h.actions.convertBlock(blockId, "child_page", true);
    await vi.waitFor(() => expect(h.stored.get(blockId)?.type).toBe("child_page"));
    expect(h.session.selectedPageId).toBe(pageId);
    await h.actions.convertBlock(blockId, "paragraph", true);
    await h.actions.updateBlockText(blockId, "/Note");
    deliverResponse();
    await creation;
    await h.persistence.flushPendingBlockSaves();
    expect(api.updateNotesBlock).not.toHaveBeenCalled();
    expect(h.error).not.toHaveBeenCalled();
    expect(h.session.selectedPageId).toBe(blockId);
  });

  it("keeps a failed conversion retryable and opens the child when that same queued operation succeeds", async () => {
    const h = editor();
    api.createNotesChildPageFromBlock.mockRejectedValueOnce(new Error("Storage unavailable"));
    h.release();
    await expect(h.actions.convertBlock(blockId, "child_page", true)).rejects.toThrow("Storage unavailable");
    expect(h.session.selectedPageId).toBe(pageId);
    expect(h.block()?.type).toBe("child_page");
    expect(h.error).toHaveBeenLastCalledWith("Storage unavailable");
    await h.persistence.retryEditorMutations();
    expect(api.createNotesChildPageFromBlock).toHaveBeenCalledTimes(2);
    expect(api.createNotesChildPageFromBlock.mock.calls[1]).toEqual(api.createNotesChildPageFromBlock.mock.calls[0]);
    expect(h.session.selectedPageId).toBe(blockId);
    expect(h.writes).toEqual(["create:child_page"]);
    expect(h.error).toHaveBeenLastCalledWith(null);
    await h.persistence.flushPendingBlockSaves();
  });
});
