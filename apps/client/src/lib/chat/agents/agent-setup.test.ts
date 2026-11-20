import { describe, expect, it } from "vitest";
import type { ProviderInstanceRead } from "$lib/chat/contracts";
import { agentNeedsSetupHere, providerBecameReady, readyProviderInstances } from "./agent-setup";

function provider(
  instanceId: string,
  options: { enabled?: boolean; executable?: string; pendingReview?: boolean } = {},
): ProviderInstanceRead {
  return {
    configuration: {
      schemaVersion: 1,
      instanceId,
      familyId: instanceId,
      label: instanceId,
      enabled: options.enabled ?? true,
      executable: options.executable ?? instanceId,
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: options.pendingReview ? [{ label: "Executable", widensPermissions: true }] : [],
    lastProbe: null,
    lastSuccessfulProbeAt: null,
    modelCatalog: null,
    usageLimits: null,
    lastUsedAt: null,
    duplicateOf: null,
  };
}

describe("agentNeedsSetupHere", () => {
  it("asks for setup only where the device runs the work and the provider is unavailable", () => {
    expect(agentNeedsSetupHere("provider_unavailable", true)).toBe(true);
    expect(agentNeedsSetupHere("healthy", true)).toBe(false);
  });

  it("never asks for setup on a device that leaves the work to a linked desktop", () => {
    expect(agentNeedsSetupHere("provider_unavailable", false)).toBe(false);
    expect(agentNeedsSetupHere("healthy", false)).toBe(false);
  });
});

describe("readyProviderInstances", () => {
  it("keeps only enabled providers with an executable and nothing left to confirm", () => {
    const ready = readyProviderInstances([
      provider("claude"),
      provider("codex", { enabled: false }),
      provider("cursor", { executable: "  " }),
      provider("grok", { pendingReview: true }),
    ]);
    expect([...ready]).toEqual(["claude"]);
  });
});

describe("providerBecameReady", () => {
  it("reports a provider that turned ready since the previous settings", () => {
    expect(providerBecameReady(new Set(["claude"]), new Set(["claude", "codex"]))).toBe(true);
  });

  it("ignores unchanged settings and providers that stopped being ready", () => {
    expect(providerBecameReady(new Set(["claude"]), new Set(["claude"]))).toBe(false);
    expect(providerBecameReady(new Set(["claude", "codex"]), new Set(["codex"]))).toBe(false);
  });
});
