import type { PlaybackStatus } from "$lib/music/playback";
import type { YouTubePlaylistSource, YouTubeVideoSource } from "$lib/music/sources";
import { isPlaybackStatus } from "./settings";

export type YouTubeSource = YouTubeVideoSource | YouTubePlaylistSource;
export type YouTubeCommandAction = "play" | "pause" | "stop" | "seek" | "volume" | "rate";

export interface ResolvingYouTubePlaylist {
  playlistId: string;
  preferredVideoId: string | null;
  startMs: number | null;
  endMs: number | null;
  autoplay: boolean;
  generation: number;
}

interface YouTubeHostStateMessage {
  token: string;
  load: string;
  type: "ganbaru-ai-youtube-state";
  status: PlaybackStatus;
  positionMs: number;
  durationMs: number | null;
  videoId: string | null;
  title: string | null;
  channel: string | null;
}

interface YouTubeHostReadyMessage {
  token: string;
  load: string;
  type: "ganbaru-ai-youtube-ready";
}

interface YouTubeHostErrorMessage {
  token: string;
  load: string;
  type: "ganbaru-ai-youtube-error";
  code: number;
}

export interface YouTubeHostPlaylistMessage {
  token: string;
  load: string;
  type: "ganbaru-ai-youtube-playlist";
  playlistId: string;
  videoIds: string[];
  index: number | null;
}

interface YouTubeHostPlaylistErrorMessage {
  token: string;
  load: string;
  type: "ganbaru-ai-youtube-playlist-error";
  playlistId: string;
}

export type YouTubeHostMessage =
  | YouTubeHostReadyMessage
  | YouTubeHostStateMessage
  | YouTubeHostErrorMessage
  | YouTubeHostPlaylistMessage
  | YouTubeHostPlaylistErrorMessage;

interface YouTubeHostUrlInput {
  baseUrl: string;
  generation: number;
  source: YouTubeSource;
  positionMs: number | null;
  autoplay: boolean;
  volume: number;
  rate: number;
}

export function buildYouTubeHostUrl(input: YouTubeHostUrlInput): URL {
  const url = new URL(input.baseUrl);
  url.searchParams.set("load", String(input.generation));
  url.searchParams.set("sourceKind", input.source.kind);
  if (input.source.kind === "youtube-video") {
    url.searchParams.set("videoId", input.source.videoId);
  } else {
    url.searchParams.set("playlistId", input.source.playlistId);
    if (input.source.videoId) {
      url.searchParams.set("videoId", input.source.videoId);
    }
  }
  if (input.source.startMs !== null) {
    url.searchParams.set("startMs", String(input.source.startMs));
  }
  if (input.source.endMs !== null) {
    url.searchParams.set("endMs", String(input.source.endMs));
  }
  url.searchParams.set(
    "resumeMs",
    String(input.positionMs ?? input.source.startMs ?? 0),
  );
  url.searchParams.set(
    "volume",
    String(input.source.kind === "youtube-playlist" ? 0 : input.volume),
  );
  url.searchParams.set("rate", String(input.rate));
  url.searchParams.set("autoplay", String(input.autoplay));
  return url;
}

export function parseYouTubeHostMessage(value: unknown): YouTubeHostMessage | null {
  if (typeof value !== "object" || value === null) return null;
  const record = value as Record<string, unknown>;
  if (
    typeof record.token !== "string"
    || typeof record.load !== "string"
    || typeof record.type !== "string"
  ) return null;
  if (record.type === "ganbaru-ai-youtube-ready") {
    return { token: record.token, load: record.load, type: "ganbaru-ai-youtube-ready" };
  }
  if (record.type === "ganbaru-ai-youtube-error" && typeof record.code === "number") {
    return {
      token: record.token,
      load: record.load,
      type: "ganbaru-ai-youtube-error",
      code: record.code,
    };
  }
  if (
    record.type === "ganbaru-ai-youtube-playlist"
    && typeof record.playlistId === "string"
    && Array.isArray(record.videoIds)
    && record.videoIds.every((id) => typeof id === "string" && id.length > 0)
    && (typeof record.index === "number" || record.index === null)
  ) {
    return {
      token: record.token,
      load: record.load,
      type: "ganbaru-ai-youtube-playlist",
      playlistId: record.playlistId,
      videoIds: record.videoIds,
      index: record.index,
    };
  }
  if (
    record.type === "ganbaru-ai-youtube-playlist-error"
    && typeof record.playlistId === "string"
  ) {
    return {
      token: record.token,
      load: record.load,
      type: "ganbaru-ai-youtube-playlist-error",
      playlistId: record.playlistId,
    };
  }
  if (
    record.type === "ganbaru-ai-youtube-state"
    && isPlaybackStatus(record.status)
    && typeof record.positionMs === "number"
    && (typeof record.durationMs === "number" || record.durationMs === null)
    && (typeof record.videoId === "string" || record.videoId === null)
    && (typeof record.title === "string" || record.title === null)
    && (typeof record.channel === "string" || record.channel === null)
  ) {
    return {
      token: record.token,
      load: record.load,
      type: "ganbaru-ai-youtube-state",
      status: record.status,
      positionMs: record.positionMs,
      durationMs: record.durationMs,
      videoId: record.videoId,
      title: record.title,
      channel: record.channel,
    };
  }
  return null;
}
