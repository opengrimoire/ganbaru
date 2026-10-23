import type { MusicYouTubeResolutionState } from "$lib/music/library/contracts";

/**
 * Whether the replicated resolution state allows attempting YouTube playback on this device.
 *
 * `embedding-blocked` and `unavailable` block. `ready` plays, and `resolving`, `timed-out`, or no state
 * attempt playback, so a transient state written by one device never blocks another and a video resolved
 * on one device plays on every online device without a refresh.
 */
export function youtubeResolutionAllowsPlayback(state: MusicYouTubeResolutionState | null): boolean {
  return state !== "embedding-blocked" && state !== "unavailable";
}
