import type { ChatSettingsRead, ProviderFamilyMetadataRead, ProviderInstanceRead } from "$lib/chat/contracts";

/** Whether a provider family can take turns on this device, and if not, the next thing the person does about it. */
export type ProviderAvailability =
  /** A configuration runs and is signed in, with the account it reported when it has one. */
  | { kind: "ready"; accountLabel: string | null }
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
  /** Anything else Settings resolves: a turned off configuration, an unsupported version, or a broken setup. */
  | { kind: "attention" };

/** One provider family with its availability on this device. */
export interface ProviderFamilyAvailability {
  family: ProviderFamilyMetadataRead;
  availability: ProviderAvailability;
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
): ProviderAvailability | null {
  if (family.implementationStatus !== "available") return null;
  if (instances.length === 0) {
    if (installedExecutables === undefined) return null;
    const found = installedExecutables[0];
    return found === undefined ? { kind: "not-installed" } : { kind: "installed", path: found };
  }
  const active = instances.filter((instance) => instance.configuration.enabled && instance.duplicateOf === null);
  const ready = active.find((instance) => instance.lastProbe?.state === "healthy" && !instance.lastProbe.agentMinimumVersion);
  if (ready) return { kind: "ready", accountLabel: ready.lastProbe?.accountLabel ?? null };
  if (active.some((instance) => instance.lastProbe?.state === "authentication_required")) return { kind: "signed-out" };
  if (active.some((instance) => instance.lastProbe === null)) return { kind: "unchecked" };
  const missing = active.find((instance) => instance.lastProbe?.state === "executable_missing");
  if (missing) return { kind: "missing-at", path: missing.configuration.executable };
  return { kind: "attention" };
}

/** The availability of every family the device offers, in the order of `families`. */
export function providerFamilyAvailabilities(
  families: readonly ProviderFamilyMetadataRead[],
  settings: Pick<ChatSettingsRead, "providerInstances" | "installedExecutables">,
): ProviderFamilyAvailability[] {
  return families.flatMap((family) => {
    const instances = settings.providerInstances.filter((instance) => instance.configuration.familyId === family.familyId);
    const availability = providerAvailability(family, instances, settings.installedExecutables[family.familyId]);
    return availability === null ? [] : [{ family, availability }];
  });
}
