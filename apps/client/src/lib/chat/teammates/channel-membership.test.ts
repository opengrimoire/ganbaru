import { describe, expect, it } from "vitest";
import type {
  ChatAccessProfileRead,
  ChatTeammateAccessRead,
  ChatTeammateChannelAccess,
} from "$lib/chat/contracts";
import {
  channelAccessWithChannel,
  channelAccessWithoutChannel,
  conversationAccessProfile,
  defaultChannelAccessInput,
  resolveChannelMemberChanges,
} from "./channel-membership";

function profile(id: string, builtinKey: ChatAccessProfileRead["builtinKey"]): ChatAccessProfileRead {
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
      maximumFolderCapability: "none",
      createdAt: "2026-10-01T00:00:00.000Z",
    },
  };
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
    accessProfileRevision: 2,
    capabilities: { readHistory: true, participate: true },
    historyBoundary: { kind: "fromGrant", lowerOrdinal: 12 },
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
    const conversation = profile("profile:conversation", "conversationOnly");
    const build = profile("profile:build", "buildAndTest");
    expect(conversationAccessProfile([build, conversation])).toBe(conversation);
    expect(conversationAccessProfile([build])).toBe(build);
    expect(conversationAccessProfile([])).toBeNull();
  });

  it("builds the narrowest default channel access from a profile", () => {
    const input = defaultChannelAccessInput("channel:a", profile("profile:conversation", "conversationOnly"));
    expect(input).toEqual({
      channelId: "channel:a",
      accessProfileId: "profile:conversation",
      accessProfileRevision: 3,
      capabilities: { readHistory: false, participate: true },
      historyBoundary: { kind: "entire" },
      runtimeApprovalOverride: null,
      scratchRuntimeApprovalOverride: null,
      folderGrants: [],
    });
    expect(defaultChannelAccessInput("channel:a", null).accessProfileId).toBe("");
  });

  it("removes one channel while preserving the other channels' grants and dropping revoked folders", () => {
    const current = access([channelAccess("channel:a"), channelAccess("channel:b")]);
    const next = channelAccessWithoutChannel(current, "channel:a");
    expect(next.map((entry) => entry.channelId)).toEqual(["channel:b"]);
    expect(next[0]).toMatchObject({
      accessProfileId: "profile:build",
      accessProfileRevision: 2,
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
    const conversation = profile("profile:conversation", "conversationOnly");
    const added = channelAccessWithChannel(current, "channel:b", conversation);
    expect(added.map((entry) => entry.channelId)).toEqual(["channel:a", "channel:b"]);
    expect(added[1].capabilities).toEqual({ readHistory: false, participate: true });
    expect(channelAccessWithChannel(current, "channel:a", conversation)).toHaveLength(1);
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
