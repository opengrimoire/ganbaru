export type MusicSourceKind = "local-file" | "youtube-video" | "youtube-playlist";

export interface MusicSourceBase {
  kind: MusicSourceKind;
  originalInput: string;
  identity: string;
  title: string;
  /** Display artist from the library, when known; file tags apply otherwise. */
  artist?: string | null;
  /** Display album from the library, when known; file tags apply otherwise. */
  album?: string | null;
  startMs: number | null;
  endMs: number | null;
}

export interface LocalFileSource extends MusicSourceBase {
  kind: "local-file";
  path: string;
  artworkPath: string | null;
}

export interface YouTubeVideoSource extends MusicSourceBase {
  kind: "youtube-video";
  videoId: string;
  playlistId: string | null;
}

export interface YouTubePlaylistSource extends MusicSourceBase {
  kind: "youtube-playlist";
  playlistId: string;
  videoId: string | null;
}

export type MusicSource = LocalFileSource | YouTubeVideoSource | YouTubePlaylistSource;

export interface YouTubeVideoSourceOptions {
  playlistId?: string | null;
  playlistIndex?: number | null;
  startMs?: number | null;
  endMs?: number | null;
}

export interface SourceParseResult {
  source: MusicSource | null;
  error: string | null;
}

const YOUTUBE_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  "youtu.be",
  "www.youtu.be",
  "youtube-nocookie.com",
  "www.youtube-nocookie.com",
]);

const VIDEO_ID_PATTERN = /^[a-zA-Z0-9_-]{6,}$/;
const PLAYLIST_ID_PATTERN = /^[a-zA-Z0-9_-]{2,}$/;
const WINDOWS_ABSOLUTE_PATH_PATTERN = /^[a-zA-Z]:[\\/]/;
const URL_LIKE_PATTERN = /^[a-zA-Z][a-zA-Z\d+.-]*:/;

/** Extracts the video ID from a standalone library identity. */
export function youtubeVideoIdFromIdentity(identityKey: string): string | null {
  const prefix = "youtube:video:";
  if (!identityKey.startsWith(prefix)) return null;
  const videoId = identityKey.slice(prefix.length);
  return VIDEO_ID_PATTERN.test(videoId) ? videoId : null;
}

export function parseMusicSourceInput(input: string): SourceParseResult {
  const trimmed = input.trim();
  if (!trimmed) {
    return { source: null, error: "Enter a local file path or YouTube link." };
  }

  const url = parseUrl(trimmed);
  if (url) {
    if (isYouTubeUrl(url)) {
      return parseYouTubeSource(trimmed, url);
    }
    if (url.protocol === "file:") {
      const path = decodeFileUrl(url);
      return {
        source: makeLocalFileSource(trimmed, path),
        error: null,
      };
    }
    return { source: null, error: "Only local files and YouTube links are supported." };
  }

  if (looksLikeLocalPath(trimmed)) {
    return {
      source: makeLocalFileSource(trimmed, trimmed),
      error: null,
    };
  }

  return { source: null, error: "Enter an absolute local path or a YouTube link." };
}

export function formatSourceKind(kind: MusicSourceKind): string {
  switch (kind) {
    case "local-file":
      return "Local file";
    case "youtube-video":
      return "YouTube video";
    case "youtube-playlist":
      return "YouTube playlist";
  }
}

export function sourceDisplayLabel(source: MusicSource): string {
  if (source.title) return source.title;
  switch (source.kind) {
    case "local-file":
      return fileNameFromPath(source.path);
    case "youtube-video":
      return source.videoId;
    case "youtube-playlist":
      return source.playlistId;
  }
}

export function localFileSourceFromPath(path: string, title?: string, artworkPath?: string | null): LocalFileSource {
  return makeLocalFileSource(path, path, title, artworkPath);
}

export function youtubeVideoSourceFromId(
  videoId: string,
  options: YouTubeVideoSourceOptions = {},
): YouTubeVideoSource {
  const playlistId = options.playlistId ?? null;
  const playlistIndex = options.playlistIndex ?? null;
  return {
    kind: "youtube-video",
    originalInput: playlistId
      ? `https://www.youtube.com/watch?v=${encodeURIComponent(videoId)}&list=${encodeURIComponent(playlistId)}`
      : `https://www.youtube.com/watch?v=${encodeURIComponent(videoId)}`,
    identity: playlistId && playlistIndex !== null
      ? `youtube:playlist:${playlistId}:item:${playlistIndex}:video:${videoId}`
      : `youtube:video:${videoId}`,
    title: videoId,
    videoId,
    playlistId,
    startMs: options.startMs ?? null,
    endMs: options.endMs ?? null,
  };
}

export function isYouTubeSource(source: MusicSource): source is YouTubeVideoSource | YouTubePlaylistSource {
  return source.kind === "youtube-video" || source.kind === "youtube-playlist";
}

export function parseTimestampMs(value: string | null): number | null {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) return null;

  if (/^\d+(\.\d+)?$/.test(trimmed)) {
    return secondsToMs(Number(trimmed));
  }

  const colonValue = parseColonTimestamp(trimmed);
  if (colonValue !== null) return colonValue;

  const tokenPattern = /(\d+(?:\.\d+)?)(h|m|s)/g;
  let totalSeconds = 0;
  let consumed = "";
  for (const match of trimmed.matchAll(tokenPattern)) {
    const amount = Number(match[1]);
    const unit = match[2];
    consumed += match[0];
    if (unit === "h") totalSeconds += amount * 3600;
    if (unit === "m") totalSeconds += amount * 60;
    if (unit === "s") totalSeconds += amount;
  }
  if (consumed.length > 0 && consumed === trimmed.replace(/\s+/g, "")) {
    return secondsToMs(totalSeconds);
  }

  return null;
}

function parseYouTubeSource(originalInput: string, url: URL): SourceParseResult {
  const videoId = extractYouTubeVideoId(url);
  const playlistId = url.searchParams.get("list");
  const startMs = parseTimestampMs(
    url.searchParams.get("start")
      ?? url.searchParams.get("t")
      ?? url.searchParams.get("time_continue"),
  );
  const endMs = parseTimestampMs(url.searchParams.get("end") ?? url.searchParams.get("end_seconds"));

  if (playlistId && !PLAYLIST_ID_PATTERN.test(playlistId)) {
    return { source: null, error: "The YouTube playlist ID is not valid." };
  }
  if (videoId && !VIDEO_ID_PATTERN.test(videoId)) {
    return { source: null, error: "The YouTube video ID is not valid." };
  }

  if (playlistId) {
    return {
      source: {
        kind: "youtube-playlist",
        originalInput,
        identity: makeYouTubePlaylistIdentity(playlistId, videoId),
        title: playlistId,
        playlistId,
        videoId,
        startMs,
        endMs,
      },
      error: null,
    };
  }

  if (videoId) {
    return {
      source: {
        kind: "youtube-video",
        originalInput,
        identity: `youtube:video:${videoId}`,
        title: videoId,
        videoId,
        playlistId: null,
        startMs,
        endMs,
      },
      error: null,
    };
  }

  return { source: null, error: "The YouTube link does not include a video or playlist ID." };
}

function parseUrl(input: string): URL | null {
  try {
    return new URL(input);
  } catch {
    return null;
  }
}

function isYouTubeUrl(url: URL): boolean {
  return YOUTUBE_HOSTS.has(url.hostname.toLowerCase());
}

function extractYouTubeVideoId(url: URL): string | null {
  const host = url.hostname.toLowerCase();
  if (host === "youtu.be" || host === "www.youtu.be") {
    return firstPathSegment(url);
  }
  if (url.pathname === "/watch") {
    return url.searchParams.get("v");
  }
  const segments = url.pathname.split("/").filter(Boolean);
  if (segments[0] === "embed" || segments[0] === "shorts" || segments[0] === "live") {
    return segments[1] ?? null;
  }
  return null;
}

function firstPathSegment(url: URL): string | null {
  return url.pathname.split("/").filter(Boolean)[0] ?? null;
}

function makeYouTubePlaylistIdentity(playlistId: string, videoId: string | null): string {
  return videoId
    ? `youtube:playlist:${playlistId}:video:${videoId}`
    : `youtube:playlist:${playlistId}`;
}

function makeLocalFileSource(
  originalInput: string,
  path: string,
  title?: string,
  artworkPath?: string | null,
): LocalFileSource {
  const normalizedPath = normalizeLocalPath(path);
  return {
    kind: "local-file",
    originalInput,
    identity: `local:${normalizedPath}`,
    title: title?.trim() || fileNameFromPath(normalizedPath),
    path: normalizedPath,
    artworkPath: artworkPath ? normalizeLocalPath(artworkPath) : null,
    startMs: null,
    endMs: null,
  };
}

function normalizeLocalPath(path: string): string {
  return path.replace(/\\/g, "/");
}

function decodeFileUrl(url: URL): string {
  if (url.hostname && url.hostname !== "localhost") {
    return `//${url.hostname}${decodeURIComponent(url.pathname)}`;
  }
  return decodeURIComponent(url.pathname);
}

function looksLikeLocalPath(value: string): boolean {
  if (URL_LIKE_PATTERN.test(value)) return false;
  return value.startsWith("/") || WINDOWS_ABSOLUTE_PATH_PATTERN.test(value);
}

function fileNameFromPath(path: string): string {
  const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
  const dotIndex = name.lastIndexOf(".");
  return dotIndex > 0 ? name.slice(0, dotIndex) : name;
}

function parseColonTimestamp(value: string): number | null {
  const pieces = value.split(":");
  if (pieces.length < 2 || pieces.length > 3) return null;
  if (!pieces.every((piece) => /^\d+(\.\d+)?$/.test(piece))) return null;
  const numbers = pieces.map(Number);
  if (numbers.some((number) => !Number.isFinite(number))) return null;
  const seconds = pieces.length === 2
    ? numbers[0] * 60 + numbers[1]
    : numbers[0] * 3600 + numbers[1] * 60 + numbers[2];
  return secondsToMs(seconds);
}

function secondsToMs(seconds: number): number {
  return Math.max(0, Math.round(seconds * 1000));
}
