import { listen } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_PLAYBACK_SNAPSHOT } from "$lib/music/playback";
import { localFileSourceFromPath } from "$lib/music/sources";
import { createMusicExternalControls } from "$lib/stores/music-player/external-controls";

function createContext() {
  const unlisten = vi.fn();
  const listenMock = vi.fn(async () => unlisten) as unknown as typeof listen;
  const context: Parameters<typeof createMusicExternalControls>[0] = {
    currentSource: () => null,
    snapshot: () => DEFAULT_PLAYBACK_SNAPSHOT,
    title: () => "Nothing loaded",
    sourceKindLabel: () => "No source",
    artworkUrl: () => null,
    isBusy: () => false,
    canPrevious: () => false,
    canNext: () => false,
    volume: () => 1,
    muted: () => false,
    shuffleEnabled: () => true,
    play: vi.fn(async () => undefined),
    pause: vi.fn(async () => undefined),
    togglePlay: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    previous: vi.fn(async () => undefined),
    next: vi.fn(async () => undefined),
    seekBy: vi.fn(async () => undefined),
    seekTo: vi.fn(async () => undefined),
    setVolume: vi.fn(async () => undefined),
    setRate: vi.fn(async () => undefined),
    toggleShuffle: vi.fn(),
    inspectAssignment: vi.fn(),
    handleWindowMessage: vi.fn(),
    listen: listenMock,
  };
  return {
    context,
    listenMock,
    unlisten,
  };
}

describe("Music external controls", () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });
  it("owns one listener set and releases every listener on destroy", async () => {
    const { context, listenMock, unlisten } = createContext();
    const controls = createMusicExternalControls(context);

    controls.init();
    controls.init();
    await Promise.resolve();

    expect(listenMock).toHaveBeenCalledTimes(1);
    expect(listenMock).toHaveBeenCalledWith("tray-music-inspect-assignment", expect.any(Function));
    controls.destroy();
    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(controls.isInitialized()).toBe(false);
  });

  it("releases browser metadata and handlers when its decoder no longer owns the source", () => {
    const handlers = new Map<string, unknown>();
    const mediaSession = {
      metadata: null as unknown,
      playbackState: "none",
      setActionHandler: vi.fn((action: string, handler: unknown) => { handlers.set(action, handler); }),
    };
    vi.stubGlobal("navigator", { mediaSession });
    vi.stubGlobal("MediaMetadata", class { constructor(public value: unknown) {} });
    const { context } = createContext();
    context.currentSource = () => localFileSourceFromPath("/music/video.mp4", "Video");
    context.snapshot = () => ({ ...DEFAULT_PLAYBACK_SNAPSHOT, status: "playing" });
    const controls = createMusicExternalControls(context);
    controls.updateBrowser();
    expect(mediaSession.metadata).not.toBeNull();
    expect(mediaSession.playbackState).toBe("playing");
    expect(handlers.get("play")).toBeTypeOf("function");
    context.currentSource = () => null;
    controls.updateBrowser();
    expect(mediaSession.metadata).toBeNull();
    expect(mediaSession.playbackState).toBe("none");
    expect([...handlers.values()]).toEqual([null, null, null, null, null]);
  });

  it("handles an unsupported browser media action without preventing decoder cleanup", () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const mediaSession = {
      metadata: null as unknown,
      playbackState: "none",
      setActionHandler: vi.fn((action: string) => {
        if (action === "stop") throw new Error("unsupported media action");
      }),
    };
    vi.stubGlobal("navigator", { mediaSession });
    vi.stubGlobal("MediaMetadata", class { constructor(public value: unknown) {} });
    const { context } = createContext();
    context.currentSource = () => localFileSourceFromPath("/music/video.mp4", "Video");
    context.snapshot = () => ({ ...DEFAULT_PLAYBACK_SNAPSHOT, status: "playing" });
    const controls = createMusicExternalControls(context);
    controls.updateBrowser(); controls.updateBrowser();
    context.currentSource = () => null;
    expect(() => controls.updateBrowser()).not.toThrow();
    expect(mediaSession.metadata).toBeNull();
    expect(mediaSession.playbackState).toBe("none");
    expect(warning).toHaveBeenCalledTimes(1);
  });

});
