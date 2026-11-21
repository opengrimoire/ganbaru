import { describe, expect, it } from "vitest";
import type {
  ChatAssignmentTargetRead,
  ChatFolderCapability,
  ChatRuntimeApprovalPolicy,
  ProviderAuthoritySupport,
} from "$lib/chat/contracts";
import { providerTargetAuthorityBlockers } from "./provider-authority";

const confining: ProviderAuthoritySupport = {
  isolatedConversation: true,
  internalHostTools: true,
  denyShell: true,
  readOnlyRoot: true,
  writableRoot: true,
  confinedCommands: true,
  networkBoundary: true,
  classifiedPublish: true,
  personApprovedCommands: false,
};

const approving: ProviderAuthoritySupport = {
  ...confining,
  confinedCommands: false,
  classifiedPublish: false,
  personApprovedCommands: true,
};

type Target = Pick<ChatAssignmentTargetRead, "kind" | "folderCapability" | "effectiveRuntimeApproval">;

function folder(capability: ChatFolderCapability, approval: ChatRuntimeApprovalPolicy = "ask"): Target {
  return { kind: "currentFolder", folderCapability: capability, effectiveRuntimeApproval: approval };
}

function scratch(approval: ChatRuntimeApprovalPolicy = "ask"): Target {
  return { kind: "scratch", folderCapability: null, effectiveRuntimeApproval: approval };
}

describe("providerTargetAuthorityBlockers", () => {
  it("accepts every target except publish when the provider confines commands itself", () => {
    for (const target of [folder("read"), folder("edit"), folder("execute", "autoApprove"), scratch("autoApprove")]) {
      expect(providerTargetAuthorityBlockers(confining, target)).toEqual([]);
    }
    expect(providerTargetAuthorityBlockers(confining, folder("publish"))).toEqual([]);
  });

  it("admits person-approved commands only under Ask", () => {
    for (const target of [folder("execute"), scratch()]) {
      expect(providerTargetAuthorityBlockers(approving, target)).toEqual([]);
      expect(providerTargetAuthorityBlockers(approving, { ...target, effectiveRuntimeApproval: "autoApprove" })).toEqual([
        "chat.organization.providerCommandsNeedAsk",
      ]);
    }
  });

  it("does not require Ask for targets that run no commands", () => {
    for (const target of [folder("read", "autoApprove"), folder("edit", "autoApprove")]) {
      expect(providerTargetAuthorityBlockers(approving, target)).toEqual([]);
    }
  });

  it("keeps publish refused when the provider cannot classify publication", () => {
    expect(providerTargetAuthorityBlockers(approving, folder("publish"))).toEqual([
      "chat.organization.providerCannotPublish",
    ]);
  });

  it("reports commands the provider can neither confine nor relay", () => {
    const neither = { ...approving, personApprovedCommands: false };
    expect(providerTargetAuthorityBlockers(neither, folder("execute"))).toEqual([
      "chat.organization.providerCannotConfineCommands",
    ]);
  });

  it("lists each missing boundary for the target", () => {
    const bare: ProviderAuthoritySupport = {
      ...approving,
      denyShell: false,
      readOnlyRoot: false,
      writableRoot: false,
      networkBoundary: false,
    };
    expect(providerTargetAuthorityBlockers(bare, folder("read"))).toEqual([
      "chat.organization.providerCannotReadRoot",
      "chat.organization.providerCannotDenyShell",
      "chat.organization.providerCannotEnforceNetwork",
    ]);
    expect(providerTargetAuthorityBlockers(bare, scratch())).toEqual([
      "chat.organization.providerCannotWriteRoot",
      "chat.organization.providerCannotEnforceNetwork",
    ]);
    expect(providerTargetAuthorityBlockers(bare, folder("none"))).toEqual([]);
  });
});
