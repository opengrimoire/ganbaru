import { describe, expect, it } from "vitest";
import type { ProviderFamilyMetadataRead, ProviderProbeResult } from "./contracts";
import { providerVersionNotice } from "./provider-version";

const family: ProviderFamilyMetadataRead = {
  familyId: "claude",
  displayName: "Anthropic",
  configurationSchemaVersion: 1,
  supportedPlatforms: ["linux"],
  minimumTestedCliVersion: "2.1.170",
  maximumTestedCliVersion: "2.1.322",
  updateCommand: "claude update",
  installGuide: null,
  defaultExecutableCandidates: ["claude"],
  implementationStatus: "available",
  maturity: "stable",
  protocolName: "claude-stream-json",
  potentialCapabilities: [],
  unavailableReason: null,
};

function probe(overrides: Partial<ProviderProbeResult> = {}): ProviderProbeResult {
  return {
    instanceId: "claude",
    state: "healthy",
    version: "2.1.320",
    negotiatedProtocolVersion: "stream-json",
    accountLabel: null,
    authSource: null,
    accountPlan: null,
    capabilities: { entries: [] },
    authoritySupport: {
      isolatedConversation: true,
      internalHostTools: true,
      denyShell: false,
      readOnlyRoot: false,
      writableRoot: false,
      confinedCommands: false,
      networkBoundary: false,
      classifiedPublish: false,
      personApprovedCommands: false,
    },
    checkedAt: "2026-11-16T12:00:00Z",
    detail: null,
    versionAdmission: "supported",
    agentMinimumVersion: null,
    ...overrides,
  };
}

describe("providerVersionNotice", () => {
  it("stays quiet for an unchecked provider or a version that runs everything", () => {
    expect(providerVersionNotice(null, family)).toBeNull();
    expect(providerVersionNotice(probe(), family)).toBeNull();
    expect(providerVersionNotice(probe({ versionAdmission: "untested_newer" }), family)).toBeNull();
  });

  it("asks for the agent minimum while direct threads still run", () => {
    expect(providerVersionNotice(probe({ agentMinimumVersion: "2.1.325" }), family)).toEqual({
      kind: "agents",
      minimumVersion: "2.1.325",
      updateCommand: "claude update",
    });
  });

  it("names the higher minimum when the version is too old for every run", () => {
    const tooOld = probe({ state: "unsupported_version", version: "2.1.100", versionAdmission: "below_minimum" });
    expect(providerVersionNotice(tooOld, family)).toEqual({
      kind: "too_old",
      minimumVersion: "2.1.170",
      updateCommand: "claude update",
    });
    expect(providerVersionNotice({ ...tooOld, agentMinimumVersion: "2.1.325" }, family)).toMatchObject({
      minimumVersion: "2.1.325",
    });
    expect(providerVersionNotice(tooOld, { ...family, minimumTestedCliVersion: null, updateCommand: null })).toEqual({
      kind: "too_old",
      minimumVersion: null,
      updateCommand: null,
    });
  });

  it("tells a version past the supported line apart from an old one", () => {
    const tooNew = probe({ state: "unsupported_version", version: "3.0.0", versionAdmission: "at_or_above_ceiling" });
    expect(providerVersionNotice(tooNew, family)).toEqual({ kind: "too_new" });
  });
});
