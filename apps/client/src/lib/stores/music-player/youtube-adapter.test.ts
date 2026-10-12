import { describe, expect, it, vi } from "vitest";
import { DEFAULT_PLAYBACK_SNAPSHOT } from "$lib/music/playback";
import {
  parseMusicSourceInput,
  youtubeVideoSourceFromId,
} from "$lib/music/sources";
import { MusicLoadRuntime } from "./load-runtime";
import {
  createMusicYouTubeAdapter,
  type MusicYouTubeState,
} from "./youtube-adapter";

describe("Music YouTube adapter", () => {
  it("keeps autoplay feedback until the host plays and clears it on failure", async () => {
    const source = youtubeVideoSourceFromId("video-1");
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "loading" },
      playerError: null,
      queue: [source],
      queueHistory: [],
      shuffleEnabled: false,
      shuffleOrder: [],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const generation = loadRuntime.begin();
    const setPlaybackStarting = vi.fn();
    const onDurationKnown = vi.fn();
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: vi.fn(async () => undefined),
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown,
      setPlaybackStarting,
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo: vi.fn(async () => undefined),
      persistYouTubePlaylist: vi.fn(async () => undefined),
      reportYouTubeFailure: vi.fn(async () => undefined),
    });
    await adapter.load(source, null, generation, true);
    expect(setPlaybackStarting).toHaveBeenLastCalledWith(true);

    const sendState = (status: "ready" | "playing", durationMs: number | null) => adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(generation),
        type: "ganbaru-ai-youtube-state",
        status,
        positionMs: 0,
        durationMs,
        videoId: "video-1",
        title: null,
        channel: null,
      },
    } as unknown as MessageEvent<unknown>);
    sendState("ready", null);
    expect(setPlaybackStarting).toHaveBeenLastCalledWith(true);
    expect(onDurationKnown).not.toHaveBeenCalled();
    sendState("playing", 10_000);
    expect(setPlaybackStarting).toHaveBeenLastCalledWith(false);
    expect(onDurationKnown).toHaveBeenCalledWith("video-1", 10_000);

    await adapter.load(source, null, generation, true);
    adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(generation),
        type: "ganbaru-ai-youtube-error",
        code: 150,
      },
    } as unknown as MessageEvent<unknown>);
    expect(setPlaybackStarting).toHaveBeenLastCalledWith(false);
  });

  it("reports a resolved YouTube playlist for native queue selection", async () => {
    const parsed = parseMusicSourceInput(
      "https://www.youtube.com/playlist?list=PL1234567890",
    );
    if (!parsed.source || parsed.source.kind !== "youtube-playlist") {
      throw new Error("Expected a YouTube playlist fixture");
    }
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: parsed.source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "loading" },
      playerError: null,
      queue: [parsed.source],
      queueHistory: [2],
      shuffleEnabled: false,
      shuffleOrder: [1],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const generation = loadRuntime.begin();
    const loadSource = vi.fn(async () => undefined);
    const persistYouTubePlaylist = vi.fn(async () => undefined);
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: loadSource,
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown: vi.fn(),
      setPlaybackStarting: vi.fn(),
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo: vi.fn(async () => undefined),
      persistYouTubePlaylist,
      reportYouTubeFailure: vi.fn(async () => undefined),
    });
    await adapter.load(parsed.source, null, generation, true);

    adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(generation),
        type: "ganbaru-ai-youtube-playlist",
        playlistId: parsed.source.playlistId,
        videoIds: ["video-a", "video-b", "video-c"],
        index: 0,
      },
    } as unknown as MessageEvent<unknown>);
    await vi.waitFor(() => expect(loadSource).toHaveBeenCalled());

    expect(loadSource).toHaveBeenCalledWith([
      expect.objectContaining({ identity: "youtube:video:video-a" }),
      expect.objectContaining({ identity: "youtube:video:video-b" }),
      expect.objectContaining({ identity: "youtube:video:video-c" }),
    ], null, true);
    expect(state.queue).toEqual([parsed.source]);
    expect(state.pendingQueueIndex).toBeNull();
    expect(persistYouTubePlaylist).toHaveBeenCalledWith(expect.objectContaining({
      playlistId: parsed.source.playlistId,
      videoIds: ["video-a", "video-b", "video-c"],
    }));
  });

  it("ignores stale playing snapshots during the optimistic pause window", async () => {
    let nowMs = 100;
    const source = youtubeVideoSourceFromId("video-1");
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "paused" },
      playerError: null,
      queue: [source],
      queueHistory: [],
      shuffleEnabled: false,
      shuffleOrder: [],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const generation = loadRuntime.begin();
    const persistYouTubeVideo = vi.fn(async () => undefined);
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: vi.fn(async () => undefined),
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown: vi.fn(),
      setPlaybackStarting: vi.fn(),
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo,
      persistYouTubePlaylist: vi.fn(async () => undefined),
      reportYouTubeFailure: vi.fn(async () => undefined),
      now: () => nowMs,
    });
    await adapter.load(source, null, generation, false);
    adapter.beginOptimisticPause();
    const message = {
      token: "test-token",
      load: String(generation),
      type: "ganbaru-ai-youtube-state",
      status: "playing",
      positionMs: 500,
      durationMs: 10_000,
      videoId: "video-1",
      title: "Video 1",
      channel: "Channel 1",
    };

    adapter.handleMessage({
      source: contentWindow,
      data: message,
    } as unknown as MessageEvent<unknown>);
    expect(state.snapshot.status).toBe("paused");

    nowMs = 2_000;
    adapter.handleMessage({
      source: contentWindow,
      data: message,
    } as unknown as MessageEvent<unknown>);
    expect(state.snapshot.status).toBe("playing");
    expect(persistYouTubeVideo).toHaveBeenLastCalledWith(expect.objectContaining({
      videoId: "video-1",
      title: "Video 1",
      channel: "Channel 1",
      resolutionState: "ready",
    }));
  });

  it("rejects a playlist result after a newer source generation starts", async () => {
    const parsed = parseMusicSourceInput(
      "https://www.youtube.com/playlist?list=PL1234567890",
    );
    if (!parsed.source || parsed.source.kind !== "youtube-playlist") {
      throw new Error("Expected a YouTube playlist fixture");
    }
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: parsed.source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "loading" },
      playerError: null,
      queue: [parsed.source],
      queueHistory: [],
      shuffleEnabled: false,
      shuffleOrder: [],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const playlistGeneration = loadRuntime.begin();
    const loadSource = vi.fn(async () => undefined);
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: loadSource,
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown: vi.fn(),
      setPlaybackStarting: vi.fn(),
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo: vi.fn(async () => undefined),
      persistYouTubePlaylist: vi.fn(async () => undefined),
      reportYouTubeFailure: vi.fn(async () => undefined),
    });
    await adapter.load(parsed.source, null, playlistGeneration, false);
    loadRuntime.begin();

    adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(playlistGeneration),
        type: "ganbaru-ai-youtube-playlist",
        playlistId: parsed.source.playlistId,
        videoIds: ["video-a", "video-b"],
        index: 0,
      },
    } as unknown as MessageEvent<unknown>);
    await Promise.resolve();

    expect(loadSource).not.toHaveBeenCalled();
    expect(state.queue).toEqual([parsed.source]);
  });

  it("persists embedding-blocked direct videos without losing the player error", async () => {
    const source = youtubeVideoSourceFromId("dQw4w9WgXcQ");
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "loading" },
      playerError: null,
      queue: [source],
      queueHistory: [],
      shuffleEnabled: false,
      shuffleOrder: [],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const generation = loadRuntime.begin();
    const persistYouTubeVideo = vi.fn(async () => undefined);
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: vi.fn(async () => undefined),
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown: vi.fn(),
      setPlaybackStarting: vi.fn(),
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo,
      persistYouTubePlaylist: vi.fn(async () => undefined),
      reportYouTubeFailure: vi.fn(async () => undefined),
      now: () => 100,
    });
    await adapter.load(source, null, generation, false);
    adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(generation),
        type: "ganbaru-ai-youtube-error",
        code: 150,
      },
    } as unknown as MessageEvent<unknown>);
    expect(persistYouTubeVideo).toHaveBeenLastCalledWith(expect.objectContaining({
      videoId: "dQw4w9WgXcQ",
      resolutionState: "embedding-blocked",
    }));
    expect(state.snapshot.status).toBe("error");
  });

  it("reports unavailable playlist resolution without replacing its last snapshot", async () => {
    const parsed = parseMusicSourceInput("https://www.youtube.com/playlist?list=PL1234567890");
    if (!parsed.source || parsed.source.kind !== "youtube-playlist") throw new Error("Expected playlist");
    const contentWindow = { postMessage: vi.fn() };
    const state: MusicYouTubeState = {
      currentSource: parsed.source,
      snapshot: { ...DEFAULT_PLAYBACK_SNAPSHOT, status: "loading" },
      playerError: null,
      queue: [parsed.source],
      queueHistory: [],
      shuffleEnabled: false,
      shuffleOrder: [],
      pendingQueueIndex: null,
      youtubeHostUrl: null,
      youtubeFrame: { contentWindow } as unknown as HTMLIFrameElement,
      youtubeHostToken: null,
      youtubeHostReady: false,
    };
    const loadRuntime = new MusicLoadRuntime();
    const generation = loadRuntime.begin();
    const reportYouTubeFailure = vi.fn(async () => undefined);
    const adapter = createMusicYouTubeAdapter({
      state,
      loadRuntime,
      effectiveVolume: () => 1,
      resolvePlaylist: vi.fn(async () => undefined),
      persist: vi.fn(async () => undefined),
      updateExternalControls: vi.fn(),
      onDurationKnown: vi.fn(),
      setPlaybackStarting: vi.fn(),
      getHostUrl: vi.fn(async () => "http://127.0.0.1:1234/player?token=test-token"),
      persistYouTubeVideo: vi.fn(async () => undefined),
      persistYouTubePlaylist: vi.fn(async () => undefined),
      reportYouTubeFailure,
      now: () => 100,
    });
    await adapter.load(parsed.source, null, generation, false);
    adapter.handleMessage({
      source: contentWindow,
      data: {
        token: "test-token",
        load: String(generation),
        type: "ganbaru-ai-youtube-error",
        code: 100,
      },
    } as unknown as MessageEvent<unknown>);
    expect(reportYouTubeFailure).toHaveBeenCalledWith(expect.objectContaining({
      playlistId: "PL1234567890",
      resolutionState: "unavailable",
    }));
  });
});
