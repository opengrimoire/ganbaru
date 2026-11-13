import { describe, expect, it } from "vitest";
import type {
  ChatAccessProfileRead,
  ChatAiTeammateRead,
  ChatFolderCapability,
  ChatTeammateAccessRead,
  ChatTeammateChannelAccess,
  ProviderAuthoritySupport,
  ProviderInstanceRead,
} from "$lib/chat/contracts";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  conversationAccessProfile,
  defaultChannelAccessInput,
  resolveChannelMemberChanges,
  teammateCommandSupport,
  type ChannelAccessDefaults,
  type TeammateCommandSupport,
} from "./channel-membership";

function profile(
  id: string,
  builtinKey: ChatAccessProfileRead["builtinKey"],
  maximumFolderCapability: ChatFolderCapability = "none",
): ChatAccessProfileRead {
  return {
    id,
    builtinKey,
    displayName: id,
    revision: 3,
    archivedAt: null,
    latestRevision: {
      id: `${id}:revision:3`,
      accessProfileId: id,
      revision: 3,
      defaultChannelCapabilities: { readHistory: false, participate: true },
      defaultHistoryBoundary: { kind: "entire" },
      maximumFolderCapability,
      createdAt: "2026-10-01T00:00:00.000Z",
    },
  };
}

const conversation = profile("profile:conversation", "conversationOnly");
const build = profile("profile:build", "buildAndTest", "execute");

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

const claude: TeammateCommandSupport = { authority: claudeAuthority, safetyMode: "ask_for_approval" };

function defaults(overrides: Partial<ChannelAccessDefaults> = {}): ChannelAccessDefaults {
  return { profiles: [conversation, build], projectWorkingFolderId: "folder:project", ...overrides };
}

function channelAccess(
  channelId: string,
  overrides: Partial<ChatTeammateChannelAccess> = {},
): ChatTeammateChannelAccess {
  return {
    channelId,
    conversationId: `conversation:${channelId}`,
    projectId: "project:1",
    groupId: "group:1",
    channelName: channelId,
    accessProfileId: "profile:build",
    accessProfileRevisionId: "profile:build:revision:2",
    accessProfileRevision: 2,
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

function access(channels: ChatTeammateChannelAccess[]): ChatTeammateAccessRead {
  return {
    teammateId: "participant:bot",
    accessRevision: 7,
    teammateDefaultRuntimeApproval: "ask",
    channels,
  };
}

describe("channel membership", () => {
  it("prefers the conversation-only profile and falls back to the first profile", () => {
    expect(conversationAccessProfile([build, conversation])).toBe(conversation);
    expect(conversationAccessProfile([build])).toBe(build);
    expect(conversationAccessProfile([])).toBeNull();
  });

  it("grants the channel project folder at Execute with Ask when the provider keeps commands person-approved", () => {
    const input = defaultChannelAccessInput("channel:a", defaults(), claude);
    expect(input).toEqual({
      channelId: "channel:a",
      accessProfileId: "profile:build",
      accessProfileRevisionId: "profile:build:revision:3",
      capabilities: { readHistory: false, participate: true },
      historyBoundary: { kind: "entire" },
      runtimeApprovalOverride: null,
      scratchRuntimeApprovalOverride: null,
      folderGrants: [
        { workingFolderId: "folder:project", capability: "execute", isDefault: true, runtimeApprovalOverride: "ask" },
      ],
    });
  });

  it("falls back to conversation only when the folder, the Execute profile, or provider enforcement is missing", () => {
    const conversationOnly = {
      accessProfileId: "profile:conversation",
      accessProfileRevisionId: "profile:conversation:revision:3",
      folderGrants: [],
    };
    expect(defaultChannelAccessInput("channel:a", defaults({ projectWorkingFolderId: null }), claude))
      .toMatchObject(conversationOnly);
    expect(defaultChannelAccessInput("channel:a", defaults({ profiles: [conversation] }), claude))
      .toMatchObject(conversationOnly);
    expect(defaultChannelAccessInput("channel:a", defaults(), { authority: null, safetyMode: "ask_for_approval" }))
      .toMatchObject(conversationOnly);
    expect(defaultChannelAccessInput("channel:a", defaults(), {
      authority: { ...claudeAuthority, networkBoundary: false },
      safetyMode: "ask_for_approval",
    })).toMatchObject(conversationOnly);
    expect(defaultChannelAccessInput("channel:a", defaults(), { authority: claudeAuthority, safetyMode: "custom" }))
      .toMatchObject(conversationOnly);
    expect(defaultChannelAccessInput("channel:a", { profiles: [], projectWorkingFolderId: null }, claude).accessProfileId)
      .toBe("");
  });

  it("reads command support from the teammate policy and its provider probe", () => {
    const provider = {
      configuration: { instanceId: "provider:claude" },
      lastProbe: { authoritySupport: claudeAuthority },
    } as unknown as ProviderInstanceRead;
    const teammate = {
      latestPolicy: { providerInstanceId: "provider:claude", safetyMode: "approve_for_me" },
    } as unknown as ChatAiTeammateRead;
    expect(teammateCommandSupport(teammate, [provider])).toEqual({
      authority: claudeAuthority,
      safetyMode: "approve_for_me",
    });
    expect(teammateCommandSupport(teammate, [])).toEqual({ authority: null, safetyMode: "approve_for_me" });
    expect(teammateCommandSupport(null, [provider])).toEqual({ authority: null, safetyMode: null });
  });

  it("removes one channel while preserving the other channels' grants and dropping revoked folders", () => {
    const current = access([channelAccess("channel:a"), channelAccess("channel:b")]);
    const next = channelAccessWithoutChannel(current, "channel:a");
    expect(next.map((entry) => entry.channelId)).toEqual(["channel:b"]);
    expect(next[0]).toMatchObject({
      accessProfileId: "profile:build",
      accessProfileRevisionId: "profile:build:revision:2",
      historyBoundary: { kind: "fromGrant" },
      runtimeApprovalOverride: "autoApprove",
    });
    expect(next[0].folderGrants).toEqual([
      { workingFolderId: "folder:active", capability: "edit", isDefault: true, runtimeApprovalOverride: null },
    ]);
  });

  it("ignores channels the teammate already left", () => {
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
    expect(added[1].capabilities).toEqual({ readHistory: false, participate: true });
    expect(added[1].folderGrants).toEqual([
      { workingFolderId: "folder:project", capability: "execute", isDefault: true, runtimeApprovalOverride: "ask" },
    ]);
    expect(channelAccessWithChannel(current, "channel:a", defaults(), claude)).toHaveLength(1);
  });

  it("cancels out an add followed by a remove when resolving member changes", () => {
    const current = new Set(["participant:a", "participant:b"]);
    const selected = new Set(["participant:b", "participant:c"]);
    expect(resolveChannelMemberChanges(current, selected)).toEqual({
      addedTeammateIds: ["participant:c"],
      removedTeammateIds: ["participant:a"],
    });
    expect(resolveChannelMemberChanges(current, current)).toEqual({
      addedTeammateIds: [],
      removedTeammateIds: [],
    });
  });
});
