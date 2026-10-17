import { describe, expect, it } from "vitest";
import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";
import { preferredProjectWorkingFolder } from "./working-folder-selection";

function folder(
  id: string,
  kind: "managed" | "external",
  bindingStatus: ProjectWorkingFolderRead["bindingStatus"],
  projectId = "project-1",
): ProjectWorkingFolderRead {
  return {
    workingFolder: {
      id,
      projectId,
      displayName: id,
      kind,
      managedRelativePath: kind === "managed" ? `projects/${projectId}` : null,
      sortOrder: kind === "managed" ? 0 : 10,
      repositoryKind: "none",
      repositoryIdentity: null,
      createdAt: "2026-07-25T12:00:00Z",
      updatedAt: "2026-07-25T12:00:00Z",
      archivedAt: null,
      revision: 1,
    },
    bindingStatus,
    canonicalPath: bindingStatus === "available" ? `/work/${id}` : null,
    lastVerifiedAt: null,
    currentBranch: null,
  };
}

describe("preferredProjectWorkingFolder", () => {
  it("restores an available remembered external folder", () => {
    const managed = folder("managed", "managed", "available");
    const external = folder("external", "external", "available");
    expect(preferredProjectWorkingFolder([managed, external], "project-1", "external"))
      .toBe(external);
  });

  it("falls back to the available managed folder when the remembered folder is missing", () => {
    const managed = folder("managed", "managed", "available");
    const external = folder("external", "external", "missing");
    expect(preferredProjectWorkingFolder([external, managed], "project-1", "external"))
      .toBe(managed);
  });

  it("keeps the managed folder selected for recovery when none are available", () => {
    const managed = folder("managed", "managed", "missing");
    const external = folder("external", "external", "unbound");
    expect(preferredProjectWorkingFolder([external, managed], "project-1", null))
      .toBe(managed);
  });

  it("never selects a folder from another project", () => {
    expect(preferredProjectWorkingFolder([
      folder("other", "managed", "available", "project-2"),
    ], "project-1", "other")).toBeNull();
  });
});
