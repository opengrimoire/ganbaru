import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatAiAgentRead,
  ChatChannelRead,
  ChatSettingsRead,
  ProjectWorkingFolderRead,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
  SafetyMode,
} from "$lib/chat/contracts";
import { applyChannelMemberChanges, channelMemberChangeContext } from "./channel-member-changes";

const api = vi.hoisted(() => ({
  readChatAgentAccess: vi.fn<typeof import("$lib/api/chat").readChatAgentAccess>(),
  replaceChatAgentAccess: vi.fn<typeof import("$lib/api/chat").replaceChatAgentAccess>(),
}));

vi.mock("$lib/api/chat", () => api);

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

function agent(id: string, providerInstanceId: string, safetyMode: SafetyMode = "ask_for_approval"): ChatAiAgentRead {
  return {
    participant: { id },
    latestPolicy: { providerInstanceId, safetyMode },
  } as unknown as ChatAiAgentRead;
}

function folder(id: string, projectId: string): ProjectWorkingFolderRead {
  return {
    workingFolder: { id, projectId, kind: "managed", archivedAt: null },
    bindingStatus: "available",
  } as unknown as ProjectWorkingFolderRead;
}

const channel = { id: "channel:general", projectId: "project:app" } as ChatChannelRead;

function store(agents: ChatAiAgentRead[]) {
  return {
    agents,
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
    api.readChatAgentAccess.mockImplementation(async (agentId) => ({
      agentId,
      accessRevision: 4,
      agentDefaultRuntimeApproval: "ask",
      channels: [],
    }));
  });

  it("grants a joining agent the channel project folder at Execute with Ask", async () => {
    const chat = store([agent("participant:claude", "provider:claude")]);
    const changed = await applyChannelMemberChanges(
      new Set(),
      new Set(["participant:claude"]),
      channelMemberChangeContext(chat, channel),
    );
    expect(changed).toBe(true);
    expect(api.replaceChatAgentAccess).toHaveBeenCalledWith({
      agentId: "participant:claude",
      expectedAccessRevision: 4,
      channels: [expect.objectContaining({
        channelId: "channel:general",
        capabilities: { readHistory: true, participate: true },
        folderGrants: [{
          workingFolderId: "folder:app",
          capability: "execute",
          isDefault: true,
          runtimeApprovalOverride: "ask",
        }],
      })],
    });
  });

  it("adds an agent whose provider cannot enforce commands without file access", async () => {
    const chat = store([agent("participant:cursor", "provider:cursor")]);
    await applyChannelMemberChanges(new Set(), new Set(["participant:cursor"]), channelMemberChangeContext(chat, channel));
    expect(api.replaceChatAgentAccess).toHaveBeenCalledWith(expect.objectContaining({
      channels: [expect.objectContaining({ capabilities: { readHistory: true, participate: true }, folderGrants: [] })],
    }));
  });

  it("writes nothing when the member set is unchanged", async () => {
    const chat = store([agent("participant:claude", "provider:claude")]);
    const members = new Set(["participant:claude"]);
    expect(await applyChannelMemberChanges(members, members, channelMemberChangeContext(chat, channel))).toBe(false);
    expect(api.replaceChatAgentAccess).not.toHaveBeenCalled();
  });
});
