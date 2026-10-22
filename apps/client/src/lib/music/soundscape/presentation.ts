import type { MusicGeneratedNoiseKind, MusicSoundscapeDefinition } from "./contracts";

/** Present the gentlest rain-like texture first without changing stored identities. */
export const RAIN_NOISE_ORDER: readonly MusicGeneratedNoiseKind[] = ["brown", "pink", "white"];

/** Return generated sounds in their deliberate low-to-high intensity order. */
export function orderedGeneratedSounds(definitions: readonly MusicSoundscapeDefinition[]): MusicSoundscapeDefinition[] {
  return definitions
    .filter((entry) => entry.sourceKind === "generated-noise")
    .sort((left, right) => RAIN_NOISE_ORDER.indexOf(left.generatedKind ?? "brown") - RAIN_NOISE_ORDER.indexOf(right.generatedKind ?? "brown"));
}

/**
 * Compact option for soundscapes and layers: icon at the left and only as wide as its name, in the surrounding text
 * size. Pair it with `bg-primary/10` while the option is playing or picked.
 */
export const SOUNDSCAPE_CHIP_CLASS = "inline-flex h-8 min-w-0 max-w-full items-center gap-2 rounded-floating-item px-2 transition-colors hover:bg-accent/70 disabled:opacity-50";

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
