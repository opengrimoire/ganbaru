import type { MusicGeneratedNoiseKind, MusicSoundscapeDefinition } from "./contracts";

/** Present the gentlest rain-like texture first without changing stored identities. */
export const RAIN_NOISE_ORDER: readonly MusicGeneratedNoiseKind[] = ["brown", "pink", "white"];

/** Return generated sounds in their deliberate low-to-high intensity order. */
export function orderedGeneratedSounds(definitions: readonly MusicSoundscapeDefinition[]): MusicSoundscapeDefinition[] {
  return definitions
    .filter((entry) => entry.sourceKind === "generated-noise")
    .sort((left, right) => RAIN_NOISE_ORDER.indexOf(left.generatedKind ?? "brown") - RAIN_NOISE_ORDER.indexOf(right.generatedKind ?? "brown"));
}

/** Picker icon of each generated rain texture, matching the seeded one-sound presets. */
export const GENERATED_RAIN_ICONS: Readonly<Record<MusicGeneratedNoiseKind, string>> = {
  brown: "lucide:cloud-hail",
  pink: "lucide:cloud-rain",
  white: "lucide:cloud-rain-wind",
};
