import type { ChatSettingsRead, ProviderFamilyMetadataRead, ProviderInstanceRead } from "$lib/chat/contracts";

/** Whether a provider family can take turns on this device, and if not, the next thing the person does about it. */
export type ProviderAvailability =
  /** A configuration runs and is signed in. */
  | { kind: "ready" }
  /** The CLI runs but its provider account is signed out. */
  | { kind: "signed-out" }
  /** A configuration names a program that is no longer there. */
  | { kind: "missing-at"; path: string }
  /** The CLI was found but no configuration uses it, such as after the person removed the automatic one. */
  | { kind: "installed"; path: string }
  /** The device searched for the CLI and found nothing. */
  | { kind: "not-installed" }
  /** A configuration exists that no probe has checked yet. */
  | { kind: "unchecked" }
  /** Every configuration of the family is turned off. */
  | { kind: "disabled" }
  /** Anything else Settings resolves: an unsupported version or a broken setup. */
  | { kind: "attention" };

/** One provider family with its availability on this device. */
export interface ProviderFamilyAvailability {
  family: ProviderFamilyMetadataRead;
  availability: ProviderAvailability;
  /** The configuration the availability describes, whose probe holds its version and account, if any. */
  instance: ProviderInstanceRead | null;
}

/**
 * The availability of one family from its configurations and the programs the settings read found for it, or `null`
 * when the family is not offered: it is not implemented, or it has no configuration and the device does not search for
 * its CLI, as with a generic family the person sets up by hand.
 */
export function providerAvailability(
  family: ProviderFamilyMetadataRead,
  instances: readonly ProviderInstanceRead[],
  installedExecutables: readonly string[] | undefined,
): Omit<ProviderFamilyAvailability, "family"> | null {
  if (family.implementationStatus !== "available") return null;
  if (instances.length === 0) {
    if (installedExecutables === undefined) return null;
    const found = installedExecutables[0];
    return { availability: found === undefined ? { kind: "not-installed" } : { kind: "installed", path: found }, instance: null };
  }
  const active = instances.filter((instance) => instance.configuration.enabled && instance.duplicateOf === null);
  if (active.length === 0) return { availability: { kind: "disabled" }, instance: instances[0] };
  const ready = active.find((instance) => instance.lastProbe?.state === "healthy" && !instance.lastProbe.agentMinimumVersion);
  if (ready) return { availability: { kind: "ready" }, instance: ready };
  const signedOut = active.find((instance) => instance.lastProbe?.state === "authentication_required");
  if (signedOut) return { availability: { kind: "signed-out" }, instance: signedOut };
  const unchecked = active.find((instance) => instance.lastProbe === null);
  if (unchecked) return { availability: { kind: "unchecked" }, instance: unchecked };
  const missing = active.find((instance) => instance.lastProbe?.state === "executable_missing");
  if (missing) return { availability: { kind: "missing-at", path: missing.configuration.executable }, instance: missing };
  return { availability: { kind: "attention" }, instance: active[0] };
}

/** The availability of every family the device offers, in the order of `families`. */
export function providerFamilyAvailabilities<Family extends ProviderFamilyMetadataRead>(
  families: readonly Family[],
  settings: Pick<ChatSettingsRead, "providerInstances" | "installedExecutables">,
): Array<ProviderFamilyAvailability & { family: Family }> {
  return families.flatMap((family) => {
    const instances = settings.providerInstances.filter((instance) => instance.configuration.familyId === family.familyId);
    const result = providerAvailability(family, instances, settings.installedExecutables[family.familyId]);
    return result === null ? [] : [{ family, ...result }];
  });
}
