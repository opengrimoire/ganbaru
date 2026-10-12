// @vitest-environment jsdom

import { beforeAll, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, createBlockUpdate, createBlockWrite } from "$lib/notes/blocks/factory";
import { notesDatabaseSession } from "$lib/notes/database/session.svelte";
import type { NotesCompoundEdit, NotesCompoundEditResult } from "$lib/api/notes/compound-edits";
import type {
  NotesBlock,
  NotesBlockUpdate,
  NotesMoveBlockRequest,
  NotesAppendBlockChildrenRequest,
  NotesBlockFrontier,
  NotesDatabaseReference,
  NotesPage,
  NotesPageOpenResponse,
  NotesWorkspaceShell,
} from "$lib/notes/types";

const backend = vi.hoisted(() => ({
  calls: [] as Array<{ name: string; ids?: readonly string[] }>,
  pages: new Map<string, NotesPageOpenResponse>(),
  hydrated: new Map<string, NotesBlock>(),
  frontier: async (_ids: readonly string[]): Promise<NotesBlockFrontier> => ({ blocks: [] }),
  databaseReference: async (_id: string): Promise<NotesDatabaseReference> => { throw new Error("Missing database fixture"); },
  record(name: string, ids?: readonly string[]): void {
    this.calls.push({ name, ids });
  },
  count(name: string): number {
    return this.calls.filter((call) => call.name === name).length;
  },
  clear(): void {
    this.calls.length = 0;
  },
}));

vi.mock("$lib/api/notes/compound-edits", () => ({
  applyNotesCompoundEdit: async (request: NotesCompoundEdit): Promise<NotesCompoundEditResult> => {
    backend.record("compound");
    const api = await import("$lib/api/notes");
    const before = Object.keys(request.expected_blocks).map((id) => {
      const block = backend.hydrated.get(id);
      if (!block) throw new Error("Missing canonical block fixture");
      return block;
    });
    const blocks: NotesBlock[] = [];
    const placements: NotesCompoundEditResult["placements"] = [];
    for (const operation of request.operations) {
      switch (operation.type) {
        case "append": {
          const appended = (await api.appendNotesBlockChildren(operation.request)).results;
          blocks.push(...appended);
          let after = operation.request.after;
          for (const block of appended) {
            placements.push({ blockId: block.id, parent: block.parent, after });
            after = block.id;
          }
          break;
        }
        case "update": blocks.push(await api.updateNotesBlock(operation.block_id, operation.update)); break;
        case "move": {
          const block = await api.moveNotesBlock(operation.block_id, operation.request);
          blocks.push(block);
          placements.push({ blockId: block.id, ...operation.request });
          break;
        }
        case "trash": {
          const block = backend.hydrated.get(operation.block_id);
          if (!block) throw new Error("Missing trash block fixture");
          const saved = { ...block, in_trash: operation.in_trash };
          backend.hydrated.set(saved.id, saved);
          blocks.push(saved);
          break;
        }
        default: throw new Error(`Unsupported page fixture operation ${operation.type}`);
      }
    }
    return { operation_id: request.operation_id, page_id: request.page_id, blocks, placements, databases: [], before_blocks: before, before_placements: [] };
  },
}));

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (_key: string, fallback: unknown) => fallback,
  setConfigKey: vi.fn(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  const unresolved = (name: string) => {
    backend.record(name);
    return new Promise<never>(() => undefined);
  };
  return {
    ...actual,
    getNotesDatabaseReference: async (id: string) => {
      backend.record("database-reference", [id]);
      return backend.databaseReference(id);
    },
    updateNotesBlock: async (id: string, update: NotesBlockUpdate) => {
      const block = backend.hydrated.get(id);
      if (!block) throw new Error("Missing block fixture");
      const saved = applyBlockUpdate(block, update);
      backend.hydrated.set(id, saved);
      return saved;
    },
    moveNotesBlock: async (id: string, request: NotesMoveBlockRequest) => {
      const block = backend.hydrated.get(id);
      if (!block) throw new Error("Missing block fixture");
      const saved = { ...block, parent: request.parent };
      backend.hydrated.set(id, saved);
      return saved;
    },
    saveNotesUndoState: async () => undefined,
    clearNotesUndoState: async () => undefined,
    loadNotesWorkspaceShell: async (): Promise<NotesWorkspaceShell> => ({
      pages: [...backend.pages.values()].map((response) => response.page),
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
    appendNotesBlockChildren: async (request: NotesAppendBlockChildrenRequest) => {
      backend.record("append");
      if (request.parent.type !== "page_id") throw new Error("Expected a page paragraph");
      const current = backend.pages.get(request.parent.page_id);
      if (!current) throw new Error("Missing append parent");
      const blocks = request.children.map((write) => ({
        ...paragraph(write.id, request.parent), ...write,
      }) as NotesBlock);
      const updated = response(current.page.id, [...current.blocks.results, ...blocks]);
      backend.pages.set(current.page.id, updated);
      return { object: "list", type: "block", block: {}, results: blocks, next_cursor: null, has_more: false };
    },
    openNotesPage: async (pageId: string) => {
      backend.record("open");
      const response = backend.pages.get(pageId);
      if (!response) throw new Error("missing page fixture");
      return response;
    },
    getNotesBlockFrontier: async (ids: readonly string[]) => {
      backend.record("frontier", [...ids]);
      return backend.frontier(ids);
    },
    getNotesBlockOutlineFrontier: async (_pageId: string, ids: readonly string[]) => {
      backend.record("frontier", [...ids]);
      const result = await backend.frontier(ids);
      for (const block of result.blocks) backend.hydrated.set(block.id, block);
      return result.blocks.map((block, index) => ({
        id: block.id,
        page_id: block.parent.type === "page_id" ? block.parent.page_id : pageAId,
        parent: block.parent.type === "page_id" || block.parent.type === "block_id"
          ? block.parent
          : { type: "page_id" as const, page_id: pageAId },
        type: block.type,
        sort_order: (index + 1) * 1_000,
        has_children: block.has_children,
        retained_height: 36,
      }));
    },
    hydrateNotesBlocks: async (request: { block_ids: string[] }) => {
      const blocks = request.block_ids.map((id) => backend.hydrated.get(id))
        .filter((block): block is NotesBlock => block !== undefined);
      return blocks;
    },
    getNotesBlockChildren: async () => ({
      object: "list",
      type: "block",
      block: {},
      results: [],
      next_cursor: null,
      has_more: false,
    }),
    listNotesBacklinks: () => unresolved("backlinks"),
    listNotesPageAliases: () => unresolved("aliases"),
    listNotesUnresolvedLinks: () => unresolved("unresolved"),
    listNotesDestinationCandidates: () => unresolved("destinations"),
    listNotesComments: () => unresolved("comments"),
    listNotesSuggestions: () => unresolved("suggestions"),
    listNotesPageHistorySnapshots: () => unresolved("history"),
    loadNotesUndoState: () => unresolved("undo"),
  };
});

const pageAId = "10000000-0000-4000-8000-000000000001";
const pageBId = "10000000-0000-4000-8000-000000000002";
const topAId = "10000000-0000-4000-8000-000000000003";
const topBId = "10000000-0000-4000-8000-000000000004";
const childAId = "10000000-0000-4000-8000-000000000005";
const childBId = "10000000-0000-4000-8000-000000000006";
const nestedId = "10000000-0000-4000-8000-000000000007";
const staleId = "10000000-0000-4000-8000-000000000008";
const now = "2026-07-11T12:00:00.000Z";

function page(id: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent: { type: "workspace", workspace: true },
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

function paragraph(id: string, parent: NotesBlock["parent"], hasChildren = false): NotesBlock {
  const write = createBlockWrite(id, "paragraph", id);
  if (write.type !== "paragraph") throw new Error("expected paragraph");
  return {
    object: "block",
    id,
    parent,
    created_time: now,
    last_edited_time: now,
    has_children: hasChildren,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    edit_revision: "1".padStart(64, "0"),
    type: "paragraph",
    paragraph: write.paragraph,
  };
}

function response(pageId: string, blocks: NotesBlock[]): NotesPageOpenResponse {
  for (const block of blocks) backend.hydrated.set(block.id, block);
  return {
    page: page(pageId),
    breadcrumb: [{ id: pageId, title: pageId, current: true, status: "active" }],
    outlines: blocks.map((block, index) => ({
      id: block.id,
      page_id: pageId,
      parent: block.parent.type === "page_id" || block.parent.type === "block_id"
        ? block.parent
        : { type: "page_id", page_id: pageId },
      type: block.type,
      sort_order: (index + 1) * 1_000,
      has_children: block.has_children,
      retained_height: 36,
    })),
    blocks: {
      object: "list",
      type: "block",
      block: {},
      results: blocks,
      next_cursor: null,
      has_more: false,
    },
  };
}

describe("Notes critical page opening", () => {
  let notes: ReturnType<(typeof import("./notes.svelte"))["getNotes"]>;

  beforeAll(async () => {
    backend.pages.set(pageAId, response(pageAId, [
      paragraph(topAId, { type: "page_id", page_id: pageAId }, true),
      paragraph(topBId, { type: "page_id", page_id: pageAId }, true),
    ]));
    backend.pages.set(pageBId, response(pageBId, [
      paragraph(childBId, { type: "page_id", page_id: pageBId }),
    ]));
    notes = (await import("./notes.svelte")).getNotes();
    await notes.load();
  });

  it("makes content ready after one critical command while auxiliary promises stay unresolved", async () => {
    backend.frontier = async (ids) => ({
      blocks: ids.includes(topAId)
        ? [
            paragraph(childAId, { type: "block_id", block_id: topAId }, true),
            paragraph(childBId, { type: "block_id", block_id: topBId }),
          ]
        : [paragraph(nestedId, { type: "block_id", block_id: childAId })],
    });
    backend.clear();

    await notes.selectPage(pageAId);

    expect(notes.primaryContentReady).toBe(true);
    expect(notes.loadedPage?.id).toBe(pageAId);
    expect(Object.keys(notes.blocksById)).toContain(topAId);
    expect(backend.count("open")).toBe(1);
    expect(backend.count("backlinks")).toBe(0);
    expect(backend.count("aliases")).toBe(0);
    expect(backend.count("unresolved")).toBe(0);
    expect(backend.count("comments")).toBe(0);
    expect(backend.count("suggestions")).toBe(0);
    expect(backend.count("history")).toBe(0);
    expect(backend.count("undo")).toBe(0);

    await vi.waitFor(() => expect(backend.count("frontier")).toBe(2));
    expect(backend.calls.filter((call) => call.name === "frontier").map((call) => call.ids)).toEqual([
      [topAId, topBId],
      [childAId],
    ]);
    await vi.waitFor(() => expect(Object.keys(notes.blocksById)).toContain(nestedId));
  });

  it("ignores a late descendant frontier after switching pages", async () => {
    const frontierDeferred: {
      resolve?: (value: NotesBlockFrontier) => void;
    } = {};
    backend.frontier = () => new Promise((resolve) => {
      frontierDeferred.resolve = resolve;
    });
    await notes.selectPage(null);
    backend.clear();
    await notes.selectPage(pageAId);
    await vi.waitFor(() => expect(backend.count("frontier")).toBe(1));
    const indentation = notes.nestBlock(topAId);
    await notes.selectPage(pageBId);
    frontierDeferred.resolve?.({
      blocks: [paragraph(staleId, { type: "block_id", block_id: topAId })],
    });
    await Promise.resolve();
    await indentation;

    expect(notes.selectedPageId).toBe(pageBId);
    expect(notes.loadedPage?.id).toBe(pageBId);
    expect(Object.keys(notes.blocksById)).not.toContain(staleId);
    expect(notes.flatBlocks.map((row) => row.depth)).toEqual([0]);
  });

  it("returns to the primary note after closing a contextual page", async () => {
    backend.frontier = async () => ({ blocks: [] });
    await notes.selectPage(pageAId, { openMode: "full" });

    await notes.openPageContextually(pageBId);

    expect(notes.selectedPageId).toBe(pageBId);
    expect(notes.pageOpenMode).toBe("center");

    await notes.closeContextualPage();

    expect(notes.selectedPageId).toBe(pageAId);
    expect(notes.pageOpenMode).toBe("full");
  });
  it("opens a previously emptied note with a writable body and persists that recovery only once", async () => {
    const emptyPageId = "10000000-0000-4000-8000-000000000009";
    backend.pages.set(emptyPageId, response(emptyPageId, []));
    backend.clear();
    await notes.selectPage(emptyPageId);
    expect(notes.primaryContentReady).toBe(true);
    expect(notes.flatBlocks).toHaveLength(1);
    const paragraphId = notes.flatBlocks[0].block.id;
    expect(notes.flatBlocks[0].block.type).toBe("paragraph");
    await vi.waitFor(() => expect(backend.count("append")).toBe(1));
    await notes.selectPage(pageBId);
    await notes.selectPage(emptyPageId);
    expect(notes.flatBlocks.map((item) => item.block.id)).toEqual([paragraphId]);
    expect(backend.count("append")).toBe(1);
  });

  it("waits for nested rows to be discovered before applying repeated parent indentation", async () => {
    let finishFrontier!: (value: NotesBlockFrontier) => void;
    backend.frontier = () => new Promise((resolve) => { finishFrontier = resolve; });
    await notes.selectPage(null);
    await notes.selectPage(pageAId);
    const first = notes.nestBlock(topAId);
    const second = notes.nestBlock(topAId);
    expect(notes.flatBlocks.map((row) => row.depth)).toEqual([0, 0]);
    finishFrontier({ blocks: [paragraph(childAId, { type: "block_id", block_id: topAId })] });
    await Promise.all([first, second]);
    expect(notes.flatBlocks.map((row) => [row.block.id, row.depth])).toEqual([
      [topAId, 2], [childAId, 1], [topBId, 0],
    ]);
    await notes.flushPendingWrites();
  });

  it("opens a database in its existing owner pane without closing the other pane or reading either note again", async () => {
    await notes.selectPage(null);
    backend.frontier = async () => ({ blocks: [] });
    const database = applyBlockUpdate(paragraph(topAId, { type: "page_id", page_id: pageAId }), {
      type: "child_database", child_database: { title: "Planning", database_id: topAId },
    });
    backend.pages.set(pageAId, response(pageAId, [database]));
    backend.databaseReference = async () => ({
      block_id: topAId, page_id: pageAId, title: "Planning",
      source_block_id: topAId, source_page_id: pageAId, is_linked: false, owned_data_source_count: 1, editing_locked: false,
    });
    await notes.selectPage(pageAId, { openMode: "full" });
    const owner = notes.editorPanes[0];
    const focusBlockId = owner.store.focusBlockId;
    const focusRequestId = owner.store.focusRequestId;
    await notes.selectPage(pageBId, { openMode: "side" });
    const preview = notes.previewPane;
    backend.clear();
    expect(await notes.openNotesLink({ pageId: pageAId, blockId: topAId })).toBe(true);
    expect(notes.activePaneId).toBe(owner.id);
    expect(notes.editorPanes[0]).toBe(owner);
    expect(notes.previewPane).toBe(preview);
    expect(notes.selectedDatabaseBlock?.id).toBe(topAId);
    expect(owner.store.focusBlockId).toBeNull();
    expect(owner.store.focusRequestId).toBe(focusRequestId);
    expect(backend.count("database-reference")).toBe(1);
    expect(backend.count("open")).toBe(0);
    notes.closeDatabase();
    expect(notes.selectedPageId).toBe(pageAId);
    expect(notes.selectedDatabaseBlockId).toBeNull();
    expect(owner.store.focusBlockId).toBe(focusBlockId);
    expect(notes.previewPane).toBe(preview);
  });

  it("rejects a database link that names a different owner and preserves the active document", async () => {
    const owner = notes.selectedPageId;
    backend.clear();
    expect(await notes.openNotesLink({ pageId: pageBId, blockId: topAId })).toBe(false);
    expect(notes.selectedPageId).toBe(owner);
    expect(notes.selectedDatabaseBlockId).toBeNull();
    expect(backend.count("open")).toBe(0);
  });

  it.each(["single block", "block selection", "atomic document range"])("leaves a database unchanged when deleting its %s is cancelled", async (kind) => {
    await notes.selectPage(pageAId);
    const database = notes.blockById(topAId);
    const deletion = kind === "single block" ? notes.deleteBlock(topAId)
      : kind === "block selection" ? notes.deleteBlockSelection([topAId])
        : notes.replaceDocumentRange([topAId], 0, 1, "");
    await vi.waitFor(() => expect(notes.databaseDeletion.prompt?.loading).toBe(false));
    expect(notes.databaseDeletion.prompt?.databaseIds).toEqual([topAId]);
    expect(notes.blockById(topAId)).toBe(database);
    notes.databaseDeletion.cancel();
    expect(await deletion).toBe(false);
    expect(notes.blockById(topAId)).toBe(database);
    expect(notes.blockById(topAId)?.type).toBe("child_database");
    expect(notes.databaseDeletion.prompt).toBeNull();
  });

  it("refuses database conversion while preserving its source identities", async () => {
    const database = notes.blockById(topAId);
    await expect(notes.convertBlock(topAId, "paragraph")).rejects.toThrow();
    expect(notes.blockById(topAId)).toBe(database);
    expect(notes.blockById(topAId)?.type).toBe("child_database");
  });

  it("keeps a dismissed database closed when its metadata arrives later", async () => {
    const readReference = backend.databaseReference;
    const reference = await readReference(topAId);
    let finish!: (value: NotesDatabaseReference) => void;
    backend.databaseReference = () => new Promise((resolve) => { finish = resolve; });
    const opening = notes.openDatabase(topAId);
    notes.closeDatabase();
    finish(reference);
    expect(await opening).toBe(false);
    expect(notes.selectedDatabaseBlockId).toBeNull();
    backend.databaseReference = readReference;
    await notes.selectPage(null);
  });

  it("opens a linked shell's own saved view from hierarchy navigation while retaining its owner", async () => {
    await notes.selectPage(null);
    const previousReference = backend.databaseReference;
    const previousPage = backend.pages.get(pageBId);
    const linked = applyBlockUpdate(paragraph(topBId, { type: "page_id", page_id: pageBId }), {
      type: "child_database", child_database: { title: "Shared planning", database_id: topBId, data_source_id: childAId },
    });
    backend.pages.set(pageBId, response(pageBId, [linked]));
    backend.databaseReference = async () => ({
      block_id: topBId, page_id: pageBId, title: "Shared planning", source_block_id: topAId,
      source_page_id: pageAId, is_linked: true, owned_data_source_count: 0, editing_locked: false,
    });
    await notes.selectPage(pageBId, { openMode: "full" });
    const owner = notes.editorPanes[0];
    backend.clear();
    const revision = notesDatabaseSession.revision;
    expect(await notes.openDatabase({ pageId: pageBId, blockId: topBId }, { followSource: false, viewId: childBId })).toBe(true);
    expect(notes.selectedPageId).toBe(pageBId);
    expect(notes.selectedDatabaseBlockId).toBe(topBId);
    expect(notes.editorPanes[0]).toBe(owner);
    expect(notesDatabaseSession.recall(topBId)?.viewId).toBe(childBId);
    expect(notesDatabaseSession.revision).toBe(revision);
    expect(backend.count("open")).toBe(0);
    expect(notes.navigationDatabases).toContainEqual({ id: topBId, page_id: pageBId, title: "Shared planning", data_source_id: childAId });
    notes.closeDatabase();
    await notes.selectPage(null);
    backend.databaseReference = previousReference;
    if (previousPage) backend.pages.set(pageBId, previousPage);
    notesDatabaseSession.clear();
  });

  /** Load a collapsed parent while leaving its database descendant to bounded hydration. */
  async function openCollapsedDatabaseParent(): Promise<void> {
    await notes.selectPage(null);
    const toggle = applyBlockUpdate(paragraph(topBId, { type: "page_id", page_id: pageAId }, true), createBlockUpdate("toggle", "Details"));
    if (toggle.type !== "toggle") throw new Error("Expected a toggle fixture");
    toggle.toggle.ganbaru_open = false;
    const database = applyBlockUpdate(paragraph(childAId, { type: "block_id", block_id: topBId }), {
      type: "child_database", child_database: { title: "Nested planning", database_id: childAId },
    });
    const loaded = response(pageAId, [toggle, database]);
    loaded.blocks.results = [toggle];
    backend.pages.set(pageAId, loaded);
    backend.databaseReference = async (id) => ({
      block_id: id, page_id: pageAId, title: "Nested planning", source_block_id: id,
      source_page_id: pageAId, is_linked: false, owned_data_source_count: 1, editing_locked: false,
    });
    await notes.selectPage(pageAId);
    backend.clear();
  }

  it.each(["block selection", "collapsed document range"])("hydrates and confirms an unseen descendant database before removing its %s", async (kind) => {
    await openCollapsedDatabaseParent();
    expect(notes.blockById(childAId)).toBeUndefined();
    const deletion = kind === "block selection" ? notes.deleteBlockSelection([topBId])
      : notes.replaceDocumentRange([topBId], 0, "Details".length, "");
    await vi.waitFor(() => expect(notes.databaseDeletion.prompt?.loading).toBe(false));
    expect(notes.databaseDeletion.prompt?.databaseIds).toEqual([childAId]);
    expect(notes.blockById(childAId)?.type).toBe("child_database");
    notes.databaseDeletion.cancel();
    expect(await deletion).toBe(false);
    expect(notes.blockById(topBId)?.type).toBe("toggle");
    expect(notes.blockById(childAId)?.type).toBe("child_database");
    await notes.selectPage(null);
  });

  it("preserves a collapsed database descendant without confirmation when only part of the parent's text changes", async () => {
    await openCollapsedDatabaseParent();
    const descendant = backend.hydrated.get(childAId);
    expect(descendant?.type).toBe("child_database");
    expect(await notes.replaceDocumentRange([topBId], 1, "Details".length, "")).toBe(true);
    expect(notes.databaseDeletion.prompt).toBeNull();
    expect(backend.count("database-reference")).toBe(0);
    expect(notes.blockById(childAId)).toBeUndefined();
    expect(backend.hydrated.get(childAId)).toBe(descendant);
    await notes.flushPendingWrites();
    await notes.selectPage(null);
  });

});
