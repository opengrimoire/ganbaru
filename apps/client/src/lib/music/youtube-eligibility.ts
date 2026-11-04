import type { MusicItemAvailability, MusicLibrarySourceKind, MusicYouTubeResolutionState } from "$lib/music/library/contracts";

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

/** The fields of a library item that decide whether it can play here. */
export interface MusicPlayableItemFields {
  sourceKind: MusicLibrarySourceKind;
  availability: MusicItemAvailability;
  youtubeResolutionState: MusicYouTubeResolutionState | null;
}

/**
 * Whether an item counts as playable in playlist counts: a local file by this device's availability
 * observation, and a YouTube video by its replicated resolution state, matching the native queue policy.
 */
export function musicItemCountsAsPlayable(item: MusicPlayableItemFields): boolean {
  return item.sourceKind === "youtube-video" ? youtubeResolutionAllowsPlayback(item.youtubeResolutionState) : item.availability === "available";
}
