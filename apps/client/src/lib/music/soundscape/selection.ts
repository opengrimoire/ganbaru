import type { MusicSoundscapeStatus } from "./contracts";

export type SoundSelectionAction =
  | { kind: "pause" }
  | { kind: "resume" }
  | { kind: "stop" }
  | { kind: "start"; ids: string[] };

/** Resolve a sound click without conflating single selection with optional layering. */
export function soundSelectionAction(
  activeIds: readonly string[],
  selectedId: string,
  multipleEnabled: boolean,
  status: MusicSoundscapeStatus,
): SoundSelectionAction {
  if (multipleEnabled) {
    const next = activeIds.includes(selectedId)
      ? activeIds.filter((id) => id !== selectedId)
      : [...activeIds, selectedId];
    return next.length === 0 ? { kind: "stop" } : { kind: "start", ids: next };
  }
  if (activeIds[0] === selectedId && status === "playing") return { kind: "pause" };
  if (activeIds[0] === selectedId && status === "paused") return { kind: "resume" };
  return { kind: "start", ids: [selectedId] };
}
