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

/** Icon a layer added from a file starts with until the listener picks another. */
export const NEW_LAYER_ICON = "lucide:file-music";

/** Stroke soundscape icons use at their reference size, matching the app's compact icon stroke. */
const ICON_STROKE_WIDTH = 1.5;
/** Largest icon size that keeps the full stroke; larger tile icons thin out so their lines render as thin as list icons. */
const ICON_STROKE_REFERENCE_SIZE = 16;

/** Return the nominal stroke width for a soundscape icon so large tile icons do not look heavier than small ones. */
export function soundscapeIconStrokeWidth(size: number): number {
  if (!Number.isFinite(size) || size <= ICON_STROKE_REFERENCE_SIZE) return ICON_STROKE_WIDTH;
  return ICON_STROKE_WIDTH * ICON_STROKE_REFERENCE_SIZE / size;
}
