import { describe, expect, it } from "vitest";
import type {
  ChatAiAgentRead,
  ChatAgentAccessRead,
  ChatAgentChannelAccess,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
} from "$lib/chat/contracts";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  defaultChannelAccessInput,
  resolveChannelMemberChanges,
  agentCommandSupport,
  type ChannelAccessDefaults,
  type AgentCommandSupport,
} from "./channel-membership";

/** Authority a Claude probe reports: commands reach the person instead of being confined. */
const claudeAuthority: ProviderAuthoritySupport = {
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

const claude: AgentCommandSupport = { authority: claudeAuthority, safetyMode: "ask_for_approval" };

function defaults(overrides: Partial<ChannelAccessDefaults> = {}): ChannelAccessDefaults {
  return { projectWorkingFolderId: "folder:project", ...overrides };
}

function channelAccess(
  channelId: string,
  overrides: Partial<ChatAgentChannelAccess> = {},
): ChatAgentChannelAccess {
  return {
    channelId,
    conversationId: `conversation:${channelId}`,
    projectId: "project:1",
    groupId: "group:1",
    channelName: channelId,
    capabilities: { readHistory: true, participate: true },
    historyBoundary: { kind: "fromGrant", lowerOrderKey: "c00C" },
    runtimeApprovalOverride: "autoApprove",
    scratchRuntimeApprovalOverride: null,
    folderGrants: [
      {
        workingFolderId: "folder:active",
        displayName: "active",
        capability: "edit",
        isDefault: true,
        runtimeApprovalOverride: null,
        revision: 1,
        revokedAt: null,
      },
      {
        workingFolderId: "folder:revoked",
        displayName: "revoked",
        capability: "read",
        isDefault: false,
        runtimeApprovalOverride: null,
        revision: 1,
        revokedAt: "2026-10-02T00:00:00.000Z",
      },
    ],
    membershipRevision: 1,
    removedAt: null,
    ...overrides,
  };
}

function access(channels: ChatAgentChannelAccess[]): ChatAgentAccessRead {
  return {
    agentId: "participant:bot",
    accessRevision: 7,
    agentDefaultRuntimeApproval: "ask",
    channels,
  };
}

describe("channel membership", () => {
  it("reads history, takes assignments, and grants the project folder at Execute with Ask by default", () => {
    const input = defaultChannelAccessInput("channel:a", defaults(), claude);
    expect(input).toEqual({
      channelId: "channel:a",
      capabilities: { readHistory: true, participate: true },
      historyBoundary: { kind: "fromGrant" },
      runtimeApprovalOverride: null,
      scratchRuntimeApprovalOverride: null,
      folderGrants: [
        { workingFolderId: "folder:project", capability: "execute", isDefault: true, runtimeApprovalOverride: "ask" },
      ],
    });
  });

  it("joins without file access when the folder or provider enforcement is missing", () => {
    const withoutFiles = {
      capabilities: { readHistory: true, participate: true },
      folderGrants: [],
    };
    expect(defaultChannelAccessInput("channel:a", defaults({ projectWorkingFolderId: null }), claude))
      .toMatchObject(withoutFiles);
    expect(defaultChannelAccessInput("channel:a", defaults(), { authority: null, safetyMode: "ask_for_approval" }))
      .toMatchObject(withoutFiles);
    expect(defaultChannelAccessInput("channel:a", defaults(), {
      authority: { ...claudeAuthority, networkBoundary: false },
      safetyMode: "ask_for_approval",
    })).toMatchObject(withoutFiles);
    expect(defaultChannelAccessInput("channel:a", defaults(), { authority: claudeAuthority, safetyMode: "custom" }))
      .toMatchObject(withoutFiles);
  });

  it("reads command support from the agent policy and its provider probe", () => {
    const provider = {
      configuration: { instanceId: "provider:claude" },
      lastProbe: { authoritySupport: claudeAuthority },
    } as unknown as ProviderInstanceRead;
    const agent = {
      latestPolicy: { providerInstanceId: "provider:claude", safetyMode: "approve_for_me" },
    } as unknown as ChatAiAgentRead;
    expect(agentCommandSupport(agent, [provider])).toEqual({
      authority: claudeAuthority,
      safetyMode: "approve_for_me",
    });
    expect(agentCommandSupport(agent, [])).toEqual({ authority: null, safetyMode: "approve_for_me" });
    expect(agentCommandSupport(null, [provider])).toEqual({ authority: null, safetyMode: null });
  });

  it("removes one channel while preserving the other channels' grants and dropping revoked folders", () => {
    const current = access([channelAccess("channel:a"), channelAccess("channel:b")]);
    const next = channelAccessWithoutChannel(current, "channel:a");
    expect(next.map((entry) => entry.channelId)).toEqual(["channel:b"]);
    expect(next[0]).toMatchObject({
      capabilities: { readHistory: true, participate: true },
      historyBoundary: { kind: "fromGrant" },
      runtimeApprovalOverride: "autoApprove",
    });
    expect(next[0].folderGrants).toEqual([
      { workingFolderId: "folder:active", capability: "edit", isDefault: true, runtimeApprovalOverride: null },
    ]);
  });

  it("ignores channels the agent already left", () => {
    const current = access([
      channelAccess("channel:a", { removedAt: "2026-10-03T00:00:00.000Z" }),
      channelAccess("channel:b"),
    ]);
    expect(channelAccessWithoutChannel(current, "channel:b")).toEqual([]);
  });

  it("adds a channel at the default access without duplicating an existing membership", () => {
    const current = access([channelAccess("channel:a")]);
    const added = channelAccessWithChannel(current, "channel:b", defaults(), claude);
    expect(added.map((entry) => entry.channelId)).toEqual(["channel:a", "channel:b"]);
    expect(added[1].capabilities).toEqual({ readHistory: true, participate: true });
    expect(added[1].folderGrants).toEqual([
      { workingFolderId: "folder:project", capability: "execute", isDefault: true, runtimeApprovalOverride: "ask" },
    ]);
    expect(channelAccessWithChannel(current, "channel:a", defaults(), claude)).toHaveLength(1);
  });

  it("cancels out an add followed by a remove when resolving member changes", () => {
    const current = new Set(["participant:a", "participant:b"]);
    const selected = new Set(["participant:b", "participant:c"]);
    expect(resolveChannelMemberChanges(current, selected)).toEqual({
      addedAgentIds: ["participant:c"],
      removedAgentIds: ["participant:a"],
    });
    expect(resolveChannelMemberChanges(current, current)).toEqual({
      addedAgentIds: [],
      removedAgentIds: [],
    });
  });
});
