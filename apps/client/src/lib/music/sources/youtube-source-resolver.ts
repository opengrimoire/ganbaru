import {
  getYouTubeHostUrl,
  getYouTubeMetadata,
  type MusicYouTubeMetadataResponse,
} from "$lib/api/music";
import { buildYouTubeHostUrl, parseYouTubeHostMessage } from "$lib/stores/music-player/youtube-host";
import type { YouTubePlaylistSource, YouTubeVideoSource } from "$lib/music/sources";

export type MusicYouTubeResolutionSource = YouTubeVideoSource | YouTubePlaylistSource;

export interface MusicYouTubePreviewVideo {
  videoId: string;
  title: string;
  channel: string;
  durationMs: number | null;
  metadataResolved: boolean;
}

export interface MusicYouTubeSourcePreview {
  kind: "youtube-video" | "youtube-playlist";
  videoId: string | null;
  playlistId: string | null;
  title: string;
  channel: string;
  durationMs: number | null;
  videoIds: string[];
  videos: MusicYouTubePreviewVideo[];
  metadataTruncated: boolean;
  duplicateCount: number;
}

export interface MusicYouTubeResolverDependencies {
  hostUrl(): Promise<string>;
  createFrame(): HTMLIFrameElement;
  mountFrame(frame: HTMLIFrameElement): void;
  removeFrame(frame: HTMLIFrameElement): void;
  addMessageListener(listener: (event: MessageEvent<unknown>) => void): void;
  removeMessageListener(listener: (event: MessageEvent<unknown>) => void): void;
  setTimer(callback: () => void, timeoutMs: number): number;
  clearTimer(id: number): void;
  token(): string;
  metadata(playlistId: string | null, videoIds: string[]): Promise<MusicYouTubeMetadataResponse>;
}

const defaultDependencies: MusicYouTubeResolverDependencies = {
  hostUrl: getYouTubeHostUrl,
  createFrame: () => document.createElement("iframe"),
  mountFrame: (frame) => document.body.append(frame),
  removeFrame: (frame) => frame.remove(),
  addMessageListener: (listener) => window.addEventListener("message", listener),
  removeMessageListener: (listener) => window.removeEventListener("message", listener),
  setTimer: (callback, timeoutMs) => window.setTimeout(callback, timeoutMs),
  clearTimer: (id) => window.clearTimeout(id),
  token: () => crypto.randomUUID(),
  metadata: getYouTubeMetadata,
};

function fallbackPlaylistVideos(videoIds: readonly string[]): MusicYouTubePreviewVideo[] {
  return videoIds.map((videoId) => ({
    videoId,
    title: videoId,
    channel: "",
    durationMs: null,
    metadataResolved: false,
  }));
}

async function enrichPlaylistPreview(
  source: YouTubePlaylistSource,
  videoIds: string[],
  dependencies: MusicYouTubeResolverDependencies,
): Promise<MusicYouTubeSourcePreview> {
  const fallbackVideos = fallbackPlaylistVideos(videoIds);
  try {
    const metadata = await dependencies.metadata(source.playlistId, videoIds);
    const byId = new Map(
      metadata.videos.flatMap((video) => video.videoId ? [[video.videoId, video] as const] : []),
    );
    return {
      kind: "youtube-playlist",
      videoId: source.videoId,
      playlistId: source.playlistId,
      title: metadata.playlist?.title.trim() || source.title,
      channel: metadata.playlist?.channel.trim() || "",
      durationMs: null,
      videoIds,
      videos: fallbackVideos.map((video) => {
        const resolved = byId.get(video.videoId);
        return resolved ? {
          ...video,
          title: resolved.title.trim() || video.title,
          channel: resolved.channel.trim(),
          metadataResolved: true,
        } : video;
      }),
      metadataTruncated: metadata.truncated,
      duplicateCount: 0,
    };
  } catch {
    return {
      kind: "youtube-playlist",
      videoId: source.videoId,
      playlistId: source.playlistId,
      title: source.title,
      channel: "",
      durationMs: null,
      videoIds,
      videos: fallbackVideos,
      metadataTruncated: false,
      duplicateCount: 0,
    };
  }
}

/** Resolves one supported YouTube source through the same trusted IFrame host as playback. */
export async function resolveMusicYouTubeSource(
  source: MusicYouTubeResolutionSource,
  signal: AbortSignal,
  timeoutMs = 18_000,
  overrides: Partial<MusicYouTubeResolverDependencies> = {},
): Promise<MusicYouTubeSourcePreview> {
  const dependencies = { ...defaultDependencies, ...overrides };
  const frame = dependencies.createFrame();
  const generation = Math.max(1, Date.now());
  const baseUrl = await dependencies.hostUrl();
  if (signal.aborted) throw new DOMException("Resolution cancelled", "AbortError");
  const url = buildYouTubeHostUrl({
    baseUrl,
    generation,
    source,
    positionMs: null,
    autoplay: false,
    volume: 0,
    rate: 1,
  });
  const token = url.searchParams.get("token") || dependencies.token();
  const load = String(generation);
  if (!url.searchParams.has("token")) url.searchParams.set("token", token);
  frame.src = url.toString();
  frame.title = "YouTube source resolver";
  frame.tabIndex = -1;
  frame.setAttribute("aria-hidden", "true");
  Object.assign(frame.style, {
    position: "fixed",
    left: "-2px",
    bottom: "-2px",
    width: "1px",
    height: "1px",
    opacity: "0",
    pointerEvents: "none",
  });

  return new Promise<MusicYouTubeSourcePreview>((resolve, reject) => {
    let settled = false;
    let resolvingMetadata = false;
    const finish = (result: MusicYouTubeSourcePreview | Error): void => {
      if (settled) return;
      settled = true;
      dependencies.clearTimer(timerId);
      dependencies.removeMessageListener(onMessage);
      signal.removeEventListener("abort", onAbort);
      dependencies.removeFrame(frame);
      if (result instanceof Error) reject(result);
      else resolve(result);
    };
    const onAbort = () => finish(new DOMException("Resolution cancelled", "AbortError"));
    const onMessage = (event: MessageEvent<unknown>): void => {
      if (event.source !== frame.contentWindow) return;
      const message = parseYouTubeHostMessage(event.data);
      if (!message || message.token !== token || message.load !== load) return;
      if (message.type === "ganbaru-ai-youtube-error") {
        const reason = message.code === 101 || message.code === 150
          ? "This video does not allow embedded playback."
          : "This video is unavailable or private.";
        finish(new Error(reason));
        return;
      }
      if (message.type === "ganbaru-ai-youtube-playlist-error") {
        finish(new Error("This playlist is unavailable, private, or empty."));
        return;
      }
      if (source.kind === "youtube-playlist" && message.type === "ganbaru-ai-youtube-playlist") {
        if (resolvingMetadata) return;
        resolvingMetadata = true;
        dependencies.clearTimer(timerId);
        void enrichPlaylistPreview(source, message.videoIds, dependencies).then((preview) => {
          if (signal.aborted) finish(new DOMException("Resolution cancelled", "AbortError"));
          else finish(preview);
        });
        return;
      }
      if (source.kind === "youtube-video" && message.type === "ganbaru-ai-youtube-state") {
        finish({
          kind: "youtube-video",
          videoId: source.videoId,
          playlistId: null,
          title: message.title?.trim() || source.title,
          channel: message.channel?.trim() || "",
          durationMs: message.durationMs,
          videoIds: [source.videoId],
          videos: [{
            videoId: source.videoId,
            title: message.title?.trim() || source.title,
            channel: message.channel?.trim() || "",
            durationMs: message.durationMs,
            metadataResolved: true,
          }],
          metadataTruncated: false,
          duplicateCount: 0,
        });
      }
    };
    const timerId = dependencies.setTimer(
      () => finish(new Error("YouTube did not respond in time.")),
      timeoutMs,
    );
    signal.addEventListener("abort", onAbort, { once: true });
    dependencies.addMessageListener(onMessage);
    dependencies.mountFrame(frame);
  });
}
