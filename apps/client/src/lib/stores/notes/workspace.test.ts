import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NotesWorkspaceShell } from "$lib/notes/types";

const backend = vi.hoisted(() => {
  const resolvers: Array<(shell: NotesWorkspaceShell) => void> = [];
  return {
    resolvers,
    load: vi.fn(() => new Promise<NotesWorkspaceShell>((resolve) => resolvers.push(resolve))),
  };
});

vi.mock("$lib/api/notes", () => ({
  loadNotesWorkspaceShell: backend.load,
}));

function shell(cursor: string): NotesWorkspaceShell {
  return {
    pages: [],
    folders: [],
    navigation_pages: [],
    navigation_folders: [],
    navigation_page_ids_with_children: [],
    navigation_databases: [],
    page_ids_with_children: [],
    missing_parent_page_ids: [],
    trashed_parent_page_ids: [],
    resolved_selected_page_id: null,
    total_page_count: 0,
    total_folder_count: 0,
    next_page_cursor: cursor,
    next_folder_cursor: null,
  };
}

describe("Notes workspace reloads", () => {
  beforeEach(() => {
    backend.resolvers.length = 0;
    backend.load.mockClear();
  });

  it("cancels old project pagination while refreshing the destination sidebar without reopening its note", async () => {
    const { createNotesWorkspaceController } = await import("./workspace.svelte");
    let projectId = "project-a";
    const applyAdditionalShell = vi.fn();
    const mergeReloadedShell = vi.fn();
    const prepareLoad = vi.fn();
    const loadSelectedPage = vi.fn(async () => undefined);
    const controller = createNotesWorkspaceController({
      readRequestState: () => ({ projectId, expandedPageIds: [], seedPageIds: [], selectedPageId: "note" }),
      readNavigationMutationRevision: () => 0,
      prepareLoad, applyInitialShell: () => null, applyAdditionalShell, mergeReloadedShell,
      loadSelectedPage, clearSelectedPageState: vi.fn(), readSelectedPageId: () => "note",
    });
    const initial = controller.load();
    backend.resolvers[0]?.(shell("project-a-next"));
    await initial;
    const stalePagination = controller.loadMoreWorkspaceWindow();
    projectId = "project-b";
    const refresh = controller.refreshProjectNavigation();
    expect(backend.load).toHaveBeenLastCalledWith(expect.objectContaining({ project_id: "project-b", selected_page_id: "note" }));
    await controller.loadMoreWorkspaceWindow();
    expect(backend.load).toHaveBeenCalledTimes(3);
    backend.resolvers[1]?.(shell("stale-next"));
    await stalePagination;
    expect(applyAdditionalShell).not.toHaveBeenCalled();
    const fresh = shell("project-b-next");
    backend.resolvers[2]?.(fresh);
    await refresh;
    expect(mergeReloadedShell).toHaveBeenCalledExactlyOnceWith(fresh);
    expect(prepareLoad).toHaveBeenCalledOnce();
    expect(loadSelectedPage).not.toHaveBeenCalled();
    const more = controller.loadMoreWorkspaceWindow();
    expect(backend.load).toHaveBeenLastCalledWith(expect.objectContaining({ project_id: "project-b", page_cursor: "project-b-next" }));
    backend.resolvers[3]?.(shell("end"));
    await more;
  });

  it("drops earlier sidebar reads even when navigation returns to the same project", async () => {
    const { createNotesWorkspaceController } = await import("./workspace.svelte");
    let projectId = "project-a";
    const mergeReloadedShell = vi.fn();
    const controller = createNotesWorkspaceController({
      readRequestState: () => ({ projectId, expandedPageIds: [], seedPageIds: [], selectedPageId: "note" }),
      readNavigationMutationRevision: () => 0,
      prepareLoad: vi.fn(), applyInitialShell: () => null,
      applyAdditionalShell: vi.fn(), mergeReloadedShell,
      loadSelectedPage: async () => undefined, clearSelectedPageState: vi.fn(), readSelectedPageId: () => "note",
    });
    const old = controller.refreshProjectNavigation();
    projectId = "project-b";
    const intermediate = controller.refreshProjectNavigation();
    projectId = "project-a";
    const current = controller.refreshProjectNavigation();
    const fresh = shell("latest-a");
    backend.resolvers[2]?.(fresh);
    await current;
    backend.resolvers[0]?.(shell("old-a"));
    backend.resolvers[1]?.(shell("old-b"));
    await Promise.all([old, intermediate]);
    expect(mergeReloadedShell).toHaveBeenCalledExactlyOnceWith(fresh);
  });

  it("keeps concurrent startup callers waiting until the selected note has loaded", async () => {
    const { createNotesWorkspaceController } = await import("./workspace.svelte");
    let finishPage!: () => void;
    const loadSelectedPage = vi.fn(() => new Promise<void>((resolve) => { finishPage = resolve; }));
    const controller = createNotesWorkspaceController({
      readRequestState: () => ({ projectId: null, expandedPageIds: [], seedPageIds: [], selectedPageId: "page" }),
      readNavigationMutationRevision: () => 0,
      prepareLoad: vi.fn(), applyInitialShell: () => "page",
      applyAdditionalShell: vi.fn(), mergeReloadedShell: vi.fn(),
      loadSelectedPage, clearSelectedPageState: vi.fn(), readSelectedPageId: () => "page",
    });
    const first = controller.ensureLoaded();
    backend.resolvers[0]?.(shell("end"));
    await vi.waitFor(() => expect(loadSelectedPage).toHaveBeenCalledOnce());
    const ready = vi.fn();
    const second = controller.ensureLoaded().then(ready);
    await Promise.resolve();
    expect(ready).not.toHaveBeenCalled();
    expect(backend.load).toHaveBeenCalledOnce();
    finishPage();
    await Promise.all([first, second]);
    expect(ready).toHaveBeenCalledOnce();
  });

  it("rereads a shell that began before a local navigation mutation", async () => {
    const { createNotesWorkspaceController } = await import("./workspace.svelte");
    let mutationRevision = 0;
    const mergeReloadedShell = vi.fn();
    const controller = createNotesWorkspaceController({
      readRequestState: () => ({
        projectId: "project-a",
        expandedPageIds: [],
        seedPageIds: [],
        selectedPageId: null,
      }),
      readNavigationMutationRevision: () => mutationRevision,
      prepareLoad: vi.fn(),
      applyInitialShell: () => null,
      applyAdditionalShell: vi.fn(),
      mergeReloadedShell,
      loadSelectedPage: async () => undefined,
      clearSelectedPageState: vi.fn(),
      readSelectedPageId: () => null,
    });

    const pending = controller.reloadPages();
    mutationRevision += 1;
    backend.resolvers[0]?.(shell("stale"));
    await vi.waitFor(() => expect(backend.load).toHaveBeenCalledTimes(2));
    expect(mergeReloadedShell).not.toHaveBeenCalled();

    const fresh = shell("fresh");
    backend.resolvers[1]?.(fresh);
    await pending;
    expect(mergeReloadedShell).toHaveBeenCalledExactlyOnceWith(fresh);
  });

  it("drops stale pagination results and resets its cursor from the fresh shell", async () => {
    const { createNotesWorkspaceController } = await import("./workspace.svelte");
    let mutationRevision = 0;
    const applyAdditionalShell = vi.fn();
    const mergeReloadedShell = vi.fn();
    const controller = createNotesWorkspaceController({
      readRequestState: () => ({
        projectId: "project-a",
        expandedPageIds: [],
        seedPageIds: [],
        selectedPageId: null,
      }),
      readNavigationMutationRevision: () => mutationRevision,
      prepareLoad: vi.fn(),
      applyInitialShell: () => null,
      applyAdditionalShell,
      mergeReloadedShell,
      loadSelectedPage: async () => undefined,
      clearSelectedPageState: vi.fn(),
      readSelectedPageId: () => null,
    });

    const initial = controller.load();
    backend.resolvers[0]?.(shell("initial-next"));
    await initial;
    const pagination = controller.loadMoreWorkspaceWindow();
    mutationRevision += 1;
    backend.resolvers[1]?.(shell("stale-next"));
    await vi.waitFor(() => expect(backend.load).toHaveBeenCalledTimes(3));
    expect(applyAdditionalShell).not.toHaveBeenCalled();

    const fresh = shell("fresh-next");
    backend.resolvers[2]?.(fresh);
    await pagination;
    expect(mergeReloadedShell).toHaveBeenCalledExactlyOnceWith(fresh);
    expect(controller.totalPageCount).toBe(fresh.total_page_count);

    const nextPage = controller.loadMoreWorkspaceWindow();
    expect(backend.load).toHaveBeenLastCalledWith(expect.objectContaining({
      page_cursor: "fresh-next",
    }));
    backend.resolvers[3]?.(shell("end"));
    await nextPage;
  });
});
