import { MAX_SOUNDSCAPE_LAYERS, type MusicSoundscapeDefinition } from "./contracts";

/** Whether two sound lists hold the same sounds, ignoring order. */
export function sameSoundSet(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((id) => right.includes(id));
}

/** Add or remove one sound from a preset draft, keeping order and never growing past the layer limit. */
export function togglePresetDraftSound(ids: readonly string[], id: string): string[] {
  if (ids.includes(id)) return ids.filter((entry) => entry !== id);
  if (ids.length >= MAX_SOUNDSCAPE_LAYERS) return [...ids];
  return [...ids, id];
}

/** Display names of a preset's sounds in preset order, skipping sounds that no longer exist. */
export function presetSoundNames(
  ids: readonly string[],
  definitions: readonly MusicSoundscapeDefinition[],
  nameOf: (definition: MusicSoundscapeDefinition) => string,
): string[] {
  return ids.flatMap((id) => {
    const definition = definitions.find((entry) => entry.id === id);
    return definition ? [nameOf(definition)] : [];
  });
}
