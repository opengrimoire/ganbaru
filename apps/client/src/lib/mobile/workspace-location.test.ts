import { describe, expect, it, vi } from "vitest";
import type { WorkspaceLocationAdapter } from "$lib/stores/workspace-tabs.svelte";
import type { WorkspaceTabLocation } from "$lib/workspace-tabs/model";
import { createDeferredWorkspaceLocationAdapter } from "./workspace-location.svelte";

const notesPage: WorkspaceTabLocation = { kind: "notes", projectId: "project-1", item: { type: "notes-page", id: "page-1" } };

function loadedAdapter(): WorkspaceLocationAdapter {
  return {
    read: (kind) => ({ kind, projectId: "project-1", item: null }),
    title: () => "Roadmap",
    apply: vi.fn(() => Promise.resolve()),
  };
}

describe("createDeferredWorkspaceLocationAdapter", () => {
  it("reads nothing for store-backed kinds until a restore loads the stores", async () => {
    const inner = loadedAdapter();
    const adapter = createDeferredWorkspaceLocationAdapter(() => Promise.resolve(inner));

    expect(adapter.read("calendar")).toEqual({ kind: "calendar", projectId: null, item: null });
    expect(adapter.read("projects")).toBeNull();
    expect(adapter.title("project", notesPage)).toBeNull();

    await adapter.apply(notesPage, () => true);

    expect(inner.apply).toHaveBeenCalledWith(notesPage, expect.any(Function));
    expect(adapter.read("projects")).toEqual({ kind: "projects", projectId: "project-1", item: null });
    expect(adapter.title("project", notesPage)).toBe("Roadmap");
  });

  it("restores Calendar without loading the stores", async () => {
    const load = vi.fn(() => Promise.resolve(loadedAdapter()));
    const adapter = createDeferredWorkspaceLocationAdapter(load);

    await adapter.apply({ kind: "calendar", projectId: null, item: null }, () => true);

    expect(load).not.toHaveBeenCalled();
  });

  it("loads once for overlapping restores and skips a restore that went stale while loading", async () => {
    const inner = loadedAdapter();
    const load = vi.fn(() => Promise.resolve(inner));
    const adapter = createDeferredWorkspaceLocationAdapter(load);

    await Promise.all([
      adapter.apply(notesPage, () => false),
      adapter.apply({ ...notesPage, kind: "chat" }, () => true),
    ]);

    expect(load).toHaveBeenCalledTimes(1);
    expect(inner.apply).toHaveBeenCalledTimes(1);
    expect(inner.apply).toHaveBeenCalledWith({ ...notesPage, kind: "chat" }, expect.any(Function));
  });

  it("retries the load on the next restore after a failure", async () => {
    const inner = loadedAdapter();
    const load = vi.fn<() => Promise<WorkspaceLocationAdapter>>()
      .mockRejectedValueOnce(new Error("chunk failed"))
      .mockResolvedValueOnce(inner);
    const adapter = createDeferredWorkspaceLocationAdapter(load);

    await expect(adapter.apply(notesPage, () => true)).rejects.toThrow("chunk failed");
    expect(adapter.read("notes")).toBeNull();

    await adapter.apply(notesPage, () => true);

    expect(load).toHaveBeenCalledTimes(2);
    expect(adapter.read("notes")).not.toBeNull();
  });
});
