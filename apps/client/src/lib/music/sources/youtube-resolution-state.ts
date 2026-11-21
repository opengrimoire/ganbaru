import type { MusicYouTubeSourcePreview } from "./youtube-source-resolver";

/**
 * Where resolving a YouTube link stands. Only the running request's `controller` may settle it, so a superseded or
 * cancelled request never exposes its result.
 */
export type YouTubeResolutionState =
  | { phase: "idle" }
  | { phase: "resolving"; controller: AbortController }
  | { phase: "resolved"; preview: MusicYouTubeSourcePreview }
  | { phase: "failed"; error: string };

export type YouTubeResolutionEvent =
  | { type: "started"; controller: AbortController }
  | { type: "resolved"; controller: AbortController; preview: MusicYouTubeSourcePreview }
  | { type: "failed"; controller: AbortController; error: string }
  | { type: "settled"; controller: AbortController }
  | { type: "cancelled" };

export const IDLE_YOUTUBE_RESOLUTION: YouTubeResolutionState = { phase: "idle" };

/** Whether `controller` belongs to the request that is still running. */
export function isCurrentYouTubeResolution(state: YouTubeResolutionState, controller: AbortController): boolean {
  return state.phase === "resolving" && state.controller === controller;
}

/** Applies one resolution event; an event from a request that is no longer running returns the same state. */
export function transitionYouTubeResolution(
  state: YouTubeResolutionState,
  event: YouTubeResolutionEvent,
): YouTubeResolutionState {
  switch (event.type) {
    case "started":
      return { phase: "resolving", controller: event.controller };
    case "resolved":
      return isCurrentYouTubeResolution(state, event.controller) ? { phase: "resolved", preview: event.preview } : state;
    case "failed":
      return isCurrentYouTubeResolution(state, event.controller) ? { phase: "failed", error: event.error } : state;
    case "settled":
      return isCurrentYouTubeResolution(state, event.controller) ? IDLE_YOUTUBE_RESOLUTION : state;
    case "cancelled":
      return state.phase === "resolving" ? IDLE_YOUTUBE_RESOLUTION : state;
  }
}
