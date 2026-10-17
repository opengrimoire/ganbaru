import { describe, expect, it, vi } from "vitest";
import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";
import {
  createProjectSettingsWorkingFoldersDraft,
  type ProjectSettingsWorkingFoldersPorts,
  type WorkingFolderSelection,
} from "./working-folders-draft.svelte";

/** Build an independent saved folder with revisioned configuration. */
function folder(id: string, kind: "managed" | "external" = "external"): ProjectWorkingFolderRead {
  return {
    workingFolder: { id, projectId: "project", displayName: id, kind, managedRelativePath: kind === "managed" ? "projects/project" : null,
      repositoryKind: "none", repositoryIdentity: null, sortOrder: 1, createdAt: "2026-01-01T00:00:00Z",
      updatedAt: "2026-01-01T00:00:00Z", archivedAt: null, revision: 1 },
    canonicalPath: `/folders/${id}`, bindingStatus: "available", lastVerifiedAt: null, currentBranch: null,
  };
}

/** Keep command spies separate from the immutable canonical fixtures. */
function setup() {
  const canonical = [folder("managed", "managed"), folder("external"), folder("other")];
  const selection: WorkingFolderSelection = { selectionId: "native-selection", canonicalPath: "/folders/chosen", displayName: "Chosen" };
  const ports = {
    pick: vi.fn<ProjectSettingsWorkingFoldersPorts["pick"]>().mockResolvedValue(selection),
    add: vi.fn<ProjectSettingsWorkingFoldersPorts["add"]>().mockImplementation(async (request, selected) => ({
      ...folder(request.id), canonicalPath: selected.canonicalPath, workingFolder: { ...folder(request.id).workingFolder, displayName: request.displayName },
    })),
    bind: vi.fn<ProjectSettingsWorkingFoldersPorts["bind"]>().mockImplementation(async (id, selected) => ({ ...folder(id), canonicalPath: selected.canonicalPath })),
    rename: vi.fn<ProjectSettingsWorkingFoldersPorts["rename"]>().mockImplementation(async (id, name, revision) => ({ ...folder(id), workingFolder: { ...folder(id).workingFolder, displayName: name, revision: revision + 1 } })),
    archive: vi.fn<ProjectSettingsWorkingFoldersPorts["archive"]>().mockImplementation(async (id, revision) => ({ ...folder(id), workingFolder: { ...folder(id).workingFolder, archivedAt: "2026-10-01T00:00:00Z", revision: revision + 1 } })),
    restore: vi.fn<ProjectSettingsWorkingFoldersPorts["restore"]>().mockImplementation(async (id, revision) => ({ ...folder(id), workingFolder: { ...folder(id).workingFolder, revision: revision + 1 } })),
    recreate: vi.fn<ProjectSettingsWorkingFoldersPorts["recreate"]>().mockImplementation(async (id) => folder(id, "managed")),
    remove: vi.fn<ProjectSettingsWorkingFoldersPorts["remove"]>().mockResolvedValue(undefined),
    provider: vi.fn<ProjectSettingsWorkingFoldersPorts["provider"]>().mockResolvedValue(undefined),
    primary: vi.fn<ProjectSettingsWorkingFoldersPorts["primary"]>().mockImplementation(async (projectId, id, revision) => ({ projectId, workingFolderId: id, revision: revision + 1 })),
  };
  const draft = createProjectSettingsWorkingFoldersDraft(ports);
  draft.load("project", canonical, { projectId: "project", workingFolderId: "managed", revision: 3 }, { external: "old-provider" });
  return { draft, ports, canonical, selection };
}

describe("Project settings working-folder drafts", () => {
  it("stages every configuration edit and discards it without touching canonical data", async () => {
    const { draft, ports, canonical } = setup();
    draft.rename(draft.folders[1], "Renamed");
    draft.setPreference("external", "new-provider");
    draft.makePrimary(draft.folders[1]);
    draft.setArchived(draft.folders[2], true);
    await draft.choose();
    draft.remove(draft.folders[2]);
    expect(draft.dirty).toBe(true);
    expect(canonical[1].workingFolder.displayName).toBe("external");
    expect(canonical[2].workingFolder.archivedAt).toBeNull();
    for (const [name, spy] of Object.entries(ports)) if (name !== "pick") expect(spy).not.toHaveBeenCalled();
    draft.discard();
    expect(draft.dirty).toBe(false);
    expect(draft.folders).toEqual(canonical);
    expect(draft.preference("external")).toBe("old-provider");
    expect(draft.primaryId).toBe("managed");
  });

  it("commits native receipts, revisions, providers and primary only on Save", async () => {
    const { draft, ports, selection } = setup();
    draft.rename(draft.folders[1], "Renamed");
    draft.setPreference("external", "new-provider");
    await draft.choose();
    const added = draft.folders.at(-1)!;
    draft.makePrimary(added);
    draft.setArchived(draft.folders[2], true);
    await draft.save();
    expect(ports.add).toHaveBeenCalledExactlyOnceWith({ id: added.workingFolder.id, projectId: "project", displayName: "Chosen" }, selection);
    expect(ports.rename).toHaveBeenCalledExactlyOnceWith("external", "Renamed", 1);
    expect(ports.provider).toHaveBeenCalledExactlyOnceWith("external", "new-provider");
    expect(ports.primary).toHaveBeenCalledExactlyOnceWith("project", added.workingFolder.id, 3);
    expect(ports.archive).toHaveBeenCalledExactlyOnceWith("other", 1);
    expect(draft.dirty).toBe(false);
    draft.discard();
    expect(draft.folders.find((entry) => entry.workingFolder.id === "external")?.workingFolder.displayName).toBe("Renamed");
  });

  it("ignores a picker result that arrives after Discard", async () => {
    const { draft, ports, selection } = setup();
    let finish: (value: WorkingFolderSelection) => void = () => { throw new Error("Picker has not started"); };
    ports.pick.mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const pending = draft.choose();
    draft.discard();
    finish(selection);
    await pending;
    expect(draft.folders).toHaveLength(3);
    expect(draft.dirty).toBe(false);
    expect(ports.add).not.toHaveBeenCalled();
  });

  it("retries a failed Save without creating the folder twice", async () => {
    const { draft, ports } = setup();
    await draft.choose();
    const added = draft.folders.at(-1)!;
    draft.setPreference(added.workingFolder.id, "provider");
    ports.provider.mockRejectedValueOnce(new Error("Provider write failed"));
    await expect(draft.save()).rejects.toThrow("Provider write failed");
    expect(draft.dirty).toBe(true);
    await draft.save();
    expect(ports.add).toHaveBeenCalledOnce();
    expect(ports.provider).toHaveBeenCalledTimes(2);
    expect(draft.dirty).toBe(false);
  });

  it("keeps a pending replacement path visible when a rename saves but rebinding fails", async () => {
    const { draft, ports, selection } = setup();
    draft.rename(draft.folders[1], "Renamed");
    await draft.choose(draft.folders[1]);
    ports.bind.mockRejectedValueOnce(new Error("Binding failed"));
    await expect(draft.save()).rejects.toThrow("Binding failed");
    expect(draft.folders.find((entry) => entry.workingFolder.id === "external")?.canonicalPath).toBe(selection.canonicalPath);
    expect(draft.dirty).toBe(true);
    await draft.save();
    expect(ports.rename).toHaveBeenCalledOnce();
    expect(ports.bind).toHaveBeenCalledTimes(2);
    expect(draft.dirty).toBe(false);
  });

  it("restores managed primary before removing the old primary", async () => {
    const { draft, ports, canonical } = setup();
    draft.load("project", canonical, { projectId: "project", workingFolderId: "external", revision: 6 }, {});
    draft.remove(draft.folders[1]);
    expect(draft.primaryId).toBe("managed");
    expect(ports.primary).not.toHaveBeenCalled();
    await draft.save();
    expect(ports.primary.mock.invocationCallOrder[0]).toBeLessThan(ports.remove.mock.invocationCallOrder[0]);
    expect(ports.remove).toHaveBeenCalledExactlyOnceWith("external");
  });

  it("removes an old association before adding the same path with a new identity", async () => {
    const { draft, ports, canonical, selection } = setup();
    draft.load("project", canonical, { projectId: "project", workingFolderId: "external", revision: 6 }, {});
    ports.pick.mockResolvedValue({ ...selection, canonicalPath: "/folders/external" });
    draft.remove(draft.folders[1]);
    await draft.choose();
    const added = draft.folders.at(-1)!;
    draft.makePrimary(added);
    await draft.save();
    expect(ports.primary).toHaveBeenNthCalledWith(1, "project", "managed", 6);
    expect(ports.remove.mock.invocationCallOrder[0]).toBeLessThan(ports.add.mock.invocationCallOrder[0]);
    expect(ports.primary).toHaveBeenNthCalledWith(2, "project", added.workingFolder.id, 7);
    expect(draft.dirty).toBe(false);
  });

  it("protects managed folders and stages recreation until Save", async () => {
    const { draft, ports, canonical } = setup();
    canonical[0].bindingStatus = "missing";
    draft.load("project", canonical, { projectId: "project", workingFolderId: "managed", revision: 3 }, {});
    const managed = draft.folders[0];
    draft.rename(managed, "Renamed"); draft.setArchived(managed, true); draft.remove(managed); await draft.choose(managed);
    expect(draft.dirty).toBe(false);
    draft.recreate(managed);
    expect(draft.dirty).toBe(true);
    expect(ports.recreate).not.toHaveBeenCalled();
    await draft.save();
    expect(ports.recreate).toHaveBeenCalledExactlyOnceWith("managed");
    expect(draft.dirty).toBe(false);
  });

  it("treats a change followed by its reversal as clean", async () => {
    const { draft, ports } = setup();
    draft.rename(draft.folders[1], "Renamed"); draft.rename(draft.folders[1], "external");
    draft.setPreference("external", "new-provider"); draft.setPreference("external", "old-provider");
    draft.makePrimary(draft.folders[1]); draft.makePrimary(draft.folders[0]);
    draft.setArchived(draft.folders[1], true); draft.setArchived(draft.folders[1], false);
    await draft.choose(); draft.remove(draft.folders.at(-1)!);
    expect(draft.dirty).toBe(false);
    await draft.save();
    expect(ports.add).not.toHaveBeenCalled();
    expect(ports.remove).not.toHaveBeenCalled();
    expect(ports.archive).not.toHaveBeenCalled();
  });
});
