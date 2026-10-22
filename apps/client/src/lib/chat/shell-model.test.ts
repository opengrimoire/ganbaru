import { describe, expect, it } from "vitest";
import type { ChatThreadShellRead, ProjectWorkingFolderRead, ProviderInstanceRead } from "./contracts";
import {
  buildChatRailModel,
  chatHeaderActionInset,
  chatHeaderShowsResourcePath,
  chatNavigationFolders,
  contextualChatFolders,
  filterThreadTitles,
  isDirectChatThreadShell,
  nextThreadIndex,
  partitionThreadSearchResults,
  resolveChatFirstUseState,
  siblingChatThreads,
  threadStatus,
} from "./shell-model";

const timestamp = "2026-07-20T12:00:00.000Z";

function workspace(id = "workspace", projectId = "project"): ProjectWorkingFolderRead {
  return {
    workingFolder: {
      id,
      projectId,
      displayName: id,
      kind: "external",
      managedRelativePath: null,
      sortOrder: 10,
      repositoryKind: "git",
      repositoryIdentity: "git-sha256:test",
      createdAt: timestamp,
      updatedAt: timestamp,
      archivedAt: null,
      revision: 1,
    },
    bindingStatus: "available",
    canonicalPath: "/workspace",
    lastVerifiedAt: timestamp,
    currentBranch: "feat/chat",
    trust: "trusted",
  };
}

function provider(state: ProviderInstanceRead["lastProbe"] extends infer _Probe ? "healthy" | "authentication_required" : never = "healthy"): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1,
      instanceId: "codex",
      familyId: "codex",
      label: "Codex",
      enabled: true,
      executable: "codex",
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      visibleModelIds: [],
      favoriteModelIds: [],
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: [],
    lastProbe: {
      instanceId: "codex",
      state,
      version: "1.0",
      negotiatedProtocolVersion: "2",
      accountLabel: null,
      capabilities: { entries: [] },
      authoritySupport: {
        isolatedConversation: true,
        internalHostTools: true,
        denyShell: true,
        readOnlyRoot: true,
        writableRoot: true,
        confinedCommands: true,
        networkBoundary: true,
        classifiedPublish: true,
      },
      checkedAt: timestamp,
      detail: null,
    },
    lastSuccessfulProbeAt: timestamp,
    modelCatalog: null,
  };
}

function thread(id = "thread", overrides: Partial<ChatThreadShellRead> = {}): ChatThreadShellRead {
  return {
    id,
    workingFolderId: "workspace",
    executionEnvironmentId: "current-folder:workspace",
    scratchGenerationId: null,
    projectId: "project",
    title: "Fix calendar",
    providerFamilyId: "codex",
    providerInstanceId: "codex",
    providerThreadId: null,
    modelId: null,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    state: "idle",
    latestTurnState: null,
    latestPreview: null,
    messageCount: 0,
    revision: 1,
    lastEventSequence: 0,
    lastActivityAt: timestamp,
    unreadAt: null,
    archivedAt: null,
    ...overrides,
  };
}

describe("Chat shell model", () => {
  it("keeps private scratch execution shells out of direct navigation", () => {
    expect(isDirectChatThreadShell(thread())).toBe(true);
    expect(isDirectChatThreadShell(thread("scratch", {
      workingFolderId: null,
      executionEnvironmentId: "scratch-environment:1",
      scratchGenerationId: "scratch-generation:1",
    }))).toBe(false);
  });

  it("routes every first-use and unavailable state precisely", () => {
    const base = { providers: [provider()], workingFolders: [workspace()], selectedProjectId: "project", selectedProjectArchived: false, selectedWorkingFolderId: "workspace", selectedThreadId: null, threads: [] };
    expect(resolveChatFirstUseState({ ...base, providers: [] }).kind).toBe("no_provider");
    expect(resolveChatFirstUseState({ ...base, selectedProjectId: null }).kind).toBe("no_project");
    expect(resolveChatFirstUseState({ ...base, selectedProjectArchived: true }).kind).toBe("archived_project");
    expect(resolveChatFirstUseState({ ...base, workingFolders: [] }).kind).toBe("no_project");
    expect(resolveChatFirstUseState({ ...base, workingFolders: [{ ...workspace(), bindingStatus: "missing" }] }).kind).toBe("missing_binding");
    expect(resolveChatFirstUseState(base).kind).toBe("no_thread");
    expect(resolveChatFirstUseState({ ...base, providers: [provider("authentication_required")] }).kind).toBe("provider_unavailable");
    expect(resolveChatFirstUseState({ ...base, providers: [provider("authentication_required"), { ...provider(), configuration: { ...provider().configuration, instanceId: "healthy" }, lastProbe: { ...provider().lastProbe!, instanceId: "healthy" } }] }).kind).toBe("no_thread");
    expect(resolveChatFirstUseState({ ...base, selectedThreadId: "thread", threads: [thread()] }).kind).toBe("conversation");
    expect(resolveChatFirstUseState({ ...base, providers: [], selectedThreadId: "thread", threads: [thread()] }).kind).toBe("conversation");
    expect(resolveChatFirstUseState({ ...base, providers: [provider("authentication_required")], selectedThreadId: "thread", threads: [thread()] }).kind).toBe("conversation");
    expect(resolveChatFirstUseState({ ...base, workingFolders: [{ ...workspace(), bindingStatus: "missing" }], selectedThreadId: "thread", threads: [thread()] }).kind).toBe("conversation");
    expect(resolveChatFirstUseState({ ...base, workingFolders: [{ ...workspace(), workingFolder: { ...workspace().workingFolder, archivedAt: timestamp } }], selectedThreadId: "thread", threads: [thread()] }).kind).toBe("conversation");
    expect(resolveChatFirstUseState({ ...base, providers: [provider("authentication_required")], selectedThreadId: "thread", threads: [thread("thread", { archivedAt: timestamp })] }).kind).toBe("archived_thread");
  });

  it("prioritizes actionable and active statuses over unread", () => {
    expect(threadStatus(thread("answer", { latestTurnState: "waiting_for_user_input", unreadAt: timestamp }))).toBe("waiting_answer");
    expect(threadStatus(thread("approval", { latestTurnState: "waiting_for_approval" }))).toBe("waiting_approval");
    expect(threadStatus(thread("active", { latestTurnState: "active" }))).toBe("working");
    expect(threadStatus(thread("failed", { latestTurnState: "failed" }))).toBe("error");
    expect(threadStatus(thread("unread", { unreadAt: timestamp }))).toBe("unread");
  });

  it("builds a project-scoped folder rail with managed, selected, active, and draft context", () => {
    const managed = workspace("workspace");
    managed.workingFolder.kind = "managed";
    managed.workingFolder.sortOrder = 30;
    const selectedEmpty = workspace("selected");
    const hiddenEmpty = workspace("hidden");
    const otherProject = workspace("other-project", "project-2");
    const model = buildChatRailModel(
      [hiddenEmpty, selectedEmpty, otherProject, managed, workspace("second")],
      [thread(), thread("other", { workingFolderId: "second" })],
      "project",
      "selected",
      null,
    );
    expect(model.folders.map((entry) => entry.workingFolder.workingFolder.id)).toEqual([
      "workspace",
      "second",
      "selected",
    ]);
    expect(model.folders.find((entry) => entry.workingFolder.workingFolder.id === "selected")?.hasDraft).toBe(true);
    expect(model.folders.find((entry) => entry.workingFolder.workingFolder.id === "second")?.threads.map((entry) => entry.id)).toEqual(["other"]);
  });

  it("builds complete hover navigation and sorted sibling chats", () => {
    const managed = workspace("managed");
    managed.workingFolder.kind = "managed";
    managed.workingFolder.sortOrder = 50;
    const external = workspace("external");
    external.workingFolder.sortOrder = 1;
    const archived = workspace("archived");
    archived.workingFolder.archivedAt = timestamp;
    expect(chatNavigationFolders([external, archived, managed], "project").map((entry) => entry.workingFolder.id)).toEqual([
      "managed",
      "external",
    ]);
    expect(siblingChatThreads([
      thread("older", { workingFolderId: "external", lastActivityAt: "2026-07-19T12:00:00.000Z" }),
      thread("newer", { workingFolderId: "external", lastActivityAt: "2026-07-21T12:00:00.000Z" }),
      thread("archived-thread", { workingFolderId: "external", archivedAt: timestamp }),
    ], "project", "external").map((entry) => entry.id)).toEqual(["newer", "older"]);
  });

  it("keeps only contextual folders in the ordinary explorer", () => {
    const managed = workspace("managed");
    managed.workingFolder.kind = "managed";
    expect(contextualChatFolders(
      [workspace("empty"), workspace("selected"), workspace("active"), managed],
      [thread("active-thread", { workingFolderId: "active" })],
      "project",
      "selected",
    ).map((entry) => entry.workingFolder.id)).toEqual(["managed", "active", "selected"]);
  });

  it("shows resource breadcrumbs only when the explorer is collapsed", () => {
    expect(chatHeaderShowsResourcePath(true)).toBe(false);
    expect(chatHeaderShowsResourcePath(false)).toBe(true);
  });

  it("keeps header actions attached to the panel edge until fixed actions require clearance", () => {
    expect(chatHeaderActionInset(400, 720, 12)).toBe(12);
    expect(chatHeaderActionInset(720, 720, 12)).toBe(12);
    expect(chatHeaderActionInset(768, 720, 12)).toBe(60);
    expect(chatHeaderActionInset(768, 720, -4)).toBe(48);
  });

  it("filters titles locally and wraps keyboard traversal", () => {
    expect(filterThreadTitles([thread(), thread("other", { title: "Añadir notas" })], "anadir").map((entry) => entry.id)).toEqual(["other"]);
    expect(nextThreadIndex(1, 2, "next")).toBe(0);
    expect(nextThreadIndex(0, 2, "previous")).toBe(1);
    expect(nextThreadIndex(-1, 0, "next")).toBe(-1);
  });

  it("partitions search results", () => {
    const active = thread("active");
    const archived = thread("archived", { archivedAt: timestamp, projectId: "project-2", title: "Archived calendar" });
    expect(partitionThreadSearchResults([archived, active])).toEqual({ active: [active], archived: [archived] });
  });
});
