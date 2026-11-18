import { describe, expect, it } from "vitest";
import type { ChatAgentChannelAccessInput } from "$lib/chat/contracts";
import {
  applyChannelPresetToScope,
  capabilitiesForPreset,
  channelCapabilityPreset,
  folderAccessToggles,
  folderCapabilityIncludes,
  folderCapabilityWithToggle,
  impliedFolderGrantId,
  resolveRuntimeApproval,
  selectionState,
  agentAccessConfirmationImpact,
  agentAccessDraftErrors,
  agentAccessDraftSnapshot,
  agentAccessNeedsConfirmation,
  toggleSelectionGroup,
} from "./access";

function access(channelId: string): ChatAgentChannelAccessInput {
  return {
    channelId,
    capabilities: { readHistory: true, participate: true },
    historyBoundary: { kind: "entire" },
    runtimeApprovalOverride: null,
    scratchRuntimeApprovalOverride: null,
    folderGrants: [],
  };
}

describe("agent access", () => {
  it("maps independent channel capabilities to presets", () => {
    expect(channelCapabilityPreset(capabilitiesForPreset("contextSource"))).toBe("contextSource");
    expect(channelCapabilityPreset(capabilitiesForPreset("isolatedResponder"))).toBe("isolatedResponder");
    expect(channelCapabilityPreset(capabilitiesForPreset("collaborator"))).toBe("collaborator");
    expect(channelCapabilityPreset({ readHistory: false, participate: false })).toBe("custom");
  });

  it("orders folder capabilities from none to publish", () => {
    expect(folderCapabilityIncludes("execute", "read")).toBe(true);
    expect(folderCapabilityIncludes("execute", "execute")).toBe(true);
    expect(folderCapabilityIncludes("execute", "publish")).toBe(false);
    expect(folderCapabilityIncludes("none", "read")).toBe(false);
  });

  it("shows every level up to the granted capability as on", () => {
    expect(folderAccessToggles("none")).toEqual({ read: false, edit: false, execute: false, publish: false });
    expect(folderAccessToggles("edit")).toEqual({ read: true, edit: true, execute: false, publish: false });
    expect(folderAccessToggles("publish")).toEqual({ read: true, edit: true, execute: true, publish: true });
  });

  it("turns on every lower level when a level is turned on", () => {
    expect(folderCapabilityWithToggle("none", "execute", true)).toBe("execute");
    expect(folderCapabilityWithToggle("read", "publish", true)).toBe("publish");
    expect(folderCapabilityWithToggle("none", "read", true)).toBe("read");
  });

  it("keeps a higher capability when an already included level is turned on", () => {
    expect(folderCapabilityWithToggle("publish", "read", true)).toBe("publish");
    expect(folderCapabilityWithToggle("execute", "execute", true)).toBe("execute");
  });

  it("turns off every higher level when a level is turned off", () => {
    expect(folderCapabilityWithToggle("publish", "edit", false)).toBe("read");
    expect(folderCapabilityWithToggle("execute", "execute", false)).toBe("edit");
    expect(folderCapabilityWithToggle("publish", "publish", false)).toBe("execute");
  });

  it("maps every level off to no file access", () => {
    expect(folderCapabilityWithToggle("publish", "read", false)).toBe("none");
    expect(folderCapabilityWithToggle("read", "read", false)).toBe("none");
  });

  it("leaves a lower capability unchanged when a level above it is turned off", () => {
    expect(folderCapabilityWithToggle("read", "execute", false)).toBe("read");
    expect(folderCapabilityWithToggle("none", "publish", false)).toBe("none");
  });

  it("resolves the most local runtime approval", () => {
    expect(resolveRuntimeApproval("ask", null, null)).toEqual({ policy: "ask", source: "agent" });
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

  it("creates order-independent atomic snapshots", () => {
    expect(agentAccessDraftSnapshot([access("channel:b"), access("channel:a")]))
      .toBe(agentAccessDraftSnapshot([access("channel:a"), access("channel:b")]));
  });

  it("does not require another confirmation for ordinary responder membership", () => {
    const responder = access("channel:a");
    responder.capabilities = capabilitiesForPreset("isolatedResponder");

    const impact = agentAccessConfirmationImpact([], [responder]);

    expect(agentAccessNeedsConfirmation(impact)).toBe(false);
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

    const impact = agentAccessConfirmationImpact(
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
    expect(agentAccessNeedsConfirmation(impact)).toBe(true);
  });

  it("keeps scratch approval overrides in atomic drafts", () => {
    const first = access("channel:a");
    const second = access("channel:a");
    second.scratchRuntimeApprovalOverride = "unattended";

    expect(agentAccessDraftSnapshot([first]))
      .not.toBe(agentAccessDraftSnapshot([second]));
  });

  it("rejects duplicated channels and duplicated default folders", () => {
    const first = access("channel:a");
    first.folderGrants = [
      { workingFolderId: "folder:a", capability: "publish", isDefault: true, runtimeApprovalOverride: null },
      { workingFolderId: "folder:b", capability: "read", isDefault: true, runtimeApprovalOverride: null },
    ];
    const errors = agentAccessDraftErrors([first, access("channel:a")]);
    expect(errors).toContain("Duplicate channel channel:a");
    expect(errors).toContain("Channel channel:a has multiple default folders");
  });

  it("allows executable access to fall back to private scratch", () => {
    const draft = access("channel:a");
    draft.folderGrants = [{
      workingFolderId: "folder:a",
      capability: "execute",
      isDefault: false,
      runtimeApprovalOverride: null,
    }];
    expect(agentAccessDraftErrors([draft])).toEqual([]);
  });
});

describe("impliedFolderGrantId", () => {
  function grant(workingFolderId: string, isDefault = false, capability: "execute" | "none" = "execute") {
    return { workingFolderId, capability, isDefault };
  }

  it("prefers the default grant over the project folder", () => {
    expect(impliedFolderGrantId([grant("project"), grant("docs", true)], "project")).toBe("docs");
  });

  it("uses the granted project folder without a default", () => {
    expect(impliedFolderGrantId([grant("docs"), grant("project")], "project")).toBe("project");
  });

  it("falls back to the only grant when the project folder is not granted", () => {
    expect(impliedFolderGrantId([grant("docs")], "project")).toBe("docs");
    expect(impliedFolderGrantId([grant("docs"), grant("site", false, "none")], null)).toBe("docs");
  });

  it("implies nothing for several grants without a default or project grant", () => {
    expect(impliedFolderGrantId([grant("docs"), grant("site")], "project")).toBeNull();
    expect(impliedFolderGrantId([grant("site", true, "none")], null)).toBeNull();
    expect(impliedFolderGrantId([], "project")).toBeNull();
  });
});
