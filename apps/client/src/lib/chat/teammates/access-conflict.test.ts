import { describe, expect, it } from "vitest";
import type { ChatTeammateChannelAccessInput } from "$lib/chat/contracts";
import {
  compareChatTeammateStudioDrafts,
  rebaseChatTeammateStudioDraft,
  type ChatTeammateStudioDraft,
} from "./access-conflict";

function channel(
  channelId: string,
  participate = true,
): ChatTeammateChannelAccessInput {
  return {
    channelId,
    accessProfileId: "profile:conversation",
    accessProfileRevision: 1,
    capabilities: { readHistory: false, participate },
    historyBoundary: { kind: "entire" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: [],
  };
}

function draft(overrides: Partial<ChatTeammateStudioDraft> = {}): ChatTeammateStudioDraft {
  return {
    profile: {
      displayName: "Atlas",
      role: "Engineer",
      instructions: "Build carefully",
      providerId: "provider:codex",
      safetyMode: "ask_for_approval",
      modelId: "gpt-5",
      providerManagedModel: false,
      modelOptions: [],
      effort: "medium",
      speed: null,
      providerOptions: { schemaVersion: 1, value: {} },
    },
    teammateDefaultRuntimeApproval: "ask",
    channels: [channel("channel:general")],
    ...overrides,
  };
}

describe("teammate access conflict recovery", () => {
  it("summarizes identity, policy, approval, and channel differences", () => {
    const durable = draft({
      channels: [channel("channel:general"), channel("channel:remote")],
    });
    const local = draft({
      profile: {
        ...durable.profile,
        displayName: "Atlas local",
        modelId: "gpt-5.1",
      },
      teammateDefaultRuntimeApproval: "autoApprove",
      channels: [channel("channel:general", false), channel("channel:local")],
    });

    expect(compareChatTeammateStudioDrafts(local, durable)).toEqual({
      identityChanged: true,
      policyChanged: true,
      runtimeApprovalChanged: true,
      addedChannelCount: 1,
      removedChannelCount: 1,
      changedChannelCount: 1,
    });
  });

  it("replays local edits while retaining unrelated durable changes", () => {
    const baseline = draft();
    const local = draft({
      profile: { ...baseline.profile, modelId: "gpt-5.1" },
      channels: [channel("channel:local")],
    });
    const durable = draft({
      profile: {
        ...baseline.profile,
        role: "Lead engineer",
        providerOptions: { schemaVersion: 1, value: { durable: true } },
      },
      teammateDefaultRuntimeApproval: "unattended",
      channels: [channel("channel:general"), channel("channel:remote")],
    });

    const rebased = rebaseChatTeammateStudioDraft(baseline, local, durable);

    expect(rebased.profile.role).toBe("Lead engineer");
    expect(rebased.profile.modelId).toBe("gpt-5.1");
    expect(rebased.profile.providerOptions).toEqual({
      schemaVersion: 1,
      value: { durable: true },
    });
    expect(rebased.teammateDefaultRuntimeApproval).toBe("unattended");
    expect(rebased.channels.map((entry) => entry.channelId)).toEqual([
      "channel:remote",
      "channel:local",
    ]);
  });
});
