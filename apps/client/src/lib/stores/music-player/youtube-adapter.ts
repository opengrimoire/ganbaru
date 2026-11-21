import { tick } from "svelte";
import { getYouTubeHostUrl } from "$lib/api/music";
import {
  applyMusicYouTubePlaylistSnapshot,
  reportMusicYouTubeSourceFailure,
  upsertMusicYouTubeVideo,
} from "$lib/api/music-library";
import type {
  MusicYouTubePlaylistSnapshotWrite,
  MusicYouTubeSourceFailureWrite,
  MusicYouTubeVideoWrite,
} from "$lib/music/library/contracts";
import {
  stableStatusDuringYouTubeBuffering,
  type PlaybackSnapshot,
} from "$lib/music/playback";
import {
  isYouTubeSource,
  youtubeVideoSourceFromId,
  type MusicSource,
} from "$lib/music/sources";
import { musicPlaybackErrorMessage, youtubePlaybackErrorCode, type MusicPlaybackErrorCode } from "$lib/music/playback-errors";
import type { NativeYouTubeHost } from "$lib/music/youtube-native-host";
import type { MusicLoadRuntime } from "./load-runtime";
import {
  buildYouTubeHostUrl,
  parseYouTubeHostMessage,
  type ResolvingYouTubePlaylist,
  type YouTubeCommandAction,
  type YouTubeHostPlaylistMessage,
  type YouTubeSource,
} from "./youtube-host";
import {
  YOUTUBE_START_IDLE,
  beginYouTubeStart,
  isYouTubeStarting,
  youtubeStartAfterDeadline,
  youtubeStartAfterStatus,
  type YouTubeStartState,
} from "./youtube-start-state";

const OPTIMISTIC_PAUSE_MS = 1_500;
const PLAYLIST_RESOLVE_TIMEOUT_MS = 18_000;

export interface MusicYouTubeState {
  currentSource: MusicSource | null;
  snapshot: PlaybackSnapshot;
  playerError: string | null;
  queue: MusicSource[];
  queueHistory: number[];
  shuffleEnabled: boolean;
  shuffleOrder: number[];
  pendingQueueIndex: number | null;
  youtubeHostUrl: string | null;
  youtubeFrame: HTMLIFrameElement | null;
  youtubeHostToken: string | null;
}

interface MusicYouTubeAdapterContext {
  state: MusicYouTubeState;
  loadRuntime: MusicLoadRuntime;
  effectiveVolume(): number;
  resolvePlaylist(sources: MusicSource[], selectedIndex: number | null, autoplay: boolean): Promise<void>;
  persist(force?: boolean): Promise<void>;
  updateExternalControls(): void;
  onDurationKnown(videoId: string, durationMs: number): void;
  setPlaybackStarting(starting: boolean): void;
  getHostUrl?: typeof getYouTubeHostUrl;
  persistYouTubeVideo?: (request: MusicYouTubeVideoWrite) => Promise<unknown>;
  persistYouTubePlaylist?: (request: MusicYouTubePlaylistSnapshotWrite) => Promise<unknown>;
  reportYouTubeFailure?: (request: MusicYouTubeSourceFailureWrite) => Promise<void>;
  now?(): number;
}

export interface MusicYouTubeAdapter {
  registerFrame(frame: HTMLIFrameElement | null): void;
  /** Routes commands through an Android native host instead of the iframe while one is registered. */
  registerNativeHost(host: NativeYouTubeHost | null): void;
  load(
    source: YouTubeSource,
    positionMs: number | null,
    generation: number,
    autoplay: boolean,
  ): Promise<void>;
  handleMessage(event: MessageEvent<unknown>): void;
  /** Handles a message the native host already scoped to its own page. */
  handleNativeData(data: unknown): void;
  post(payload: Record<string, unknown> & { action: YouTubeCommandAction | "snapshot" }): void;
  beginOptimisticPause(): void;
  clearOptimisticPause(): void;
  destroy(): void;
  clearPlaylistResolution(): void;
}

/** Owns the YouTube host protocol, playlist resolution, and optimistic pause policy. */
export function createMusicYouTubeAdapter(
  context: MusicYouTubeAdapterContext,
): MusicYouTubeAdapter {
  const state = context.state;
  const getHostUrl = context.getHostUrl ?? getYouTubeHostUrl;
  const persistYouTubeVideo = context.persistYouTubeVideo ?? upsertMusicYouTubeVideo;
  const persistYouTubePlaylist = context.persistYouTubePlaylist ?? applyMusicYouTubePlaylistSnapshot;
  const reportYouTubeFailure = context.reportYouTubeFailure ?? reportMusicYouTubeSourceFailure;
  const currentTime = context.now ?? Date.now;
  let hostBaseUrl: string | null = null;
  let nativeHost: NativeYouTubeHost | null = null;
  let hostLoadId: string | null = null;
  let resolvingPlaylist: ResolvingYouTubePlaylist | null = null;
  let playlistTimeoutId: number | null = null;
  let optimisticPauseUntilMs = 0;
  let lastMetadataSignature = "";
  let start: YouTubeStartState = YOUTUBE_START_IDLE;
  let startTimeoutId: ReturnType<typeof setTimeout> | null = null;

  function clearStartTimeout(): void {
    if (startTimeoutId === null) return;
    clearTimeout(startTimeoutId);
    startTimeoutId = null;
  }

  /** Moves the autoplay start to `next`, publishes it, and arms its deadline while starting. */
  function setStart(next: YouTubeStartState): void {
    start = next;
    clearStartTimeout();
    context.setPlaybackStarting(isYouTubeStarting(next));
    if (next.phase !== "starting") return;
    const { generation, deadlineMs } = next;
    startTimeoutId = setTimeout(() => {
      startTimeoutId = null;
      const expired = youtubeStartAfterDeadline(start, generation, deadlineMs);
      if (expired !== start) setStart(expired);
    }, Math.max(0, deadlineMs - currentTime()));
  }

  function reportPersistenceError(operation: string, error: unknown): void {
    console.error(`Unable to ${operation}.`, error);
  }

  /** Shows a localized playback failure and reports its stable code to the native session. */
  function failPlayback(code: MusicPlaybackErrorCode): void {
    state.playerError = musicPlaybackErrorMessage(code);
    state.snapshot = {
      ...state.snapshot,
      status: "error",
      error: code,
    };
  }

  function persistVideo(request: MusicYouTubeVideoWrite): void {
    void persistYouTubeVideo(request).catch((error: unknown) => {
      reportPersistenceError("save YouTube metadata", error);
    });
  }

  function reportPlaylistFailure(
    resolving: ResolvingYouTubePlaylist,
    resolutionState: "unavailable" | "embedding-blocked" | "timed-out",
    errorCode: string,
  ): void {
    void reportYouTubeFailure({
      playlistId: resolving.playlistId,
      name: resolving.playlistId,
      resolutionState,
      errorCode,
      occurredAt: currentTime(),
    }).catch((error: unknown) => {
      reportPersistenceError("record the YouTube playlist issue", error);
    });
  }

  function registerFrame(frame: HTMLIFrameElement | null): void {
    state.youtubeFrame = frame;
  }

  function registerNativeHost(host: NativeYouTubeHost | null): void {
    nativeHost = host;
  }

  function post(
    payload: Record<string, unknown> & { action: YouTubeCommandAction | "snapshot" },
  ): void {
    if (!state.youtubeHostToken) return;
    const message = {
      type: "ganbaru-youtube-command",
      token: state.youtubeHostToken,
      ...payload,
    };
    if (nativeHost) {
      nativeHost.post(message);
      return;
    }
    if (!state.youtubeFrame?.contentWindow) return;
    state.youtubeFrame.contentWindow.postMessage(
      message,
      state.youtubeHostUrl ? new URL(state.youtubeHostUrl).origin : "*",
    );
  }

  function clearPlaylistTimeout(): void {
    if (playlistTimeoutId === null) return;
    if (typeof window !== "undefined") window.clearTimeout(playlistTimeoutId);
    playlistTimeoutId = null;
  }

  function clearPlaylistResolution(): void {
    resolvingPlaylist = null;
    clearPlaylistTimeout();
  }

  function failPlaylistResolution(playlistId: string): void {
    if (!resolvingPlaylist || resolvingPlaylist.playlistId !== playlistId) return;
    const failed = resolvingPlaylist;
    clearPlaylistResolution();
    reportPlaylistFailure(failed, "timed-out", "playlist-resolution-timeout");
    failPlayback("youtube-playlist-empty");
    void context.persist();
  }

  function startPlaylistTimeout(resolving: ResolvingYouTubePlaylist): void {
    clearPlaylistTimeout();
    if (typeof window === "undefined") return;
    playlistTimeoutId = window.setTimeout(() => {
      if (
        resolvingPlaylist?.generation === resolving.generation
        && resolvingPlaylist.playlistId === resolving.playlistId
      ) failPlaylistResolution(resolving.playlistId);
    }, PLAYLIST_RESOLVE_TIMEOUT_MS);
  }

  async function ensureHostFrame(
    generation: number,
    source: YouTubeSource,
    positionMs: number | null,
    autoplay: boolean,
  ): Promise<void> {
    if (!hostBaseUrl) hostBaseUrl = await getHostUrl();
    if (!context.loadRuntime.isCurrent(generation)) return;
    const url = buildYouTubeHostUrl({
      baseUrl: hostBaseUrl,
      generation,
      source,
      positionMs,
      autoplay,
      volume: context.effectiveVolume(),
      rate: state.snapshot.rate,
    });
    state.youtubeHostUrl = url.toString();
    state.youtubeHostToken = url.searchParams.get("token");
    hostLoadId = url.searchParams.get("load");
    await tick();
  }

  async function load(
    source: YouTubeSource,
    positionMs: number | null,
    generation: number,
    autoplay: boolean,
  ): Promise<void> {
    setStart(beginYouTubeStart(generation, autoplay, currentTime()));
    lastMetadataSignature = "";
    resolvingPlaylist = source.kind === "youtube-playlist"
      ? {
          playlistId: source.playlistId,
          preferredVideoId: source.videoId,
          startMs: source.startMs,
          endMs: source.endMs,
          autoplay,
          generation,
        }
      : null;
    if (resolvingPlaylist) startPlaylistTimeout(resolvingPlaylist);
    else if (source.kind === "youtube-video") {
      clearPlaylistTimeout();
      persistVideo({
        videoId: source.videoId,
        title: source.title === source.videoId ? "" : source.title,
        channel: "",
        durationMs: null,
        resolutionState: "resolving",
        resolvedAtMs: currentTime(),
      });
    }
    try {
      await ensureHostFrame(generation, source, positionMs, autoplay);
    } catch (error) {
      if (!context.loadRuntime.isCurrent(generation)) return;
      setStart(YOUTUBE_START_IDLE);
      clearPlaylistResolution();
      console.error("Unable to start the YouTube player.", error);
      failPlayback("youtube-player-failed");
    }
  }

  function matchesVideo(source: MusicSource, videoId: string | null): boolean {
    if (source.kind === "youtube-video") {
      return videoId ? source.videoId === videoId : true;
    }
    if (source.kind === "youtube-playlist") {
      return source.videoId !== null && (videoId ? source.videoId === videoId : true);
    }
    return false;
  }

  function applyMetadataTitle(videoId: string | null, title: string | null): void {
    const resolvedTitle = title?.trim();
    if (!resolvedTitle) return;
    if (
      state.currentSource
      && matchesVideo(state.currentSource, videoId)
      && state.currentSource.title !== resolvedTitle
    ) state.currentSource = { ...state.currentSource, title: resolvedTitle };
    let changed = false;
    const queue = state.queue.map((source) => {
      if (!matchesVideo(source, videoId) || source.title === resolvedTitle) return source;
      changed = true;
      return { ...source, title: resolvedTitle };
    });
    if (changed) state.queue = queue;
  }

  async function applyPlaylist(message: YouTubeHostPlaylistMessage): Promise<void> {
    const resolving = resolvingPlaylist;
    if (
      !resolving
      || !context.loadRuntime.isCurrent(resolving.generation)
      || resolving.playlistId !== message.playlistId
      || message.videoIds.length === 0
    ) return;
    try {
      await persistYouTubePlaylist({
        playlistId: message.playlistId,
        name: state.currentSource?.title || message.playlistId,
        videoIds: message.videoIds,
        resolvedAtMs: currentTime(),
      });
    } catch (error) {
      reportPersistenceError("save the YouTube playlist snapshot", error);
    }
    if (!context.loadRuntime.isCurrent(resolving.generation)) return;
    const preferredIndex = resolving.preferredVideoId
      ? message.videoIds.indexOf(resolving.preferredVideoId)
      : -1;
    const sources = message.videoIds.map((videoId, index) => youtubeVideoSourceFromId(videoId, {
      startMs: index === preferredIndex ? resolving.startMs : null,
      endMs: index === preferredIndex ? resolving.endMs : null,
    }));
    clearPlaylistResolution();
    await context.resolvePlaylist(sources, preferredIndex >= 0 ? preferredIndex : null, resolving.autoplay);
  }

  function handleMessage(event: MessageEvent<unknown>): void {
    if (nativeHost) return;
    if (!state.youtubeFrame?.contentWindow || event.source !== state.youtubeFrame.contentWindow) return;
    handleHostData(event.data);
  }

  function handleNativeData(data: unknown): void {
    if (!nativeHost) return;
    handleHostData(data);
  }

  function handleHostData(data: unknown): void {
    const message = parseYouTubeHostMessage(data);
    if (
      !message
      || message.token !== state.youtubeHostToken
      || message.load !== hostLoadId
    ) return;
    if (message.type === "ganbaru-youtube-ready") return;
    if (message.type === "ganbaru-youtube-error") {
      setStart(YOUTUBE_START_IDLE);
      const failedPlaylist = resolvingPlaylist;
      clearPlaylistResolution();
      const resolutionState = message.code === 101 || message.code === 150
        ? "embedding-blocked"
        : "unavailable";
      if (failedPlaylist) {
        reportPlaylistFailure(failedPlaylist, resolutionState, `youtube-error-${message.code}`);
      } else {
        const source = state.currentSource;
        if (source?.kind === "youtube-video") {
          persistVideo({
            videoId: source.videoId,
            title: source.title === source.videoId ? "" : source.title,
            channel: "",
            durationMs: state.snapshot.durationMs,
            resolutionState,
            resolvedAtMs: currentTime(),
          });
        }
      }
      failPlayback(youtubePlaybackErrorCode(message.code));
      void context.persist();
      return;
    }
    if (message.type === "ganbaru-youtube-playlist") {
      void applyPlaylist(message);
      return;
    }
    if (message.type === "ganbaru-youtube-playlist-error") {
      failPlaylistResolution(message.playlistId);
      return;
    }
    if (state.currentSource?.kind === "youtube-playlist" && !resolvingPlaylist) return;
    applyMetadataTitle(message.videoId, message.title);
    if (message.videoId && message.durationMs !== null && Number.isFinite(message.durationMs) && message.durationMs > 0) {
      context.onDurationKnown(message.videoId, message.durationMs);
    }
    if (message.videoId) {
      const signature = [message.videoId, message.title ?? "", message.channel ?? "", message.durationMs ?? ""].join("\u0000");
      if (signature !== lastMetadataSignature) {
        lastMetadataSignature = signature;
        persistVideo({
          videoId: message.videoId,
          title: message.title ?? "",
          channel: message.channel ?? "",
          durationMs: message.durationMs,
          resolutionState: "ready",
          resolvedAtMs: currentTime(),
        });
      }
    }
    if (message.status === "playing" && currentTime() < optimisticPauseUntilMs) return;
    const settled = youtubeStartAfterStatus(start, message.status);
    if (settled !== start) setStart(settled);
    if (message.status !== "playing") optimisticPauseUntilMs = 0;
    const status = stableStatusDuringYouTubeBuffering(
      state.snapshot.status,
      message.status,
    );
    state.snapshot = {
      ...state.snapshot,
      status,
      positionMs: message.positionMs,
      durationMs: message.durationMs,
      error: null,
    };
    context.updateExternalControls();
    void context.persist();
  }

  function beginOptimisticPause(): void {
    optimisticPauseUntilMs = currentTime() + OPTIMISTIC_PAUSE_MS;
  }

  function clearOptimisticPause(): void {
    optimisticPauseUntilMs = 0;
  }

  function destroy(): void {
    setStart(YOUTUBE_START_IDLE);
    clearPlaylistResolution();
    if (state.currentSource && isYouTubeSource(state.currentSource)) {
      post({ action: "stop" });
    }
  }

  return {
    registerFrame,
    registerNativeHost,
    load,
    handleMessage,
    handleNativeData,
    post,
    beginOptimisticPause,
    clearOptimisticPause,
    destroy,
    clearPlaylistResolution,
  };
}
