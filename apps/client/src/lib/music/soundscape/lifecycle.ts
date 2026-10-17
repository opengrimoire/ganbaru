import type { MusicSoundscapeStatus } from "./contracts";

export interface SoundscapeResumeObservation {
  desiredPlaying: boolean;
  status: MusicSoundscapeStatus;
  hiddenForMs: number;
}

/** Chooses output recovery only after a real interruption or a likely system suspend. */
export function shouldRecoverSoundscapeOutput(observation: SoundscapeResumeObservation): boolean {
  if (!observation.desiredPlaying) return false;
  return observation.status === "error" || observation.hiddenForMs >= 30_000;
}
