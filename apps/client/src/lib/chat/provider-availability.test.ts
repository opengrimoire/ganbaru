import { describe, expect, it } from "vitest";
import type { ProbeState, ProviderFamilyMetadataRead, ProviderInstanceRead } from "$lib/chat/contracts";
import { providerAvailability, providerFamilyAvailabilities } from "./provider-availability";

function family(familyId: string, implementationStatus: ProviderFamilyMetadataRead["implementationStatus"] = "available"): ProviderFamilyMetadataRead {
  return {
    familyId,
    displayName: familyId,
    configurationSchemaVersion: 1,
    supportedPlatforms: [],
    minimumTestedCliVersion: null,
    maximumTestedCliVersion: null,
    updateCommand: null,
    installGuide: null,
    defaultExecutableCandidates: [familyId],
    implementationStatus,
    maturity: "stable",
    protocolName: familyId,
    potentialCapabilities: [],
    unavailableReason: null,
  };
}

function instance(
  familyId: string,
  options: {
    state?: ProbeState | null;
    enabled?: boolean;
    accountLabel?: string | null;
    agentMinimumVersion?: string | null;
    duplicateOf?: string | null;
    executable?: string;
  } = {},
): ProviderInstanceRead {
  const state = options.state === undefined ? "healthy" : options.state;
  return {
    configuration: {
      schemaVersion: 1,
      instanceId: `${familyId}-${state ?? "unchecked"}`,
      familyId,
      label: familyId,
      enabled: options.enabled ?? true,
      executable: options.executable ?? `/usr/bin/${familyId}`,
      providerHome: null,
      launchArguments: [],
      environment: {},
      credentialReferences: {},
      providerConfig: { schemaVersion: 1, value: {} },
    },
    runtimeReview: [],
    lastProbe: state === null ? null : {
      instanceId: familyId,
      state,
      version: null,
      negotiatedProtocolVersion: null,
      accountLabel: options.accountLabel ?? null,
      authSource: null,
      accountPlan: null,
      capabilities: { entries: [] },
      authoritySupport: {
        isolatedConversation: true,
        internalHostTools: true,
        denyShell: true,
        readOnlyRoot: true,
        writableRoot: true,
        confinedCommands: true,
        networkBoundary: true,
        classifiedPublish: true,
        personApprovedCommands: false,
      },
      checkedAt: "2026-11-19T11:00:00Z",
      detail: null,
      versionAdmission: null,
      agentMinimumVersion: options.agentMinimumVersion ?? null,
    },
    lastSuccessfulProbeAt: null,
    modelCatalog: null,
    usageLimits: null,
    lastUsedAt: null,
    duplicateOf: options.duplicateOf ?? null,
  };
}

describe("provider availability", () => {
  it("is ready with the configuration that runs when any does", () => {
    expect(providerAvailability(family("claude"), [
      instance("claude", { state: "authentication_required" }),
      instance("claude", { accountLabel: "person@example.com" }),
    ], ["/usr/bin/claude"])?.availability).toEqual({ kind: "ready" });
  });

  it("asks to sign in before reporting a missing program or another problem", () => {
    expect(providerAvailability(family("codex"), [
      instance("codex", { state: "executable_missing" }),
      instance("codex", { state: "authentication_required" }),
    ], [])?.availability).toEqual({ kind: "signed-out" });
    expect(providerAvailability(family("codex"), [
      instance("codex", { state: "executable_missing", executable: "/opt/codex" }),
      instance("codex", { state: "unsupported_version" }),
    ], [])?.availability).toEqual({ kind: "missing-at", path: "/opt/codex" });
  });

  it("does not count a healthy configuration whose agents need a newer CLI as ready", () => {
    expect(providerAvailability(family("cursor"), [instance("cursor", { agentMinimumVersion: "2.0.0" })], [])?.availability)
      .toEqual({ kind: "attention" });
  });

  it("reports a family whose configurations are all turned off or duplicates as turned off", () => {
    const off = instance("grok", { enabled: false });
    expect(providerAvailability(family("grok"), [
      off,
      instance("grok", { duplicateOf: "grok-original" }),
    ], [])).toEqual({ availability: { kind: "disabled" }, instance: off });
  });

  it("describes the configuration it judged so its version and account match its state", () => {
    const signedOut = instance("codex", { state: "authentication_required" });
    const ready = instance("codex", { accountLabel: "person@example.com" });
    expect(providerAvailability(family("codex"), [signedOut, ready], [])?.instance).toBe(ready);
    expect(providerAvailability(family("codex"), [signedOut, instance("codex", { state: "executable_missing" })], [])?.instance)
      .toBe(signedOut);
  });

  it("reports a configuration that no probe checked yet", () => {
    expect(providerAvailability(family("opencode"), [instance("opencode", { state: null })], [])?.availability)
      .toEqual({ kind: "unchecked" });
  });

  it("uses the programs found on the device when no configuration exists", () => {
    expect(providerAvailability(family("claude"), [], ["/home/me/.local/bin/claude", "/usr/bin/claude"]))
      .toEqual({ availability: { kind: "installed", path: "/home/me/.local/bin/claude" }, instance: null });
    expect(providerAvailability(family("claude"), [], [])).toEqual({ availability: { kind: "not-installed" }, instance: null });
  });

  it("leaves out families that are not implemented or that the device does not search for", () => {
    expect(providerAvailability(family("future", "metadata_only"), [], [])).toBeNull();
    expect(providerAvailability(family("acp"), [], undefined)).toBeNull();
    expect(providerAvailability(family("acp"), [instance("acp")], undefined)?.availability).toEqual({ kind: "ready" });
  });

  it("lists the offered families in the given order with their own configurations", () => {
    const families = [family("codex"), family("acp"), family("claude")];
    const result = providerFamilyAvailabilities(families, {
      providerInstances: [instance("claude", { state: "authentication_required" })],
      installedExecutables: { codex: [], claude: ["/usr/bin/claude"] },
    });
    expect(result.map(({ family, availability }) => [family.familyId, availability.kind])).toEqual([
      ["codex", "not-installed"],
      ["claude", "signed-out"],
    ]);
  });
});
