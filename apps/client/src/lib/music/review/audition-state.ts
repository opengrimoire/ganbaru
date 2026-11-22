import type { MusicReviewPlaybackCheckpoint } from "$lib/stores/music-player.svelte";

/**
 * Where a Review audition stands: closed, suspending the playback it interrupts, or holding that playback's
 * checkpoint while it plays review items. `itemId` is the item Review last loaded, `null` until one loads.
 */
export type MusicAuditionState =
  | { phase: "idle" }
  | { phase: "suspending" }
  | { phase: "auditioning"; checkpoint: MusicReviewPlaybackCheckpoint; itemId: string | null };

export type MusicAuditionEvent =
  | { type: "suspendStarted" }
  | { type: "suspended"; checkpoint: MusicReviewPlaybackCheckpoint }
  | { type: "itemLoaded"; itemId: string }
  | { type: "ended" };

export const IDLE_MUSIC_AUDITION: MusicAuditionState = { phase: "idle" };

/** Applies one audition event; an event that does not fit the current phase returns the same state. */
export function transitionMusicAudition(state: MusicAuditionState, event: MusicAuditionEvent): MusicAuditionState {
  switch (event.type) {
    case "suspendStarted":
      return state.phase === "idle" ? { phase: "suspending" } : state;
    case "suspended":
      return state.phase === "suspending"
        ? { phase: "auditioning", checkpoint: event.checkpoint, itemId: null }
        : state;
    case "itemLoaded":
      return state.phase === "auditioning" && state.itemId !== event.itemId
        ? { ...state, itemId: event.itemId }
        : state;
    case "ended":
      return state.phase === "idle" ? state : IDLE_MUSIC_AUDITION;
  }
}
