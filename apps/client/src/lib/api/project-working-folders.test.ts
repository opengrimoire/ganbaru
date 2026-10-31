import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { addSelectedProjectWorkingFolder, bindSelectedProjectWorkingFolder, pickProjectWorkingFolder, removeProjectWorkingFolder } from "./project-working-folders";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:vault" }));

const selection = { selectionId: "native-receipt", canonicalPath: "/selected/path", displayName: "Folder" };
const folder = {
  workingFolder: {
    id: "folder", projectId: "project", displayName: "Folder", kind: "external", managedRelativePath: null,
    repositoryKind: "none", repositoryIdentity: null, sortOrder: 1, archivedAt: null, revision: 1,
    createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
  },
  canonicalPath: selection.canonicalPath, bindingStatus: "available", lastVerifiedAt: null, currentBranch: null, trust: "undecided",
};

const trustedFolder = { ...folder, trust: "trusted" };

beforeEach(() => { vi.restoreAllMocks(); vi.resetAllMocks(); });

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
    vi.mocked(invoke).mockResolvedValueOnce(folder).mockResolvedValueOnce(trustedFolder);
    const request = { id: "folder", projectId: "project", displayName: "Folder" };
    expect(await addSelectedProjectWorkingFolder(request, selection)).toEqual(trustedFolder);
    expect(invoke).toHaveBeenNthCalledWith(1, "projects_add_selected_working_folder", {
      dbUrl: "sqlite:vault", request, selectionId: selection.selectionId,
    });
  });

  it("commits rebinding only with the receipt for the existing folder", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(folder).mockResolvedValueOnce(trustedFolder);
    await bindSelectedProjectWorkingFolder("folder", selection);
    expect(invoke).toHaveBeenNthCalledWith(1, "projects_bind_selected_working_folder", {
      dbUrl: "sqlite:vault", workingFolderId: "folder", selectionId: selection.selectionId,
    });
  });
});

describe("trust decisions at binding", () => {
  it("asks natively once for the bound folder with localized prompt text", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(folder).mockResolvedValueOnce(trustedFolder);
    await bindSelectedProjectWorkingFolder("folder", selection);
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenNthCalledWith(2, "projects_decide_working_folder_trust", {
      dbUrl: "sqlite:vault",
      workingFolderId: "folder",
      prompt: {
        title: expect.any(String),
        message: expect.any(String),
        trustLabel: expect.any(String),
        untrustedLabel: expect.any(String),
        cancelLabel: expect.any(String),
      },
    });
  });

  it("keeps a successful binding when the decision is cancelled or fails", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(invoke).mockResolvedValueOnce(folder).mockResolvedValueOnce(null);
    expect(await bindSelectedProjectWorkingFolder("folder", selection)).toEqual(folder);
    vi.mocked(invoke).mockResolvedValueOnce(folder).mockRejectedValueOnce(new Error("git missing"));
    expect(await bindSelectedProjectWorkingFolder("folder", selection)).toEqual(folder);
  });
});

describe("external folder removal", () => {
  it("removes the folder from the active vault and passes a refusal through unchanged", async () => {
    const refusal = { code: "permission", message: "working folder is in use", retryable: true };
    vi.mocked(invoke).mockRejectedValueOnce(refusal);
    await expect(removeProjectWorkingFolder("folder")).rejects.toBe(refusal);
    expect(invoke).toHaveBeenCalledWith("projects_remove_working_folder", { dbUrl: "sqlite:vault", workingFolderId: "folder" });
  });
});
