import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAccessProfileRead,
  ChatAiTeammateRead,
  ChatTeammateAccessRead,
} from "$lib/chat/contracts";
import {
  createTeammateEditorController,
  type TeammateEditorOptions,
} from "./editor-controller.svelte";

const api = vi.hoisted(() => ({
  listChatTeammates: vi.fn<typeof import("$lib/api/chat").listChatTeammates>(),
  listChatNavigationChannels: vi.fn<typeof import("$lib/api/chat").listChatNavigationChannels>(),
  listChatAccessProfiles: vi.fn<typeof import("$lib/api/chat").listChatAccessProfiles>(),
  readChatTeammate: vi.fn<typeof import("$lib/api/chat").readChatTeammate>(),
  readChatTeammateAccess: vi.fn<typeof import("$lib/api/chat").readChatTeammateAccess>(),
  createChatTeammate: vi.fn<typeof import("$lib/api/chat").createChatTeammate>(),
  previewChatTeammateAccess: vi.fn<typeof import("$lib/api/chat").previewChatTeammateAccess>(),
  replaceChatTeammateAccess: vi.fn<typeof import("$lib/api/chat").replaceChatTeammateAccess>(),
}));

vi.mock("$lib/api/chat", () => api);

const CREATED_AT = "2026-09-18T12:00:00Z";

function teammate(id = "participant:atlas", revision = 1): ChatAiTeammateRead {
  return {
    participant: {
      id, kind: "ai_teammate", displayName: id, avatar: { schemaVersion: 1, value: { kind: "initials" } },
      revision, archivedAt: null,
    },
    role: "Engineer",
    instructions: "Build carefully",
    configurationState: "healthy",
    latestPolicy: {
      id: "policy:atlas", teammateId: id, revision,
      providerInstanceId: "provider:codex", safetyMode: "ask_for_approval",
      providerManagedModel: false, modelId: "model:test", modelOptions: [],
      effort: null, speed: null, providerOptions: { schemaVersion: 1, value: {} },
      createdAt: CREATED_AT,
    },
    channelCount: 0,
    activeAssignmentCount: 0,
    hasDurableHistory: false,
  };
}

function access(teammateId = "participant:atlas", accessRevision = 1): ChatTeammateAccessRead {
  return { teammateId, accessRevision, teammateDefaultRuntimeApproval: "ask", channels: [] };
}

function profile(): ChatAccessProfileRead {
  return {
    id: "profile:conversation",
    builtinKey: "conversationOnly",
    displayName: "Conversation",
    revision: 1,
    archivedAt: null,
    latestRevision: {
      id: "profile-revision:conversation", accessProfileId: "profile:conversation", revision: 1,
      defaultChannelCapabilities: { readHistory: false, participate: true },
      defaultHistoryBoundary: { kind: "entire" },
      maximumFolderCapability: "none",
      createdAt: CREATED_AT,
    },
  };
}

function setup(teammates = [teammate()]) {
  const chat: TeammateEditorOptions["chat"] = {
    teammates,
    archivedTeammates: [],
    settings: null,
    refreshTeammates: vi.fn(async () => {}),
  };
  const focusConflict = vi.fn(async () => {});
  const focusRecoveryNotice = vi.fn();
  const editor = createTeammateEditorController({
    chat,
    t: (key: string, ..._args: unknown[]) => key,
    initialChannelId: () => null,
    closeAccessPicker: vi.fn(),
    setExpandedAccessChannel: vi.fn(),
    focusConflict,
    focusRecoveryNotice,
  });
  editor.accessProfiles = [profile()];
  return { editor, chat, focusConflict, focusRecoveryNotice };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

describe("teammate editor workflow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.readChatTeammateAccess.mockImplementation(async (id) => access(id));
    api.listChatTeammates.mockResolvedValue([]);
    api.listChatNavigationChannels.mockResolvedValue([]);
    api.listChatAccessProfiles.mockResolvedValue([profile()]);
    api.previewChatTeammateAccess.mockResolvedValue({
      proposed: null, isExpansion: true, addedChannelIds: [], removedChannelIds: [], issues: [],
    });
    api.replaceChatTeammateAccess.mockImplementation(async (request) => (
      access(request.teammateId, request.expectedAccessRevision + 1)
    ));
  });

  it("ignores an earlier access response after switching teammates", async () => {
    const first = teammate("participant:first");
    const second = teammate("participant:second");
    const pending = deferred<ChatTeammateAccessRead>();
    api.readChatTeammateAccess.mockReturnValueOnce(pending.promise);
    const { editor } = setup([first, second]);
    editor.selectedId = first.participant.id;
    const firstLoad = editor.initializeSelectedTeammate(first);
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedTeammate(second);
    pending.resolve({
      ...access(first.participant.id, 99),
      teammateDefaultRuntimeApproval: "unattended",
    });
    await firstLoad;

    expect(editor.displayName).toBe(second.participant.displayName);
    expect(editor.teammateDefaultRuntimeApproval).toBe("ask");
    expect(editor.loadingAccess).toBe(false);
    expect(editor.dirty).toBe(false);
    editor.instructions = "A new instruction";
    await editor.save();
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledWith(expect.objectContaining({
      teammateId: second.participant.id, expectedAccessRevision: 1,
    }));
  });

  it("does not let an earlier load failure replace the current editor's error", async () => {
    const first = teammate("participant:first");
    const second = teammate("participant:second");
    const pending = deferred<ChatTeammateAccessRead>();
    api.readChatTeammateAccess.mockReturnValueOnce(pending.promise);
    const { editor } = setup([first, second]);
    editor.selectedId = first.participant.id;
    const firstLoad = editor.initializeSelectedTeammate(first);
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedTeammate(second);
    pending.reject(new Error("Old selection failed"));
    await firstLoad;
    expect(editor.error).toBeNull();
    expect(editor.loadingAccess).toBe(false);
  });

  it("preserves a draft and uses current revisions after conflict rebasing", async () => {
    const { editor, focusConflict, focusRecoveryNotice } = setup();
    editor.selectedId = teammate().participant.id;
    await editor.initializeSelectedTeammate(teammate());
    editor.instructions = "Local instructions";
    const durable = { ...teammate("participant:atlas", 4), role: "Lead engineer" };
    api.replaceChatTeammateAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatTeammate.mockResolvedValue(durable);
    api.readChatTeammateAccess.mockResolvedValue(access(durable.participant.id, 7));
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
    expect(api.replaceChatTeammateAccess).toHaveBeenLastCalledWith(expect.objectContaining({
      expectedAccessRevision: 7,
      teammateProfile: expect.objectContaining({ expectedRevision: 4, role: "Lead engineer" }),
    }));
  });

  it("requires a fresh confirmation when the draft changes after preview", async () => {
    const { editor } = setup();
    editor.selectedId = teammate().participant.id;
    await editor.initializeSelectedTeammate(teammate());
    editor.accessDraft = [{
      ...editor.defaultChannelAccess("channel:general"),
      capabilities: { readHistory: true, participate: true },
    }];
    await editor.save();
    expect(editor.accessConfirmationImpact?.entireHistoryChannelIds).toEqual(["channel:general"]);
    expect(api.replaceChatTeammateAccess).not.toHaveBeenCalled();
    editor.instructions = "Changed after preview";
    await editor.save(true);
    expect(editor.accessConfirmationImpact).toBeNull();
    expect(api.replaceChatTeammateAccess).not.toHaveBeenCalled();
    await editor.save();
    await editor.save(true);
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledOnce();
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledWith(expect.objectContaining({
      teammateProfile: expect.objectContaining({ instructions: "Changed after preview" }),
    }));
  });

  it("discards an obsolete conflict response after selecting another teammate", async () => {
    const first = teammate();
    const second = teammate("participant:second");
    const { editor, focusConflict } = setup([first, second]);
    editor.selectedId = first.participant.id;
    await editor.initializeSelectedTeammate(first);
    editor.instructions = "Local first-teammate edit";
    const pending = deferred<ChatAiTeammateRead>();
    api.replaceChatTeammateAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatTeammate.mockReturnValueOnce(pending.promise);
    const saving = editor.save();
    await vi.waitFor(() => expect(editor.conflictLoading).toBe(true));
    editor.selectedId = second.participant.id;
    await editor.initializeSelectedTeammate(second);
    pending.resolve(teammate(first.participant.id, 5));
    await saving;
    expect(editor.accessConflict).toBeNull();
    expect(editor.conflictLoading).toBe(false);
    expect(editor.displayName).toBe(second.participant.displayName);
    expect(focusConflict).not.toHaveBeenCalled();
  });

  it("reloads durable conflict state without the refresh resetting the adopted draft", async () => {
    const { editor } = setup();
    editor.selectedId = teammate().participant.id;
    await editor.initializeSelectedTeammate(teammate());
    editor.instructions = "Local instructions";
    const durable = { ...teammate("participant:atlas", 4), role: "Lead engineer" };
    api.replaceChatTeammateAccess.mockRejectedValueOnce({ code: "stale_revision" });
    api.readChatTeammate.mockResolvedValue(durable);
    api.readChatTeammateAccess.mockResolvedValue(access(durable.participant.id, 7));
    await editor.save();
    await editor.reloadCurrentAccessConflict();
    const readCount = api.readChatTeammateAccess.mock.calls.length;
    await editor.initializeSelectedTeammate(durable);
    expect(api.readChatTeammateAccess).toHaveBeenCalledTimes(readCount);
    expect(editor.role).toBe("Lead engineer");
    expect(editor.instructions).toBe(durable.instructions);
    expect(editor.conflictRecoveryNotice).toBe("reloaded");
    expect(editor.dirty).toBe(false);
  });

  it("retains edits made while saving instead of treating them as persisted", async () => {
    const { editor } = setup();
    editor.selectedId = teammate().participant.id;
    await editor.initializeSelectedTeammate(teammate());
    editor.instructions = "Submitted instructions";
    const pending = deferred<ChatTeammateAccessRead>();
    api.replaceChatTeammateAccess.mockReturnValueOnce(pending.promise);
    const saving = editor.save();
    editor.instructions = "Edited while saving";
    await editor.save();
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledOnce();
    pending.resolve(access(teammate().participant.id, 2));
    await saving;
    await editor.initializeSelectedTeammate(teammate());
    expect(editor.instructions).toBe("Edited while saving");
    expect(editor.dirty).toBe(true);
    expect(editor.saving).toBe(false);
  });

  it("retries an access replacement failure without creating another teammate", async () => {
    const created = teammate("participant:new");
    const { editor, chat } = setup([]);
    api.createChatTeammate.mockResolvedValue(created);
    chat.refreshTeammates = vi.fn(async () => { chat.teammates = [created]; });
    api.replaceChatTeammateAccess.mockRejectedValueOnce(new Error("Replacement failed"));
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
    expect(editor.error).toBe("Replacement failed");
    expect(editor.accessDraft.map((entry) => entry.channelId)).toEqual(["channel:general"]);
    expect(editor.dirty).toBe(true);
    await editor.save();
    expect(api.createChatTeammate).toHaveBeenCalledOnce();
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledTimes(2);
    expect(editor.error).toBeNull();
    expect(editor.dirty).toBe(false);
  });

  it("keeps successful directory results when one request fails", async () => {
    const { editor, chat } = setup();
    const archived = teammate("participant:archived");
    archived.participant.archivedAt = CREATED_AT;
    api.listChatTeammates.mockResolvedValue([archived]);
    api.listChatNavigationChannels.mockRejectedValue(new Error("Channels unavailable"));
    await editor.loadDirectoryData();
    expect(chat.archivedTeammates).toEqual([archived]);
    expect(editor.accessProfiles).toEqual([profile()]);
    expect(editor.error).toBe("Channels unavailable");
    expect(editor.loadingDirectory).toBe(false);
  });
});
