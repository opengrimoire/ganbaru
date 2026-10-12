export type PlaybackStatus = "idle" | "loading" | "ready" | "playing" | "paused" | "ended" | "error";

export interface PlaybackSnapshot {
  status: PlaybackStatus;
  positionMs: number;
  durationMs: number | null;
  volume: number;
  rate: number;
  error: string | null;
}

export const DEFAULT_PLAYBACK_SNAPSHOT: PlaybackSnapshot = Object.freeze({
  status: "idle",
  positionMs: 0,
  durationMs: null,
  volume: 0.8,
  rate: 1,
  error: null,
});

export const MAX_VOLUME = 1;
export const MIN_RATE = 0.25;
export const MAX_RATE = 2;

export function stableStatusDuringYouTubeBuffering(
  current: PlaybackStatus,
  incoming: PlaybackStatus,
): PlaybackStatus {
  if (incoming === "loading" && current !== "loading") {
    return current;
  }
  return incoming;
}

export function formatPlaybackTime(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return "0:00";
  const totalSeconds = Math.floor(ms / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`;
  }
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export function clampVolume(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_PLAYBACK_SNAPSHOT.volume;
  return Math.min(MAX_VOLUME, Math.max(0, value));
}

export function clampRate(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_PLAYBACK_SNAPSHOT.rate;
  return Math.min(MAX_RATE, Math.max(MIN_RATE, value));
}

export function normalizeLocalPlayableStartMs(startMs: number | null): number {
  if (startMs === null || !Number.isFinite(startMs) || startMs <= 0) return 0;
  return Math.round(startMs);
}

export function localMediaSeekTargetMs(positionMs: number, playableStartMs: number): number {
  const position = Number.isFinite(positionMs) && positionMs > 0 ? Math.round(positionMs) : 0;
  return Math.max(position, normalizeLocalPlayableStartMs(playableStartMs));
}

export function formatVolumePercent(value: number): string {
  return `${Math.round(clampVolume(value) * 100)}%`;
}
