import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAccessProfileRead,
  ChatAiTeammateRead,
  ChatChannelRead,
  ChatFolderCapability,
  ChatSettingsRead,
  ProjectWorkingFolderRead,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
  SafetyMode,
} from "$lib/chat/contracts";
import { applyChannelMemberChanges, channelMemberChangeContext } from "./channel-member-changes";

const api = vi.hoisted(() => ({
  listChatAccessProfiles: vi.fn<typeof import("$lib/api/chat").listChatAccessProfiles>(),
  readChatTeammateAccess: vi.fn<typeof import("$lib/api/chat").readChatTeammateAccess>(),
  replaceChatTeammateAccess: vi.fn<typeof import("$lib/api/chat").replaceChatTeammateAccess>(),
}));

vi.mock("$lib/api/chat", () => api);

const CREATED_AT = "2026-11-01T00:00:00.000Z";

function profile(id: string, builtinKey: ChatAccessProfileRead["builtinKey"], ceiling: ChatFolderCapability): ChatAccessProfileRead {
  return {
    id,
    builtinKey,
    displayName: id,
    revision: 1,
    archivedAt: null,
    latestRevision: {
      id: `${id}:revision:1`,
      accessProfileId: id,
      revision: 1,
      defaultChannelCapabilities: { readHistory: false, participate: true },
      defaultHistoryBoundary: { kind: "entire" },
      maximumFolderCapability: ceiling,
      createdAt: CREATED_AT,
    },
  };
}

const personApproved: ProviderAuthoritySupport = {
  isolatedConversation: true,
  internalHostTools: true,
  denyShell: true,
  readOnlyRoot: true,
  writableRoot: true,
  confinedCommands: false,
  networkBoundary: true,
  classifiedPublish: false,
  personApprovedCommands: true,
};

function provider(instanceId: string, authoritySupport: ProviderAuthoritySupport): ProviderInstanceRead {
  return {
    configuration: { instanceId },
    lastProbe: { authoritySupport },
  } as unknown as ProviderInstanceRead;
}

function teammate(id: string, providerInstanceId: string, safetyMode: SafetyMode = "ask_for_approval"): ChatAiTeammateRead {
  return {
    participant: { id },
    latestPolicy: { providerInstanceId, safetyMode },
  } as unknown as ChatAiTeammateRead;
}

function folder(id: string, projectId: string): ProjectWorkingFolderRead {
  return {
    workingFolder: { id, projectId, kind: "managed", archivedAt: null },
    bindingStatus: "available",
  } as unknown as ProjectWorkingFolderRead;
}

const channel = { id: "channel:general", projectId: "project:app" } as ChatChannelRead;

function store(teammates: ChatAiTeammateRead[]) {
  return {
    teammates,
    workingFolders: [folder("folder:other", "project:other"), folder("folder:app", "project:app")],
    settings: {
      providerInstances: [
        provider("provider:claude", personApproved),
        provider("provider:cursor", { ...personApproved, networkBoundary: false, personApprovedCommands: false }),
      ],
    } as unknown as ChatSettingsRead,
  };
}

describe("applyChannelMemberChanges", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.listChatAccessProfiles.mockResolvedValue([
      profile("profile:conversation", "conversationOnly", "none"),
      profile("profile:build", "buildAndTest", "execute"),
    ]);
    api.readChatTeammateAccess.mockImplementation(async (teammateId) => ({
      teammateId,
      accessRevision: 4,
      teammateDefaultRuntimeApproval: "ask",
      channels: [],
    }));
  });

  it("grants a joining teammate the channel project folder at Execute with Ask", async () => {
    const chat = store([teammate("participant:claude", "provider:claude")]);
    const changed = await applyChannelMemberChanges(
      new Set(),
      new Set(["participant:claude"]),
      channelMemberChangeContext(chat, channel),
    );
    expect(changed).toBe(true);
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledWith({
      teammateId: "participant:claude",
      expectedAccessRevision: 4,
      channels: [expect.objectContaining({
        channelId: "channel:general",
        accessProfileId: "profile:build",
        folderGrants: [{
          workingFolderId: "folder:app",
          capability: "execute",
          isDefault: true,
          runtimeApprovalOverride: "ask",
        }],
      })],
    });
  });

  it("adds a teammate whose provider cannot enforce commands in conversation only", async () => {
    const chat = store([teammate("participant:cursor", "provider:cursor")]);
    await applyChannelMemberChanges(new Set(), new Set(["participant:cursor"]), channelMemberChangeContext(chat, channel));
    expect(api.replaceChatTeammateAccess).toHaveBeenCalledWith(expect.objectContaining({
      channels: [expect.objectContaining({ accessProfileId: "profile:conversation", folderGrants: [] })],
    }));
  });

  it("writes nothing when the member set is unchanged", async () => {
    const chat = store([teammate("participant:claude", "provider:claude")]);
    const members = new Set(["participant:claude"]);
    expect(await applyChannelMemberChanges(members, members, channelMemberChangeContext(chat, channel))).toBe(false);
    expect(api.listChatAccessProfiles).not.toHaveBeenCalled();
    expect(api.replaceChatTeammateAccess).not.toHaveBeenCalled();
  });
});
