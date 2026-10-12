// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { localFileSourceFromPath } from "$lib/music/sources";
import { emptyMusicSkipBreakdown } from "$lib/music/playlists/playback";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";
import { getMusicSourcesController } from "$lib/music/sources/controller.svelte";
import type { NativeMusicCommand, NativeMusicSnapshot } from "$lib/music/session/native-session";

const sessionApi = vi.hoisted(() => ({
  command: vi.fn(), start: vi.fn(), subscribe: vi.fn().mockResolvedValue(undefined),
  host: vi.fn().mockResolvedValue(undefined), acknowledge: vi.fn().mockResolvedValue(undefined),
  unsubscribe: vi.fn().mockResolvedValue(undefined), readFrame: vi.fn(), observe: vi.fn(),
}));

vi.mock("$lib/api/music-session", () => ({
  musicSessionCommand: sessionApi.command, musicSessionStart: sessionApi.start,
  musicSessionSubscribe: sessionApi.subscribe, musicSessionHost: sessionApi.host,
  musicSessionAcknowledge: sessionApi.acknowledge, musicSessionUnsubscribe: sessionApi.unsubscribe,
  musicSessionReadFrame: sessionApi.readFrame, musicSessionObserve: sessionApi.observe,
}));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...await importOriginal<typeof import("@tauri-apps/api/core")>(),
  Channel: class {
    constructor(public onmessage: (value: unknown) => void) {}
  },
}));

let nativeRevision = 0;
function nativeSnapshot(patch: Partial<NativeMusicSnapshot> = {}): NativeMusicSnapshot {
  nativeRevision += 1;
  return { sessionId: "panel-session", revision: nativeRevision, generation: 1, queueRevision: nativeRevision,
    currentIndex: null, currentSource: null, backend: null, status: "idle", positionMs: 0, durationMs: null,
    volume: 0.8, muted: false, rate: 1, order: "shuffle", repeatMode: "off", owner: "manual", context: null,
    playlistId: null, queueName: "", canPrevious: false, canNext: false, issue: null, error: null,
    reviewCheckpointId: null, queue: [], ...patch };
}

sessionApi.command.mockImplementation(async (request: NativeMusicCommand) => nativeSnapshot({
  order: request.intent.kind === "order" ? request.intent.order : "shuffle",
}));

const snoozeApi = vi.hoisted(() => ({
  getMusicInspectorDetail: vi.fn(),
  removeMusicSnooze: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/api/music-library", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/music-library")>(),
  ...snoozeApi,
}));

const resizeObserverCallbacks = new Set<() => void>();

class ResizeObserverStub {
  constructor(private readonly callback: () => void) {
    resizeObserverCallbacks.add(callback);
  }
  observe(): void {}
  disconnect(): void { resizeObserverCallbacks.delete(this.callback); }
}

function matchMediaStub(query: string): MediaQueryList {
  return {
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => true,
  };
}

describe("MusicPanel", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.unstubAllGlobals();
    resizeObserverCallbacks.clear();
    getMusicSourcesController().firstUseSession = false;
    const player = getMusicPlayer();
    player.setPlaylistVisible(false);
    player.currentSource = null;
    player.queue = [];
    player.activeQueueItemIds = [];
    player.sourceQueueSnoozedItemIds = [];
    player.savedQueueEntries = [];
    player.activePlaylistId = null;
    player.activePlaylistName = null;
    player.activeSourceQueueId = null;
    player.savedQueueSkipBreakdown = emptyMusicSkipBreakdown();
    player.shuffleEnabled = true;
    player.mixEnabled = false;
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("mounts an interactive dialog above its persistent media layer and closes with Escape", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const onClose = vi.fn();
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose } });
    await tick();

    const dialog = target.querySelector<HTMLElement>("[role='dialog']");
    expect(dialog).not.toBeNull();
    expect(dialog?.classList.contains("z-70")).toBe(true);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("uses a square playlist launcher and no close button in the mobile player", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, {
      target,
      props: { onClose: vi.fn(), presentation: "mobile" },
    });
    await tick();

    const launcher = target.querySelector<HTMLElement>("[data-music-playlist-launcher]");
    const header = target.querySelector<HTMLElement>("[data-music-player-header]");
    expect(launcher?.classList.contains("h-9")).toBe(true);
    expect(launcher?.classList.contains("w-9")).toBe(true);
    expect(header?.classList.contains("py-2")).toBe(true);
    expect(target.querySelector("button[aria-label='Close']")).toBeNull();
  });

  it("replaces the speed control with a disabled track-preferences button until a library track plays", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();

    const preferences = target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']");
    expect(preferences?.disabled).toBe(true);
    expect(target.querySelector("button[aria-label='Speed']")).toBeNull();
  });

  it("offers one playback-order choice with In order, Shuffle, and Mix", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    const player = getMusicPlayer();
    player.queue = [
      localFileSourceFromPath("/music/a.flac", "A"),
      localFileSourceFromPath("/music/b.flac", "B"),
    ];
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");
    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>(".music-transport-shuffle button")?.click();
    await tick();
    expect([...target.querySelectorAll("[role='menuitemradio']")].map((option) => option.textContent?.trim())).toEqual([
      "In order", "Shuffle", "Mix",
    ]);
    const mixHelp = target.querySelector<HTMLButtonElement>("button[aria-label='Favors your preferred tracks. Recent tracks are less likely, but repeats are possible.']");
    expect(mixHelp).not.toBeNull();
    expect(target.querySelector("button[aria-label='Plays each track once in random order. No repeats until the list ends.']")).not.toBeNull();
    mixHelp?.click();
    expect(player.playbackMode).toBe("shuffle");
    [...target.querySelectorAll<HTMLButtonElement>("[role='menuitemradio']")]
      .find((option) => option.textContent?.includes("Mix"))?.click();
    await vi.waitFor(() => expect(player.playbackMode).toBe("mix"));
    expect(sessionApi.command).toHaveBeenLastCalledWith(expect.objectContaining({ intent: { kind: "order", order: "mix" } }));
  });

  it("places the side playlist summary in the desktop player header", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const player = getMusicPlayer();
    player.setPlaylistVisible(true);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();
    const emptyDesktopHeader = target.querySelector<HTMLElement>("[data-music-desktop-playlist-header]");
    expect(emptyDesktopHeader).not.toBeNull();
    expect(emptyDesktopHeader?.textContent?.trim()).toBe("");
    expect(target.querySelector("#music-playlist")).not.toBeNull();
    expect(target.querySelector("[data-music-stacked-playlist-header]")).toBeNull();

    player.queue = [
      localFileSourceFromPath("/music/first.flac", "First"),
      localFileSourceFromPath("/music/second.flac", "Second"),
    ];
    await tick();

    const desktopHeader = target.querySelector<HTMLElement>("[data-music-desktop-playlist-header]");
    const stackedHeader = target.querySelector<HTMLElement>("[data-music-stacked-playlist-header]");
    expect(desktopHeader?.textContent).toContain("Playlist");
    expect(desktopHeader?.textContent).toContain("2 tracks");
    expect(desktopHeader?.classList.contains("min-[861px]:w-80")).toBe(true);
    expect(stackedHeader?.classList.contains("min-[861px]:hidden")).toBe(true);
  });

  it("removes a source queue Snooze without playing its row", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    const player = getMusicPlayer();
    player.setPlaylistVisible(true);
    player.queue = [localFileSourceFromPath("/music/snoozed.flac", "Snoozed")];
    player.activeQueueItemIds = ["snoozed"];
    player.activeSourceQueueId = "source:music";
    player.sourceQueueSnoozedItemIds = ["snoozed"];
    sessionApi.command.mockResolvedValueOnce(nativeSnapshot({ queue: [{
      itemId: "snoozed", membershipId: null, source: player.queue[0]!, backend: "native-audio",
      availability: "available", enabled: true, weight: "normal", snoozedUntil: null,
      snoozedIndefinitely: false, embeddingBlocked: false, bound: true, phaseAllowed: true,
      skipRanges: [], volume: null, rate: null,
    }] }));
    const play = vi.spyOn(player, "playQueueItem").mockResolvedValue(undefined);
    const now = Date.now();
    snoozeApi.getMusicInspectorDetail.mockResolvedValue({ snoozes: [{
      id: "active-snooze", itemId: "snoozed", scope: "all-playlists", playlistId: null,
      startsAtMs: now - 1_000, endsAtMs: now + 86_400_000, reason: "", createdAtMs: now - 1_000,
    }] });
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");
    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();

    const button = target.querySelector<HTMLButtonElement>("#music-playlist button[aria-label='Remove snooze']");
    expect(button).not.toBeNull();
    button?.click();
    await vi.waitFor(() => expect(snoozeApi.removeMusicSnooze).toHaveBeenCalledWith("active-snooze"));
    await vi.waitFor(() => expect(player.sourceQueueSnoozedItemIds).toEqual([]));
    expect(sessionApi.command).toHaveBeenLastCalledWith(expect.objectContaining({ intent: { kind: "refresh" } }));
    expect(player.queue.map((source) => source.title)).toEqual(["Snoozed"]);
    expect(play).not.toHaveBeenCalled();
  });

  it("shows queue title tooltips only when the visible title is cut off", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    const player = getMusicPlayer();
    player.setPlaylistVisible(true);
    player.queue = [
      localFileSourceFromPath("/music/short.flac", "Short"),
      localFileSourceFromPath("/music/long.flac", "A very long song title"),
    ];
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");
    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();

    const rows = [...target.querySelectorAll<HTMLElement>("#music-playlist [data-music-queue-title]")];
    const buttons = [...target.querySelectorAll<HTMLButtonElement>("#music-playlist button[data-playlist-index]")];
    expect(rows).toHaveLength(2);
    expect(buttons).toHaveLength(2);
    Object.defineProperties(rows[0]!, { scrollWidth: { value: 45 }, clientWidth: { value: 50 } });
    Object.defineProperties(rows[1]!, { scrollWidth: { value: 160 }, clientWidth: { value: 100, configurable: true } });

    for (const callback of resizeObserverCallbacks) callback();
    expect(buttons[0]?.dataset.appTooltipDisabled).toBe("true");
    expect(buttons[1]?.dataset.appTooltipDisabled).toBe("false");
    expect(buttons[0]?.getAttribute("aria-label")).toBe("Short");
    expect(buttons[1]?.getAttribute("aria-label")).toBe("A very long song title");

    Object.defineProperty(rows[1]!, "clientWidth", { value: 200, configurable: true });
    for (const callback of resizeObserverCallbacks) callback();
    expect(buttons[1]?.dataset.appTooltipDisabled).toBe("true");
  });

  it("shows unavailable playlist recovery inside the media area without chooser actions", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const player = getMusicPlayer();
    player.activePlaylistId = "focus";
    player.activePlaylistName = "Deep focus";
    player.savedQueueSkipBreakdown = {
      ...emptyMusicSkipBreakdown(),
      unavailable: 2,
    };
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();

    const unavailable = target.querySelector<HTMLElement>("[data-music-playlist-unavailable]");
    expect(unavailable?.closest(".music-media-cell")).not.toBeNull();
    expect(unavailable?.textContent).toContain("Nothing available to play");
    expect(unavailable?.textContent).toContain("Review playlist");
    expect(target.textContent).not.toContain("Choose another playlist");
    expect(target.textContent).not.toContain("Retry");
  });

  it("does not reset persistent playback state when the panel closes", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/persistent.flac", "Persistent");
    player.currentSource = source;
    player.queue = [source];
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();
    await unmount(component);
    component = undefined;

    expect(getMusicPlayer()).toBe(player);
    expect(player.currentSource).toEqual(source);
    expect(player.queue).toEqual([source]);

    player.currentSource = null;
    player.queue = [];
  });

  it("owns playback arrow shortcuts before the underlying tab can handle them", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const player = getMusicPlayer();
    const adjustVolume = vi.spyOn(player, "adjustVolume").mockResolvedValue();
    const underlyingKeydown = vi.fn();
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();
    window.addEventListener("keydown", underlyingKeydown);
    const event = new KeyboardEvent("keydown", {
      key: "ArrowUp",
      bubbles: true,
      cancelable: true,
    });

    window.dispatchEvent(event);
    window.removeEventListener("keydown", underlyingKeydown);

    expect(event.defaultPrevented).toBe(true);
    expect(adjustVolume).toHaveBeenCalledWith(0.05);
    expect(underlyingKeydown).not.toHaveBeenCalled();
  });

  it("lets the playlist chooser close with Escape without closing Music", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    target = document.createElement("div");
    document.body.append(target);
    const onClose = vi.fn();
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose } });
    await tick();
    target.querySelector<HTMLButtonElement>("[data-music-playlist-launcher]")?.click();
    await vi.waitFor(() => {
      expect(document.body.querySelector(".playlist-launcher-popover")).not.toBeNull();
    });

    window.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }));
    await tick();

    expect(document.body.querySelector(".playlist-launcher-popover")).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps the initialized builder mounted but inactive while returning to the player", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("matchMedia", matchMediaStub);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();
    const playerPage = target.querySelector<HTMLElement>("[data-music-player-page]");
    expect(playerPage).not.toBeNull();
    expect(target.querySelector("#music-source")).toBeNull();
    expect(target.querySelector("button[aria-label='Pick folder']")).toBeNull();
    expect(target.querySelector("button[aria-label='Load source']")).toBeNull();
    expect(target.querySelector("button[aria-label='Reset']")).toBeNull();

    target.querySelector<HTMLButtonElement>("[data-music-playlist-launcher]")?.click();
    await vi.waitFor(() => {
      expect([...document.body.querySelectorAll<HTMLButtonElement>("button")]
        .some((button) => button.textContent?.includes("Open playlist builder"))).toBe(true);
    });
    const firstOpenBuilder = [...document.body.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Open playlist builder"));
    firstOpenBuilder?.click();
    await vi.waitFor(() => {
      expect(target?.querySelector(".builder-root"), target?.textContent ?? "").not.toBeNull();
    }, { timeout: 5_000 });
    expect(target.querySelector("[data-music-player-page]")).toBe(playerPage);
    expect(playerPage?.classList.contains("hidden")).toBe(true);
    expect(target.textContent).not.toContain("Everything is reviewed");
    expect(target.textContent).toMatch(/Opening your music library|Preparing your Music folder/);

    window.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }));
    await tick();
    expect(target.querySelector("[data-music-player-page]")).toBe(playerPage);
    expect(playerPage?.classList.contains("hidden")).toBe(false);
    expect(target.querySelector(".builder-root")).not.toBeNull();
    expect(target.querySelector(".builder-root")?.closest("[aria-hidden='true']")).not.toBeNull();

    target.querySelector<HTMLButtonElement>("[data-music-playlist-launcher]")?.click();
    await vi.waitFor(() => {
      expect([...document.body.querySelectorAll<HTMLButtonElement>("button")]
        .some((button) => button.textContent?.includes("Open playlist builder"))).toBe(true);
    });
    const openBuilder = [...document.body.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Open playlist builder"));
    openBuilder?.click();
    await tick();
    expect(target.querySelector(".builder-root")).not.toBeNull();
    expect(target.querySelector("[data-music-player-page]")).toBe(playerPage);
  });

  it("applies builder and expanded playlist heights in the same view transition", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("matchMedia", matchMediaStub);
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.matches("[data-music-player-header]")) return new DOMRect(0, 0, 1_000, 40);
      if (this.matches(".music-media-cell")) return new DOMRect(0, 40, 680, 383);
      if (this.matches("#music-playlist")) return new DOMRect(680, 40, 320, 383);
      return new DOMRect(0, 0, 1_000, 80);
    });
    target = document.createElement("div");
    document.body.append(target);
    const player = getMusicPlayer();
    player.queue = [localFileSourceFromPath("/music/track.flac", "Track")];
    player.setPlaylistVisible(true);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });
    await tick();
    const dialog = target.querySelector<HTMLElement>("[role='dialog']");
    expect(dialog?.style.height).toContain("503px");

    target.querySelector<HTMLButtonElement>("[data-music-playlist-launcher]")?.click();
    await vi.waitFor(() => {
      expect([...document.body.querySelectorAll<HTMLButtonElement>("button")]
        .some((button) => button.textContent?.includes("Open playlist builder"))).toBe(true);
    });
    const openBuilder = [...document.body.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Open playlist builder"));
    openBuilder?.click();
    await tick();

    expect(dialog?.style.height).toContain("680px");
    await vi.waitFor(() => {
      expect(target?.querySelector(".builder-root"), target?.textContent ?? "").not.toBeNull();
    }, { timeout: 5_000 });

    window.dispatchEvent(new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }));
    await tick();

    expect(dialog?.style.height).toContain("503px");
  });

  it("keeps first-use preparation pending when the panel closes before completion", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    vi.stubGlobal("matchMedia", matchMediaStub);
    getMusicSourcesController().firstUseSession = true;
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPanel } = await import("./MusicPanel.svelte");

    component = mount(MusicPanel, { target, props: { onClose: vi.fn() } });

    await vi.waitFor(() => {
      expect(target?.querySelector(".builder-root"), target?.textContent ?? "").not.toBeNull();
    }, { timeout: 5_000 });
    expect(target.querySelector("[data-music-player-page]")?.classList.contains("hidden")).toBe(true);

    await unmount(component);
    component = undefined;

    expect(getMusicSourcesController().firstUseSession).toBe(true);
  });
});
