import { describe, expect, it } from "vitest";
import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";
import { orderProjectWorkingFolders } from "$lib/projects/working-folder-order";

function folder(
  id: string,
  kind: "managed" | "external",
  sortOrder: number,
): ProjectWorkingFolderRead {
  return {
    workingFolder: {
      id,
      projectId: "project",
      displayName: id,
      kind,
      managedRelativePath: kind === "managed" ? "projects/project" : null,
      sortOrder,
      repositoryKind: "none",
      repositoryIdentity: null,
      createdAt: "2026-07-25T12:00:00.000Z",
      updatedAt: "2026-07-25T12:00:00.000Z",
      archivedAt: null,
      revision: 1,
    },
    bindingStatus: "available",
    canonicalPath: "/folder",
    lastVerifiedAt: "2026-07-25T12:00:00.000Z",
    currentBranch: null, trust: "undecided",
  };
}

describe("project working-folder order", () => {
  it("keeps the managed folder first and orders external folders predictably", () => {
    const ordered = orderProjectWorkingFolders([
      folder("later", "external", 20),
      folder("managed", "managed", 100),
      folder("earlier", "external", 10),
    ]);
    expect(ordered.map((entry) => entry.workingFolder.id)).toEqual([
      "managed",
      "earlier",
      "later",
    ]);
  });
});
