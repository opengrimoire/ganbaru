import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { addSelectedProjectWorkingFolder, bindSelectedProjectWorkingFolder, pickProjectWorkingFolder } from "./project-working-folders";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:vault" }));

const selection = { selectionId: "native-receipt", canonicalPath: "/selected/path", displayName: "Folder" };
const folder = {
  workingFolder: {
    id: "folder", projectId: "project", displayName: "Folder", kind: "external", managedRelativePath: null,
    repositoryKind: "none", repositoryIdentity: null, sortOrder: 1, archivedAt: null, revision: 1,
    createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
  },
  canonicalPath: selection.canonicalPath, bindingStatus: "available", lastVerifiedAt: null, currentBranch: null,
};

beforeEach(() => { vi.resetAllMocks(); });

describe("staged project folder commands", () => {
  it("uses only the preview command when choosing or cancelling a path", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(selection).mockResolvedValueOnce(null);
    expect(await pickProjectWorkingFolder("project", "Choose folder", "folder")).toEqual(selection);
    expect(await pickProjectWorkingFolder("project", "Choose folder")).toBeNull();
    expect(invoke).toHaveBeenNthCalledWith(1, "projects_pick_working_folder", {
      dbUrl: "sqlite:vault", projectId: "project", title: "Choose folder", workingFolderId: "folder",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "projects_pick_working_folder", {
      dbUrl: "sqlite:vault", projectId: "project", title: "Choose folder", workingFolderId: null,
    });
  });

  it("rejects invalid native selection data before it can enter a draft", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...selection, selectionId: 1 });
    await expect(pickProjectWorkingFolder("project", "Choose folder")).rejects.toThrow("Invalid working-folder selection response");
    vi.mocked(invoke).mockResolvedValue({ ...selection, canonicalPath: "" });
    await expect(pickProjectWorkingFolder("project", "Choose folder")).rejects.toThrow("Invalid working-folder selection response");
  });

  it("commits an addition by native receipt rather than accepting an arbitrary path", async () => {
    vi.mocked(invoke).mockResolvedValue(folder);
    const request = { id: "folder", projectId: "project", displayName: "Folder" };
    expect(await addSelectedProjectWorkingFolder(request, selection)).toEqual(folder);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("projects_add_selected_working_folder", {
      dbUrl: "sqlite:vault", request, selectionId: selection.selectionId,
    });
  });

  it("commits rebinding only with the receipt for the existing folder", async () => {
    vi.mocked(invoke).mockResolvedValue(folder);
    await bindSelectedProjectWorkingFolder("folder", selection);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("projects_bind_selected_working_folder", {
      dbUrl: "sqlite:vault", workingFolderId: "folder", selectionId: selection.selectionId,
    });
  });
});
