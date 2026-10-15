import { describe, expect, it, vi } from "vitest";
import { createWorkspaceLocationAdapter, type WorkspaceLocationStores } from "./location-adapter";

interface FakeView {
  id: string;
  projectId: string;
}

function fakeStores(options: {
  projectId?: string | null;
  projects?: string[];
  views?: FakeView[];
  pageId?: string | null;
  channelId?: string | null;
} = {}) {
  const state = {
    projectId: options.projectId ?? null,
    viewIds: {} as Record<string, string>,
    pageId: options.pageId ?? null,
    channelId: options.channelId ?? null,
  };
  const knownProjects = new Set(options.projects ?? []);
  const views = options.views ?? [];
  const stores = {
    projects: {
      get selectedProjectId() { return state.projectId; },
      get activeViewId() { return state.projectId ? state.viewIds[state.projectId] ?? null : null; },
      ensureLoaded: vi.fn(async () => undefined),
      hasProject: (id: string) => knownProjects.has(id),
      projectName: (id: string) => (knownProjects.has(id) ? `Project ${id}` : null),
      viewName: (projectId: string, viewId: string) => (
        views.some((view) => view.projectId === projectId && view.id === viewId) ? `View ${viewId}` : null
      ),
      viewsForProject: (id: string) => views.filter((view) => view.projectId === id),
      selectProject: vi.fn(async (id: string) => { state.projectId = id; }),
      selectView: vi.fn((view: FakeView) => { state.viewIds[view.projectId] = view.id; }),
    },
    notes: {
      get selectedPageId() { return state.pageId; },
      selectPage: vi.fn(async (id: string) => { state.pageId = id; }),
      pageTitle: (id: string) => (id === "page" ? "Page title" : null),
    },
    chat: {
      get selectedChannelId() { return state.channelId; },
      selectChannel: vi.fn(async (id: string) => { state.channelId = id; }),
      channelName: (projectId: string, id: string) => (projectId === "p1" && id === "channel" ? "general" : null),
    },
  } satisfies WorkspaceLocationStores<FakeView>;
  return { state, stores };
}

describe("workspace location adapter", () => {
  it("reads Calendar as the bare kind and the other kinds with their project and item", () => {
    const { state, stores } = fakeStores({ projectId: "p1", pageId: "page", channelId: "channel" });
    state.viewIds.p1 = "v1";
    const adapter = createWorkspaceLocationAdapter(stores);

    expect(adapter.read("calendar")).toEqual({ kind: "calendar", projectId: null, item: null });
    expect(adapter.read("projects")).toEqual({ kind: "projects", projectId: "p1", item: { type: "project-view", id: "v1" } });
    expect(adapter.read("notes")).toEqual({ kind: "notes", projectId: "p1", item: { type: "notes-page", id: "page" } });
    expect(adapter.read("chat")).toEqual({ kind: "chat", projectId: "p1", item: { type: "chat-channel", id: "channel" } });
  });

  it("names loaded projects and nothing for unknown ones", () => {
    const { stores } = fakeStores({ projects: ["p1"] });
    const adapter = createWorkspaceLocationAdapter(stores);

    expect(adapter.projectName("p1")).toBe("Project p1");
    expect(adapter.projectName("gone")).toBeNull();
  });

  it("titles items with the view, page, or channel name, and nothing for unknown items or locations without one", () => {
    const { stores } = fakeStores({ projects: ["p1"], views: [{ id: "v1", projectId: "p1" }] });
    const adapter = createWorkspaceLocationAdapter(stores);

    expect(adapter.itemTitle({ kind: "projects", projectId: "p1", item: { type: "project-view", id: "v1" } }))
      .toBe("View v1");
    expect(adapter.itemTitle({ kind: "notes", projectId: null, item: { type: "notes-page", id: "page" } }))
      .toBe("Page title");
    expect(adapter.itemTitle({ kind: "chat", projectId: "p1", item: { type: "chat-channel", id: "channel" } }))
      .toBe("general");
    expect(adapter.itemTitle({ kind: "projects", projectId: "p2", item: { type: "project-view", id: "v1" } }))
      .toBeNull();
    expect(adapter.itemTitle({ kind: "notes", projectId: "p1", item: { type: "notes-folder", id: "f" } }))
      .toBeNull();
    expect(adapter.itemTitle({ kind: "chat", projectId: "p1", item: null })).toBeNull();
  });

  it("reads no item while nothing is selected", () => {
    const { stores } = fakeStores();
    const adapter = createWorkspaceLocationAdapter(stores);

    expect(adapter.read("projects")).toEqual({ kind: "projects", projectId: null, item: null });
    expect(adapter.read("chat")).toEqual({ kind: "chat", projectId: null, item: null });
  });

  it("restores the project and then its view", async () => {
    const { state, stores } = fakeStores({
      projectId: "p1",
      projects: ["p1", "p2"],
      views: [{ id: "v2", projectId: "p2" }, { id: "v3", projectId: "p2" }],
    });
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "projects", projectId: "p2", item: { type: "project-view", id: "v3" } }, () => true);

    expect(state.projectId).toBe("p2");
    expect(state.viewIds.p2).toBe("v3");
  });

  it("leaves the stores alone for a project that no longer exists", async () => {
    const { state, stores } = fakeStores({ projectId: "p1", projects: ["p1"] });
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "chat", projectId: "gone", item: { type: "chat-channel", id: "c" } }, () => true);

    expect(state.projectId).toBe("p1");
    expect(stores.chat.selectChannel).not.toHaveBeenCalled();
  });

  it("skips a view that is not in the restored project", async () => {
    const { state, stores } = fakeStores({ projects: ["p1"], views: [{ id: "other", projectId: "p2" }] });
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "projects", projectId: "p1", item: { type: "project-view", id: "other" } }, () => true);

    expect(state.projectId).toBe("p1");
    expect(stores.projects.selectView).not.toHaveBeenCalled();
  });

  it("stops before selecting anything once the restore is superseded", async () => {
    const { stores } = fakeStores({ projects: ["p1"] });
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "notes", projectId: "p1", item: { type: "notes-page", id: "page" } }, () => false);

    expect(stores.projects.selectProject).not.toHaveBeenCalled();
    expect(stores.notes.selectPage).not.toHaveBeenCalled();
  });

  it("does not reselect a page or channel that is already open", async () => {
    const { stores } = fakeStores({ projectId: "p1", projects: ["p1"], pageId: "page", channelId: "channel" });
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "notes", projectId: "p1", item: { type: "notes-page", id: "page" } }, () => true);
    await adapter.apply({ kind: "chat", projectId: "p1", item: { type: "chat-channel", id: "channel" } }, () => true);

    expect(stores.projects.ensureLoaded).not.toHaveBeenCalled();
    expect(stores.notes.selectPage).not.toHaveBeenCalled();
    expect(stores.chat.selectChannel).not.toHaveBeenCalled();
  });

  it("restores a page without a project", async () => {
    const { state, stores } = fakeStores();
    const adapter = createWorkspaceLocationAdapter(stores);

    await adapter.apply({ kind: "notes", projectId: null, item: { type: "notes-page", id: "page" } }, () => true);

    expect(state.pageId).toBe("page");
  });
});
