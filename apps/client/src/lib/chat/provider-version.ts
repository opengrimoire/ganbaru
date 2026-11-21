import type { ProviderFamilyMetadataRead, ProviderProbeResult } from "./contracts";

/**
 * What a provider's CLI version asks of the person: an update so agents can run, an update
 * so anything can run, or a version newer than any this build supports.
 */
export type ProviderVersionNotice =
  | { kind: "agents"; minimumVersion: string; updateCommand: string | null }
  | { kind: "too_old"; minimumVersion: string | null; updateCommand: string | null }
  | { kind: "too_new" };

/**
 * The version notice a provider's last probe calls for, or `null` when its version runs
 * everything. A version too old for every run names the higher of the family and agent
 * minimums, so one update clears both.
 *
 * @param probe The provider's last probe, or `null` when it never ran.
 * @param family The provider family, which names the update command and family minimum.
 */
export function providerVersionNotice(
  probe: ProviderProbeResult | null,
  family: ProviderFamilyMetadataRead,
): ProviderVersionNotice | null {
  if (!probe) return null;
  if (probe.state === "unsupported_version") {
    if (probe.versionAdmission === "at_or_above_ceiling") return { kind: "too_new" };
    return {
      kind: "too_old",
      minimumVersion: probe.agentMinimumVersion ?? family.minimumTestedCliVersion,
      updateCommand: family.updateCommand,
    };
  }
  if (probe.agentMinimumVersion) {
    return { kind: "agents", minimumVersion: probe.agentMinimumVersion, updateCommand: family.updateCommand };
  }
  return null;
}
