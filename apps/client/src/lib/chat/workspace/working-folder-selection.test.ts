import { describe, expect, it } from "vitest";
import type { ProjectWorkingFolderRead } from "$lib/chat/contracts";
import {
  channelPanelWorkingFolders,
  preferredProjectWorkingFolder,
  resolveChannelPanelWorkingFolder,
} from "./working-folder-selection";

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
    currentBranch: null, trust: "undecided",
  };
}

function withFolder(
  entry: ProjectWorkingFolderRead,
  change: Partial<ProjectWorkingFolderRead["workingFolder"]>,
): ProjectWorkingFolderRead {
  return { ...entry, workingFolder: { ...entry.workingFolder, ...change } };
}

describe("channel panel working folders", () => {
  it("offers only the project's available unarchived folders in project order", () => {
    const app = withFolder(folder("app", "external", "available"), { sortOrder: 20 });
    const managed = folder("managed", "managed", "available");
    const unbound = folder("unbound", "external", "unbound");
    const archived = withFolder(folder("archived", "external", "available"), { archivedAt: "2026-07-26T12:00:00Z" });
    const otherProject = folder("other", "managed", "available", "project-2");
    expect(channelPanelWorkingFolders([app, unbound, managed, archived, otherProject], "project-1"))
      .toEqual([managed, app]);
    expect(channelPanelWorkingFolders([app, managed], null)).toEqual([]);
  });

  it("keeps the chosen folder while it stays bound and otherwise follows the channel default", () => {
    const managed = folder("managed", "managed", "available");
    const app = folder("app", "external", "available");
    expect(resolveChannelPanelWorkingFolder([managed, app], "app", "managed")).toBe("app");
    expect(resolveChannelPanelWorkingFolder([managed, app], null, "managed")).toBe("managed");
    expect(resolveChannelPanelWorkingFolder([managed], "app", "managed")).toBe("managed");
    expect(resolveChannelPanelWorkingFolder([], "app", null)).toBeNull();
  });
});

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
