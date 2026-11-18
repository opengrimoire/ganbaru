import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatChannelRead,
  ChatSettingsRead,
  ProjectWorkingFolderRead,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
  ChatAiAgentRead,
  ChatAgentAccessRead,
} from "$lib/chat/contracts";
import {
  createAgentEditorController,
  type AgentEditorOptions,
} from "./editor-controller.svelte";

const api = vi.hoisted(() => ({
  listChatAgents: vi.fn<typeof import("$lib/api/chat").listChatAgents>(),
  listChatNavigationChannels: vi.fn<typeof import("$lib/api/chat").listChatNavigationChannels>(),
  readChatAgent: vi.fn<typeof import("$lib/api/chat").readChatAgent>(),
  readChatAgentAccess: vi.fn<typeof import("$lib/api/chat").readChatAgentAccess>(),
  createChatAgent: vi.fn<typeof import("$lib/api/chat").createChatAgent>(),
  previewChatAgentAccess: vi.fn<typeof import("$lib/api/chat").previewChatAgentAccess>(),
  replaceChatAgentAccess: vi.fn<typeof import("$lib/api/chat").replaceChatAgentAccess>(),
}));

vi.mock("$lib/api/chat", () => api);

const CREATED_AT = "2026-09-18T12:00:00Z";

function agent(id = "participant:atlas", revision = 1): ChatAiAgentRead {
  return {
    participant: {
      id, kind: "ai_agent", displayName: id, avatar: { schemaVersion: 1, value: { kind: "initials" } },
      revision, archivedAt: null,
    },
    role: "Engineer",
    instructions: "Build carefully",
    configurationState: "healthy",
    latestPolicy: {
      id: "policy:atlas", agentId: id, revision,
      providerInstanceId: "provider:codex", safetyMode: "ask_for_approval",
      providerManagedModel: false, modelId: "model:test", modelOptions: [],
      effort: null, speed: null,
      providerOptions: { schemaVersion: 1, value: {} },
      createdAt: CREATED_AT,
    },
    channelCount: 0,
    activeAssignmentCount: 0,
  };
}

function access(agentId = "participant:atlas", accessRevision = 1): ChatAgentAccessRead {
  return { agentId, accessRevision, agentDefaultRuntimeApproval: "ask", channels: [] };
}

function setup(agents = [agent()], overrides: Partial<AgentEditorOptions["chat"]> = {}) {
  const chat: AgentEditorOptions["chat"] = {
    agents,
    archivedAgents: [],
    settings: null,
    workingFolders: [],
    primaryWorkingFolder: null,
    refreshAgents: vi.fn(async () => {}),
    ...overrides,
  };
  const focusConflict = vi.fn(async () => {});
  const focusRecoveryNotice = vi.fn();
  const editor = createAgentEditorController({
    chat,
    t: (key: string, ..._args: unknown[]) => key,
    initialChannelId: () => null,
    closeAccessPicker: vi.fn(),
    setExpandedAccessChannel: vi.fn(),
    focusConflict,
    focusRecoveryNotice,
  });
  return { editor, chat, focusConflict, focusRecoveryNotice };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("agent editor workflow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.readChatAgentAccess.mockImplementation(async (id) => access(id));
    api.listChatAgents.mockResolvedValue([]);
    api.listChatNavigationChannels.mockResolvedValue([]);
    api.previewChatAgentAccess.mockResolvedValue({
      proposed: null, isExpansion: true, addedChannelIds: [], removedChannelIds: [], issues: [],
    });
    api.replaceChatAgentAccess.mockImplementation(async (request) => (
      access(request.agentId, request.expectedAccessRevision + 1)
    ));
  });

  it("ignores an earlier access response after switching agents", async () => {
    const first = agent("participant:first");
    const second = agent("participant:second");
    const pending = deferred<ChatAgentAccessRead>();
    api.readChatAgentAccess.mockReturnValueOnce(pending.promise);
    const { editor } = setup([first, second]);
    editor.selectedId = first.participant.id;
    const firstLoad = editor.initializeSelectedAgent(first);
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedAgent(second);
    pending.resolve(access(first.participant.id, 99));
    await firstLoad;

    expect(editor.displayName).toBe(second.participant.displayName);
    expect(editor.loadingAccess).toBe(false);
    expect(editor.dirty).toBe(false);
    editor.instructions = "A new instruction";
    await editor.save();
    expect(api.replaceChatAgentAccess).toHaveBeenCalledWith(expect.objectContaining({
      agentId: second.participant.id, expectedAccessRevision: 1,
    }));
  });

  it("does not let an earlier load failure replace the current editor's error", async () => {
    const first = agent("participant:first");
    const second = agent("participant:second");
    const pending = deferred<ChatAgentAccessRead>();
    api.readChatAgentAccess.mockReturnValueOnce(pending.promise);
    const { editor } = setup([first, second]);
    editor.selectedId = first.participant.id;
    const firstLoad = editor.initializeSelectedAgent(first);
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedAgent(second);
    pending.reject(new Error("Old selection failed"));
    await firstLoad;
    expect(editor.error).toBeNull();
    expect(editor.loadingAccess).toBe(false);
  });

  it("preserves a draft and uses current revisions after conflict rebasing", async () => {
    const { editor, focusConflict, focusRecoveryNotice } = setup();
    editor.selectedId = agent().participant.id;
    await editor.initializeSelectedAgent(agent());
    editor.instructions = "Local instructions";
    const durable = { ...agent("participant:atlas", 4), role: "Lead engineer" };
    api.replaceChatAgentAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatAgent.mockResolvedValue(durable);
    api.readChatAgentAccess.mockResolvedValue(access(durable.participant.id, 7));
    await editor.save();

    expect(editor.instructions).toBe("Local instructions");
    expect(editor.accessConflict?.localDraft.profile.instructions).toBe("Local instructions");
    expect(editor.canSave).toBe(false);
    expect(focusConflict).toHaveBeenCalledOnce();
    editor.rebaseAccessConflict();
    expect(editor.role).toBe("Lead engineer");
    expect(editor.instructions).toBe("Local instructions");
    expect(editor.conflictRecoveryNotice).toBe("rebased");
    expect(focusRecoveryNotice).toHaveBeenCalledOnce();
    await editor.save();
    expect(api.replaceChatAgentAccess).toHaveBeenLastCalledWith(expect.objectContaining({
      expectedAccessRevision: 7,
      agentProfile: expect.objectContaining({ expectedRevision: 4, role: "Lead engineer" }),
    }));
  });

  it("requires a fresh confirmation when the draft changes after preview", async () => {
    const { editor } = setup();
    editor.selectedId = agent().participant.id;
    await editor.initializeSelectedAgent(agent());
    editor.accessDraft = [{
      ...editor.defaultChannelAccess("channel:general"),
      capabilities: { readHistory: true, participate: true },
    }];
    await editor.save();
    expect(editor.accessConfirmationImpact?.historyChannelIds).toEqual(["channel:general"]);
    expect(api.replaceChatAgentAccess).not.toHaveBeenCalled();
    editor.instructions = "Changed after preview";
    await editor.save(true);
    expect(editor.accessConfirmationImpact).toBeNull();
    expect(api.replaceChatAgentAccess).not.toHaveBeenCalled();
    await editor.save();
    await editor.save(true);
    expect(api.replaceChatAgentAccess).toHaveBeenCalledOnce();
    expect(api.replaceChatAgentAccess).toHaveBeenCalledWith(expect.objectContaining({
      agentProfile: expect.objectContaining({ instructions: "Changed after preview" }),
    }));
  });

  it("discards an obsolete conflict response after selecting another agent", async () => {
    const first = agent();
    const second = agent("participant:second");
    const { editor, focusConflict } = setup([first, second]);
    editor.selectedId = first.participant.id;
    await editor.initializeSelectedAgent(first);
    editor.instructions = "Local first-agent edit";
    const pending = deferred<ChatAiAgentRead>();
    api.replaceChatAgentAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatAgent.mockReturnValueOnce(pending.promise);
    const saving = editor.save();
    await vi.waitFor(() => expect(editor.conflictLoading).toBe(true));
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedAgent(second);
    pending.resolve(agent(first.participant.id, 5));
    await saving;
    expect(editor.accessConflict).toBeNull();
    expect(editor.conflictLoading).toBe(false);
    expect(editor.displayName).toBe(second.participant.displayName);
    expect(focusConflict).not.toHaveBeenCalled();
  });

  it("reloads durable conflict state without the refresh resetting the adopted draft", async () => {
    const { editor } = setup();
    editor.selectedId = agent().participant.id;
    await editor.initializeSelectedAgent(agent());
    editor.instructions = "Local instructions";
    const durable = { ...agent("participant:atlas", 4), role: "Lead engineer" };
    api.replaceChatAgentAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatAgent.mockResolvedValue(durable);
    api.readChatAgentAccess.mockResolvedValue(access(durable.participant.id, 7));
    await editor.save();
    await editor.reloadCurrentAccessConflict();
    const readCount = api.readChatAgentAccess.mock.calls.length;
    await editor.initializeSelectedAgent(durable);
    expect(api.readChatAgentAccess).toHaveBeenCalledTimes(readCount);
    expect(editor.role).toBe("Lead engineer");
    expect(editor.instructions).toBe(durable.instructions);
    expect(editor.conflictRecoveryNotice).toBe("reloaded");
    expect(editor.dirty).toBe(false);
  });

  it("retains edits made while saving instead of treating them as persisted", async () => {
    const { editor } = setup();
    editor.selectedId = agent().participant.id;
    await editor.initializeSelectedAgent(agent());
    editor.instructions = "Submitted instructions";
    const pending = deferred<ChatAgentAccessRead>();
    api.replaceChatAgentAccess.mockReturnValueOnce(pending.promise);
    const saving = editor.save();
    editor.instructions = "Edited while saving";
    await editor.save();
    expect(api.replaceChatAgentAccess).toHaveBeenCalledOnce();
    pending.resolve(access(agent().participant.id, 2));
    await saving;
    await editor.initializeSelectedAgent(agent());
    expect(editor.instructions).toBe("Edited while saving");
    expect(editor.dirty).toBe(true);
    expect(editor.saving).toBe(false);
  });

  it("retries an access replacement failure without creating another agent", async () => {
    const created = agent("participant:new");
    const { editor, chat } = setup([]);
    api.createChatAgent.mockResolvedValue(created);
    chat.refreshAgents = vi.fn(async () => { chat.agents = [created]; });
    api.replaceChatAgentAccess.mockRejectedValueOnce(new Error("Replacement failed"));
    editor.beginCreate("channel:general");
    editor.displayName = created.participant.displayName;
    editor.role = created.role;
    editor.selectExecution({
      providerInstanceId: created.latestPolicy!.providerInstanceId,
      modelId: created.latestPolicy!.modelId,
      providerManaged: false,
      options: [],
    });
    await editor.save();

    expect(editor.creating).toBe(false);
    expect(editor.selectedId).toBe(created.participant.id);
    expect(editor.error).toBe("settings.agents.agents.saveFailed");
    expect(editor.accessDraft.map((entry) => entry.channelId)).toEqual(["channel:general"]);
    expect(editor.dirty).toBe(true);
    await editor.save();
    expect(editor.accessConfirmationImpact?.historyChannelIds).toEqual(["channel:general"]);
    await editor.save(true);
    expect(api.createChatAgent).toHaveBeenCalledOnce();
    expect(api.replaceChatAgentAccess).toHaveBeenCalledTimes(2);
    expect(editor.error).toBeNull();
    expect(editor.dirty).toBe(false);
  });

  it("keeps successful directory results when one request fails", async () => {
    const { editor, chat } = setup();
    const archived = agent("participant:archived");
    archived.participant.archivedAt = CREATED_AT;
    api.listChatAgents.mockResolvedValue([archived]);
    api.listChatNavigationChannels.mockRejectedValue(new Error("Channels unavailable"));
    await editor.loadDirectoryData();
    expect(chat.archivedAgents).toEqual([archived]);
    expect(editor.error).toBe("settings.agents.agents.loadFailed");
    expect(editor.loadingDirectory).toBe(false);
  });

  it("rebuilds an untouched new agent's channel default when its provider changes", async () => {
    const authority: ProviderAuthoritySupport = {
      isolatedConversation: true, internalHostTools: true, denyShell: true, readOnlyRoot: true,
      writableRoot: true, confinedCommands: false, networkBoundary: true, classifiedPublish: false,
      personApprovedCommands: true,
    };
    const providers = [
      { configuration: { instanceId: "provider:claude", familyId: "claude", favoriteModelIds: [] }, lastProbe: { authoritySupport: authority } },
      {
        configuration: { instanceId: "provider:cursor", familyId: "cursor", favoriteModelIds: [] },
        lastProbe: { authoritySupport: { ...authority, networkBoundary: false, personApprovedCommands: false } },
      },
    ] as unknown as ProviderInstanceRead[];
    api.listChatNavigationChannels.mockResolvedValue([
      { id: "channel:general", projectId: "project:app" } as ChatChannelRead,
    ]);
    const workingFolders = [{
      workingFolder: { id: "folder:app", projectId: "project:app", kind: "managed", archivedAt: null },
      bindingStatus: "available",
    }] as unknown as ProjectWorkingFolderRead[];
    const { editor } = setup([], { settings: { providerInstances: providers, configuration: { rememberedSelections: [] } } as unknown as ChatSettingsRead, workingFolders });
    await editor.loadDirectoryData();
    const choose = (providerInstanceId: string) => editor.selectExecution({
      providerInstanceId, modelId: null, providerManaged: true, options: [],
    });
    editor.beginCreate("channel:general");
    choose("provider:claude");
    expect(editor.accessDraft[0].folderGrants).toEqual([
      { workingFolderId: "folder:app", capability: "execute", isDefault: true, runtimeApprovalOverride: "ask" },
    ]);
    choose("provider:cursor");
    expect(editor.accessDraft[0]).toMatchObject({ capabilities: { readHistory: true, participate: true }, folderGrants: [] });
    choose("provider:claude");
    editor.accessDraft = editor.accessDraft.map((channel) => ({ ...channel, folderGrants: [] }));
    choose("provider:cursor");
    choose("provider:claude");
    expect(editor.accessDraft[0].folderGrants).toEqual([]);
  });
});
