import type { PlaybackSnapshot } from "$lib/music/playback";

/**
 * How long autoplay feedback waits for the YouTube host to report playback. The host reports
 * `ready` both before playback begins and when the browser blocks autoplay, so a start that
 * never leaves `ready` ends here and Play is offered again.
 */
export const YOUTUBE_START_TIMEOUT_MS = 15_000;

/** Whether an autoplay load of one source generation is still waiting for the host to play. */
export type YouTubeStartState =
  | { phase: "idle" }
  | { phase: "starting"; generation: number; deadlineMs: number };

export const YOUTUBE_START_IDLE: YouTubeStartState = { phase: "idle" };

/** Starts waiting for playback when a load autoplays; a paused load never waits. */
export function beginYouTubeStart(
  generation: number,
  autoplay: boolean,
  nowMs: number,
): YouTubeStartState {
  return autoplay
    ? { phase: "starting", generation, deadlineMs: nowMs + YOUTUBE_START_TIMEOUT_MS }
    : YOUTUBE_START_IDLE;
}

/**
 * Applies a host status report. Any status other than `ready` or `loading` settles the start,
 * because the host is then playing, paused, ended, or failed.
 */
export function youtubeStartAfterStatus(
  state: YouTubeStartState,
  status: PlaybackSnapshot["status"],
): YouTubeStartState {
  if (state.phase === "idle" || status === "ready" || status === "loading") return state;
  return YOUTUBE_START_IDLE;
}

/** Settles the start of `generation` once its deadline passes; other generations are kept. */
export function youtubeStartAfterDeadline(
  state: YouTubeStartState,
  generation: number,
  nowMs: number,
): YouTubeStartState {
  if (state.phase === "idle" || state.generation !== generation || nowMs < state.deadlineMs) return state;
  return YOUTUBE_START_IDLE;
}

/** Whether surfaces show autoplay feedback instead of an enabled Play control. */
export function isYouTubeStarting(state: YouTubeStartState): boolean {
  return state.phase === "starting";
}
