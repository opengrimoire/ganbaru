// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import type {
  NotesPage,
  NotesPageSummaryWindow,
  NotesSearchWindow,
  NotesWorkspaceShell,
} from "$lib/notes/types";

const backend = vi.hoisted(() => {
  function queue<T>() {
    const pending: Array<{ resolve: (value: T) => void; reject: (error: Error) => void }> = [];
    return {
      next: vi.fn(() => new Promise<T>((resolve, reject) => pending.push({ resolve, reject }))),
      resolve(index: number, value: T): void {
        pending[index]?.resolve(value);
      },
      reject(index: number, error: Error): void {
        pending[index]?.reject(error);
      },
    };
  }
  return {
    archive: queue<NotesPageSummaryWindow>(),
    search: queue<NotesSearchWindow>(),
    destinations: queue<NotesWorkspaceShell>(),
  };
});

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    listArchivedNotesPages: backend.archive.next,
    listTrashedNotesPages: vi.fn(),
    searchNotes: backend.search.next,
    listNotesDestinationCandidates: backend.destinations.next,
  };
});

function destinationPage(id: string, folderId: string | null): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-03T00:00:00.000Z",
    last_edited_time: "2026-07-03T00:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: folderId,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {},
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function destinationShell(pages: NotesPage[]): NotesWorkspaceShell {
  return {
    pages,
    folders: [],
    navigation_pages: [],
    navigation_folders: [],
    navigation_page_ids_with_children: [],
    navigation_databases: [],
    page_ids_with_children: [],
    missing_parent_page_ids: [],
    trashed_parent_page_ids: [],
    resolved_selected_page_id: null,
    total_page_count: pages.length,
    total_folder_count: 0,
    next_page_cursor: null,
    next_folder_cursor: null,
  };
}

describe("Notes query controllers", () => {
  it("rejects a stale archive reload", async () => {
    const { createNotesArchiveController } = await import("./archive.svelte");
    const controller = createNotesArchiveController();
    const stale = controller.reloadArchivedPages("old");
    const current = controller.reloadArchivedPages("new");
    backend.archive.resolve(1, { pages: [], next_cursor: "current", total_count: 0 });
    await current;
    backend.archive.resolve(0, { pages: [], next_cursor: "stale", total_count: 0 });
    await stale;

    expect(controller.archiveHasMore).toBe(true);
    const more = controller.loadMoreArchivedPages();
    expect(backend.archive.next).toHaveBeenLastCalledWith({
      cursor: "current",
      query: "new",
    });
    backend.archive.resolve(2, { pages: [], next_cursor: null, total_count: 0 });
    await more;
  });

  it("rejects stale search results", async () => {
    const { createNotesSearchController } = await import("./search.svelte");
    const controller = createNotesSearchController();
    const stale = controller.search("old");
    const current = controller.search("new");
    backend.search.resolve(1, { results: [], next_cursor: "current" });
    await current;
    backend.search.resolve(0, { results: [], next_cursor: "stale" });
    await stale;

    expect(controller.hasMore).toBe(true);
    const more = controller.loadMoreSearchResults();
    expect(backend.search.next).toHaveBeenLastCalledWith("new", 20, false, "current");
    backend.search.resolve(2, { results: [], next_cursor: null });
    await more;
  });

  it("loads later search pages with the resolved-comment filter of the original search", async () => {
    const { createNotesSearchController } = await import("./search.svelte");
    const controller = createNotesSearchController();
    const first = controller.search("tasks", 10, true);
    backend.search.resolve(backend.search.next.mock.calls.length - 1, { results: [], next_cursor: "next" });
    await first;

    const more = controller.loadMoreSearchResults();
    expect(backend.search.next).toHaveBeenLastCalledWith("tasks", 10, true, "next");
    backend.search.resolve(backend.search.next.mock.calls.length - 1, { results: [], next_cursor: null });
    await more;
  });

  it("reports a failed archive page load without discarding loaded pages", async () => {
    const { createNotesArchiveController } = await import("./archive.svelte");
    const controller = createNotesArchiveController();
    const page = destinationPage("archived", null);
    const reload = controller.reloadArchivedPages();
    backend.archive.resolve(backend.archive.next.mock.calls.length - 1, { pages: [page], next_cursor: "next", total_count: 2 });
    await reload;

    const more = controller.loadMoreArchivedPages();
    backend.archive.reject(backend.archive.next.mock.calls.length - 1, new Error("Storage unavailable"));
    await expect(more).resolves.toBeUndefined();
    expect(controller.archiveError).toBe("Storage unavailable");
    expect(controller.archiveLoading).toBe(false);
    expect(controller.archivedPages).toEqual([page]);
    expect(controller.archiveHasMore).toBe(true);
  });

  it("keeps destination candidates and their cursor when a later page fails", async () => {
    const { createNotesLinksController } = await import("./links.svelte");
    const page = destinationPage("page-a", null);
    const controller = createNotesLinksController({
      readSelectedPageId: () => null,
      readSelectedProjectId: () => "project-a",
      readAllPages: () => [page],
      reloadSelectedPage: async () => undefined,
      scheduleVisibleMetadataRefresh: () => undefined,
    });
    const reload = controller.reloadLinkResolutionPages();
    backend.destinations.resolve(backend.destinations.next.mock.calls.length - 1, { ...destinationShell([page]), next_page_cursor: "next" });
    await reload;

    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const more = controller.loadMoreDestinationCandidates();
    backend.destinations.reject(backend.destinations.next.mock.calls.length - 1, new Error("Storage unavailable"));
    await expect(more).resolves.toBeUndefined();
    expect(controller.destinations).toEqual([page]);
    expect(controller.destinationHasMore).toBe(true);
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });

  it("keeps changed and removed pages current when a destination read finishes late", async () => {
    const { createNotesLinksController } = await import("./links.svelte");
    const stale = destinationPage("page-a", "old-folder");
    const current = destinationPage("page-a", "new-folder");
    const extra = destinationPage("page-b", null);
    let workspacePages: NotesPage[] = [stale];
    const controller = createNotesLinksController({
      readSelectedPageId: () => null,
      readSelectedProjectId: () => "project-a",
      readAllPages: () => workspacePages,
      reloadSelectedPage: async () => undefined,
      scheduleVisibleMetadataRefresh: () => undefined,
    });

    const pending = controller.reloadLinkResolutionPages();
    workspacePages = [current];
    controller.reconcileDestinationPages([current], []);
    backend.destinations.resolve(backend.destinations.next.mock.calls.length - 1, destinationShell([stale, extra]));
    await pending;
    expect(controller.destinations).toEqual([current, extra]);

    const movedAgain = destinationPage("page-a", "latest-folder");
    workspacePages = [movedAgain];
    controller.reconcileDestinationPages([movedAgain], []);
    expect(controller.destinations).toEqual([movedAgain, extra]);

    workspacePages = [];
    controller.reconcileDestinationPages([], [current.id]);
    expect(controller.destinations).toEqual([extra]);
    const late = controller.reloadLinkResolutionPages();
    backend.destinations.resolve(backend.destinations.next.mock.calls.length - 1, destinationShell([stale, extra]));
    await late;
    expect(controller.destinations).toEqual([extra]);
  });
});
