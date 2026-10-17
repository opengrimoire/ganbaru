import { describe, expect, it } from "vitest";
import type { ChatTeammateChannelAccessInput } from "$lib/chat/contracts";
import {
  applyAccessProfileToScope,
  applyChannelPresetToScope,
  capabilitiesForPreset,
  channelCapabilityPreset,
  folderCapabilityFits,
  resolveRuntimeApproval,
  selectionState,
  teammateAccessConfirmationImpact,
  teammateAccessDraftErrors,
  teammateAccessDraftSnapshot,
  teammateAccessNeedsConfirmation,
  toggleSelectionGroup,
} from "./access";

function access(channelId: string): ChatTeammateChannelAccessInput {
  return {
    channelId,
    accessProfileId: "profile:build",
    accessProfileRevision: 1,
    capabilities: { readHistory: true, participate: true },
    historyBoundary: { kind: "entire" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: [],
  };
}

describe("teammate access", () => {
  it("maps independent channel capabilities to presets", () => {
    expect(channelCapabilityPreset(capabilitiesForPreset("contextSource"))).toBe("contextSource");
    expect(channelCapabilityPreset(capabilitiesForPreset("isolatedResponder"))).toBe("isolatedResponder");
    expect(channelCapabilityPreset(capabilitiesForPreset("collaborator"))).toBe("collaborator");
    expect(channelCapabilityPreset({ readHistory: false, participate: false })).toBe("custom");
  });

  it("keeps folder capabilities inside the profile ceiling", () => {
    expect(folderCapabilityFits("read", "execute")).toBe(true);
    expect(folderCapabilityFits("publish", "execute")).toBe(false);
  });

  it("resolves the most local runtime approval", () => {
    expect(resolveRuntimeApproval("ask", null, null)).toEqual({ policy: "ask", source: "teammate" });
    expect(resolveRuntimeApproval("ask", "autoApprove", null)).toEqual({
      policy: "autoApprove", source: "channel",
    });
    expect(resolveRuntimeApproval("ask", "autoApprove", "unattended")).toEqual({
      policy: "unattended", source: "folder",
    });
  });

  it("bulk-selects only the exact provided channel set", () => {
    const selected = new Set(["channel:elsewhere"]);
    const next = toggleSelectionGroup(["channel:a", "channel:b"], selected, true);
    expect([...next].sort()).toEqual(["channel:a", "channel:b", "channel:elsewhere"]);
    expect(selectionState(["channel:a", "channel:b"], next)).toBe("all");
    expect(selectionState(["channel:a", "channel:c"], next)).toBe("some");
  });

  it("applies one behavior to an exact selected scope", () => {
    const outside = access("channel:outside");
    const next = applyChannelPresetToScope(
      [access("channel:a"), access("channel:b"), outside],
      new Set(["channel:a", "channel:b"]),
      "isolatedResponder",
    );

    expect(next.slice(0, 2).map((entry) => channelCapabilityPreset(entry.capabilities)))
      .toEqual(["isolatedResponder", "isolatedResponder"]);
    expect(next[2]).toBe(outside);
  });

  it("applies a profile to a scope and removes grants above its ceiling", () => {
    const inside = access("channel:a");
    inside.folderGrants = [
      { workingFolderId: "folder:read", capability: "read", isDefault: false, runtimeApprovalOverride: null },
      { workingFolderId: "folder:build", capability: "execute", isDefault: true, runtimeApprovalOverride: null },
    ];
    const outside = access("channel:outside");
    const next = applyAccessProfileToScope(
      [inside, outside],
      new Set(["channel:a"]),
      { id: "profile:read", revision: 4, maximumFolderCapability: "read" },
    );

    expect(next[0]?.accessProfileId).toBe("profile:read");
    expect(next[0]?.accessProfileRevision).toBe(4);
    expect(next[0]?.folderGrants.map((grant) => grant.workingFolderId)).toEqual(["folder:read"]);
    expect(next[1]).toBe(outside);
  });

  it("creates order-independent atomic snapshots", () => {
    expect(teammateAccessDraftSnapshot([access("channel:b"), access("channel:a")]))
      .toBe(teammateAccessDraftSnapshot([access("channel:a"), access("channel:b")]));
  });

  it("does not require another confirmation for ordinary responder membership", () => {
    const responder = access("channel:a");
    responder.capabilities = capabilitiesForPreset("isolatedResponder");

    const impact = teammateAccessConfirmationImpact([], [responder]);

    expect(teammateAccessNeedsConfirmation(impact)).toBe(false);
  });

  it("identifies history, executable folder, publishing, and removal consequences", () => {
    const retained = access("channel:retained");
    retained.historyBoundary = { kind: "fromGrant" };
    retained.folderGrants = [
      { workingFolderId: "folder:build", capability: "read", isDefault: false, runtimeApprovalOverride: null },
      { workingFolderId: "folder:publish", capability: "execute", isDefault: true, runtimeApprovalOverride: null },
    ];
    const removed = access("channel:removed");
    const nextRetained = access("channel:retained");
    nextRetained.folderGrants = [
      { workingFolderId: "folder:build", capability: "execute", isDefault: true, runtimeApprovalOverride: null },
      { workingFolderId: "folder:publish", capability: "publish", isDefault: false, runtimeApprovalOverride: null },
    ];
    const boundedHistory = access("channel:bounded");
    boundedHistory.historyBoundary = { kind: "fromGrant" };

    const impact = teammateAccessConfirmationImpact(
      [retained, removed],
      [nextRetained, boundedHistory],
    );

    expect(impact).toEqual({
      historyChannelIds: ["channel:bounded"],
      entireHistoryChannelIds: ["channel:retained"],
      executeFolderIds: ["folder:build"],
      publishFolderIds: ["folder:publish"],
      removedChannelIds: ["channel:removed"],
    });
    expect(teammateAccessNeedsConfirmation(impact)).toBe(true);
  });

  it("keeps scratch approval overrides in atomic drafts", () => {
    const first = access("channel:a");
    const second = access("channel:a");
    second.scratchRuntimeApprovalOverride = "unattended";

    expect(teammateAccessDraftSnapshot([first]))
      .not.toBe(teammateAccessDraftSnapshot([second]));
  });

  it("keeps the reviewed profile revision in atomic drafts", () => {
    const first = access("channel:a");
    const second = access("channel:a");
    second.accessProfileRevision = 2;

    expect(teammateAccessDraftSnapshot([first]))
      .not.toBe(teammateAccessDraftSnapshot([second]));
  });

  it("rejects duplicated channels and grants above the profile ceiling", () => {
    const first = access("channel:a");
    first.folderGrants = [{
      workingFolderId: "folder:a",
      capability: "publish",
      isDefault: true,
      runtimeApprovalOverride: null,
    }];
    const errors = teammateAccessDraftErrors(
      [first, access("channel:a")],
      new Map([["profile:build", "execute"]]),
    );
    expect(errors).toContain("Duplicate channel channel:a");
    expect(errors).toContain("Folder folder:a exceeds its access profile");
  });

  it("allows executable access to fall back to private scratch", () => {
    const draft = access("channel:a");
    draft.folderGrants = [{
      workingFolderId: "folder:a",
      capability: "execute",
      isDefault: false,
      runtimeApprovalOverride: null,
    }];
    expect(teammateAccessDraftErrors(
      [draft],
      new Map([["profile:build", "execute"]]),
    )).toEqual([]);
  });
});
