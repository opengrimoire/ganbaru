// @vitest-environment jsdom

import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type {
  NotesBacklink,
  NotesBlock,
  NotesLoadedPage,
  NotesPage,
  NotesSearchWindow,
  NotesWorkspaceShell,
} from "$lib/notes/types";

const backend = vi.hoisted(() => ({
  calls: [] as string[],
  pages: new Map<string, NotesPage>(),
  loadedPages: new Map<string, NotesLoadedPage>(),
  pendingBacklinks: [] as Array<(value: NotesBacklink[]) => void>,
  pendingSearches: [] as Array<{ query: string; resolve: (value: NotesSearchWindow) => void }>,
  record(name: string): void {
    this.calls.push(name);
  },
  count(name: string): number {
    return this.calls.filter((call) => call === name).length;
  },
  clear(): void {
    this.calls.length = 0;
  },
}));

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (_key: string, fallback: unknown) => fallback,
  setConfigKey: vi.fn(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    loadNotesWorkspaceShell: async (): Promise<NotesWorkspaceShell> => {
      backend.record("workspace");
      return {
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
      };
    },
    loadNotesPage: async (pageId: string) => {
      backend.record("page");
      const loaded = backend.loadedPages.get(pageId);
      if (!loaded) throw new Error("missing page fixture");
      return loaded;
    },
    openNotesPage: async (pageId: string) => {
      backend.record("page");
      const loaded = backend.loadedPages.get(pageId);
      if (!loaded) throw new Error("missing page fixture");
      return { ...loaded, breadcrumb: [], outlines: [] };
    },
    getNotesBlockFrontier: async () => {
      backend.record("frontier");
      return { blocks: [] };
    },
    getNotesBlockOutlineFrontier: async () => [],
    hydrateNotesBlocks: async () => [],
    getNotesBlockChildren: async () => {
      backend.record("children");
      return { object: "list", type: "block", block: {}, results: [], next_cursor: null, has_more: false };
    },
    getNotesPageBreadcrumb: async () => {
      backend.record("breadcrumb");
      return [];
    },
    updateNotesPage: async (pageId: string) => {
      backend.record("update-page");
      const page = backend.pages.get(pageId);
      if (!page) throw new Error("missing page fixture");
      return page;
    },
    archiveNotesPage: async (pageId: string, archived: boolean) => {
      backend.record(archived ? "archive" : "restore-archive");
      const page = backend.pages.get(pageId);
      if (!page) throw new Error("missing page fixture");
      return { ...page, archived };
    },
    listNotesSidebarPages: async () => {
      backend.record("sidebar");
      return {
        pages: [...backend.pages.values()],
        page_ids_with_children: [],
        missing_parent_page_ids: [],
        trashed_parent_page_ids: [],
      };
    },
    listNotesPages: async () => {
      backend.record("all-pages");
      return [...backend.pages.values()];
    },
    listNotesFolders: async () => {
      backend.record("folders");
      return [];
    },
    listNotesBacklinks: async () => {
      backend.record("backlinks");
      return new Promise<NotesBacklink[]>((resolve) => backend.pendingBacklinks.push(resolve));
    },
    listNotesPageAliases: async () => {
      backend.record("aliases");
      return [];
    },
    listNotesUnresolvedLinks: async () => {
      backend.record("unresolved");
      return [];
    },
    listNotesDestinationCandidates: async () => {
      backend.record("destinations");
      return { pages: [], folders: [] };
    },
    searchNotes: async (query: string) => {
      backend.record(`search:${query}`);
      return new Promise<NotesSearchWindow>((resolve) => backend.pendingSearches.push({ query, resolve }));
    },
    loadNotesUndoState: async () => {
      backend.record("undo-state");
      return null;
    },
  };
});

const pageAId = "00000000-0000-4000-8000-000000000001";
const pageBId = "00000000-0000-4000-8000-000000000002";
const blockAId = "00000000-0000-4000-8000-000000000003";
const blockBId = "00000000-0000-4000-8000-000000000004";
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

function block(id: string, pageId: string): NotesBlock {
  const write = createBlockWrite(id, "paragraph", "Text");
  if (write.type !== "paragraph") throw new Error("expected paragraph");
  return {
    object: "block",
    id,
    parent: { type: "page_id", page_id: pageId },
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
}

function loaded(pageValue: NotesPage, blockValue: NotesBlock): NotesLoadedPage {
  return {
    page: pageValue,
    blocks: {
      object: "list",
      type: "block",
      block: {},
      results: [blockValue],
      next_cursor: null,
      has_more: false,
    },
  };
}

describe("Notes store command counts", () => {
  let notes: ReturnType<(typeof import("./notes.svelte"))["getNotes"]>;

  beforeAll(async () => {
    vi.useFakeTimers();
    const pageA = page(pageAId);
    const pageB = page(pageBId);
    backend.pages.set(pageAId, pageA);
    backend.pages.set(pageBId, pageB);
    backend.loadedPages.set(pageAId, loaded(pageA, block(blockAId, pageAId)));
    backend.loadedPages.set(pageBId, loaded(pageB, block(blockBId, pageBId)));
    const module = await import("./notes.svelte");
    notes = module.getNotes();
    await notes.load();
    await notes.selectPage(pageAId);
    backend.clear();
  });

  afterAll(() => {
    vi.useRealTimers();
  });

  it("bounds rename, archive, and restore reads to critical and coalesced metadata", async () => {
    await notes.renamePage(pageAId, "Renamed");
    expect(backend.count("update-page")).toBe(1);
    expect(backend.count("breadcrumb")).toBeLessThanOrEqual(1);
    expect(backend.count("page")).toBe(0);
    await vi.advanceTimersByTimeAsync(60);
    expect(backend.count("workspace")).toBe(0);
    expect(backend.count("sidebar")).toBe(0);
    expect(backend.count("all-pages")).toBe(0);
    expect(backend.count("folders")).toBe(0);

    backend.clear();
    await notes.archivePage(pageAId);
    expect(backend.count("archive")).toBe(1);
    expect(backend.count("page")).toBeLessThanOrEqual(1);
    expect(backend.count("backlinks")).toBe(0);
    expect(backend.count("aliases")).toBe(0);
    await vi.advanceTimersByTimeAsync(60);
    expect(backend.count("workspace")).toBe(1);

    backend.clear();
    await notes.unarchivePage(pageAId);
    expect(backend.count("restore-archive")).toBe(1);
    expect(backend.count("page")).toBe(1);
    expect(backend.count("breadcrumb")).toBe(0);
    expect(backend.count("backlinks")).toBe(0);
    await vi.advanceTimersByTimeAsync(60);
    expect(backend.count("workspace")).toBe(1);
  });

  it("loads link presence without destinations and ignores a late old-page backlink result", async () => {
    backend.clear();
    notes.setPagePanelSubsystemOpen("links", true);
    const oldPanel = notes.ensureOptionalSubsystem("links", pageAId);
    await Promise.resolve();
    expect(backend.count("backlinks")).toBe(1);
    expect(backend.count("aliases")).toBe(1);
    expect(backend.count("unresolved")).toBe(1);
    expect(backend.count("destinations")).toBe(0);
    await notes.reloadLinkResolutionPages();
    expect(backend.count("destinations")).toBe(1);

    await notes.selectPage(pageBId);
    const staleBacklink: NotesBacklink = {
      object: "backlink",
      id: "00000000-0000-4000-8000-000000000005",
      source_page: backend.pages.get(pageAId) as NotesPage,
      source_block_id: blockAId,
      source_block_type: "paragraph",
      reference_type: "link",
      snippet: "stale",
      created_time: now,
      last_edited_time: now,
    };
    backend.pendingBacklinks.shift()?.([staleBacklink]);
    await oldPanel;

    expect(notes.selectedPageId).toBe(pageBId);
    expect(notes.backlinks).toEqual([]);
  });

  it("ignores a completed search after a newer query", async () => {
    const oldSearch = notes.search("old");
    const newSearch = notes.search("new");
    await Promise.resolve();
    const newPending = backend.pendingSearches.find((pending) => pending.query === "new");
    newPending?.resolve({ results: [], next_cursor: null });
    await newSearch;
    const oldPending = backend.pendingSearches.find((pending) => pending.query === "old");
    oldPending?.resolve({
      results: [{
        object: "search_result",
        id: `page:${pageAId}`,
        type: "page",
        page: backend.pages.get(pageAId) as NotesPage,
        block_id: null,
        block_type: null,
        comment_id: null,
        discussion_id: null,
        comment_status: null,
        comment_author: null,
        comment_anchor: null,
        snippet: "stale",
        last_edited_time: now,
      }],
      next_cursor: "stale",
    });
    await oldSearch;

    expect(notes.searchResults).toEqual([]);
    expect(notes.searchHasMore).toBe(false);
  });
});
