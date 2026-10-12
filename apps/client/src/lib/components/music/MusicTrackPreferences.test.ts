// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { localFileSourceFromPath } from "$lib/music/sources";
import { musicSnoozeEndsAt } from "$lib/music/session/snooze";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";

const api = vi.hoisted(() => ({
  bulkEditMusicMemberships: vi.fn().mockResolvedValue(undefined),
  bulkSnoozeMusicItems: vi.fn().mockResolvedValue(undefined),
  getMusicInspectorDetail: vi.fn().mockResolvedValue({ snoozes: [] }),
  getMusicMembershipMatrix: vi.fn().mockResolvedValue([]),
  getMusicPlaylistSummaries: vi.fn().mockResolvedValue([]),
  removeMusicSnooze: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("$lib/api/music-library", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/music-library")>(),
  ...api,
}));

describe("MusicTrackPreferences", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    const player = getMusicPlayer();
    vi.spyOn(player, "currentQueueIndex", "get").mockReturnValue(0);
    vi.spyOn(player, "setPlaybackMode").mockImplementation((mode) => {
      player.shuffleEnabled = mode !== "in-order";
      player.mixEnabled = mode === "mix";
    });
    vi.spyOn(player, "applyCurrentQueueSnooze").mockImplementation(() => undefined);
    vi.spyOn(player, "clearCurrentQueueSnooze").mockImplementation(() => undefined);
    vi.spyOn(player, "playNextTrack").mockResolvedValue(undefined);
    vi.spyOn(player, "pausePlayback").mockResolvedValue(undefined);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    const player = getMusicPlayer();
    player.currentSource = null;
    player.queue = [];
    player.activeQueueItemIds = [];
    player.activePlaylistId = null;
    player.setPlaybackMode("shuffle");
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("shows each die's multiplier and warns when another playback order is active", async () => {
    const player = getMusicPlayer();
    player.setPlaybackMode("shuffle");
    const source = localFileSourceFromPath("/music/help.flac", "Help");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["help"];
    api.getMusicMembershipMatrix.mockResolvedValueOnce([{ itemId: "help", playlistId: "a", weight: "normal" }]);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector("[data-music-weight='normal']")).not.toBeNull());
    const warningButton = () => target?.querySelector<HTMLButtonElement>("[data-music-frequency-warning-trigger]");
    expect(target.querySelector("[data-music-frequency-help-trigger]")).toBeNull();
    expect(target.textContent).toContain("Normal");
    expect(target.textContent).not.toContain("Normal (1x)");
    expect(warningButton()?.textContent).toBe("!");
    expect(warningButton()?.getAttribute("aria-label")).toContain("playback order is Shuffle");
    expect(warningButton()?.getAttribute("aria-label")).toContain("Switch to Mix to use it");
    warningButton()?.focus();
    await vi.waitFor(() => expect(document.querySelector("[data-music-frequency-tooltip='warning'][data-placement]")).not.toBeNull());
    expect(document.querySelectorAll("[data-music-frequency-tooltip='warning'] svg")).toHaveLength(2);
    expect(document.querySelector("[data-music-frequency-tooltip='warning']")?.textContent?.replace(/\s+/g, " ")).toContain("Shuffle. Switch to Mix to use it.");

    player.setPlaybackMode("in-order");
    await tick();
    expect(warningButton()?.getAttribute("aria-label")).toContain("playback order is In order");

    player.setPlaybackMode("mix");
    await tick();
    expect(warningButton()).toBeNull();
    for (const [weight, label] of [["rarely", "Rarely (0.5x)"], ["less-often", "Less often (0.75x)"], ["normal", "Normal (1x)"], ["more-often", "More often (1.5x)"], ["much-more-often", "Much more (2x)"]]) {
      const die = target.querySelector<HTMLButtonElement>(`[data-music-weight='${weight}']`);
      expect(die?.getAttribute("aria-label")).toBe(label);
      expect(die?.getAttribute("data-app-tooltip")).toBe(label);
      die?.focus();
      await tick();
      expect(document.querySelector("[data-music-frequency-tooltip]")).toBeNull();
    }
  });

  it("applies Everywhere Mix frequency to every membership and lets the selector target one playlist", async () => {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/test.flac", "Test");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["song"];
    api.getMusicMembershipMatrix.mockResolvedValueOnce([
      { itemId: "song", playlistId: "a", weight: "normal" },
      { itemId: "song", playlistId: "b", weight: "rarely" },
    ]);
    api.getMusicPlaylistSummaries.mockResolvedValueOnce([
      { id: "a", name: "Morning" },
      { id: "b", name: "Evening" },
    ]);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector("[data-music-weight='much-more-often']")?.hasAttribute("disabled")).toBe(false));
    expect(target.querySelector("select")).toBeNull();
    expect(target.textContent).toContain("Mixed");
    expect(target.textContent?.indexOf("Mix frequency")).toBeLessThan(target.textContent?.indexOf("Snooze") ?? 0);

    target.querySelector<HTMLButtonElement>("[data-music-weight='much-more-often']")?.click();
    await vi.waitFor(() => expect(api.bulkEditMusicMemberships).toHaveBeenCalledOnce());
    expect(api.bulkEditMusicMemberships.mock.calls[0][0]).toMatchObject({
      itemIds: ["song"], weightPlaylistIds: ["a", "b"], weight: "much-more-often",
    });

    await vi.waitFor(() => expect(target?.querySelector<HTMLButtonElement>("[data-music-weight='much-more-often']")?.getAttribute("aria-disabled")).toBe("false"));
    target.querySelector<HTMLButtonElement>("button[aria-label='Apply to']")?.click();
    await vi.waitFor(() => expect(document.querySelector("[role='option']")?.textContent).toContain("Everywhere"));
    const morning = [...document.querySelectorAll<HTMLButtonElement>("[role='option']")]
      .find((button) => button.textContent?.includes("Morning"));
    morning?.click();
    await tick();
    target.querySelector<HTMLButtonElement>("[data-music-weight='rarely']")?.click();
    await vi.waitFor(() => expect(api.bulkEditMusicMemberships).toHaveBeenCalledTimes(2));
    expect(api.bulkEditMusicMemberships.mock.calls[1][0]).toMatchObject({
      itemIds: ["song"], weightPlaylistIds: ["a"], weight: "rarely",
    });

    await vi.waitFor(() => expect(target?.querySelector<HTMLButtonElement>("[data-music-weight='rarely']")?.getAttribute("aria-disabled")).toBe("false"));
    const day = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "1 day");
    day?.click();
    await vi.waitFor(() => expect(api.bulkSnoozeMusicItems).toHaveBeenCalledOnce());
    expect(api.bulkSnoozeMusicItems.mock.calls[0][0]).toMatchObject({
      itemIds: ["song"], scope: "playlist", playlistId: "a",
    });
  });

  it("keeps snooze available but disables Mix frequency without playlist memberships", async () => {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/unassigned.flac", "Unassigned");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["unassigned"];
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector("[data-music-weight='rarely']")).not.toBeNull());
    expect(target.querySelector<HTMLButtonElement>("button[aria-label='Apply to']")?.disabled).toBe(true);
    expect(target.querySelectorAll("[role='group'][aria-label='Mix frequency'] button:disabled")).toHaveLength(5);
    const day = [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "1 day");
    expect(day?.disabled).toBe(false);
    day?.click();
    await vi.waitFor(() => expect(api.bulkSnoozeMusicItems).toHaveBeenCalledOnce());
    expect(api.bulkSnoozeMusicItems.mock.calls[0][0]).toMatchObject({
      itemIds: ["unassigned"], scope: "all-playlists", playlistId: null,
    });
  });

  it("requests native queue reconciliation after snoozing without selecting the successor", async () => {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/only.flac", "Only");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["only"];
    const next = vi.spyOn(player, "playNextTrack").mockResolvedValue(undefined);
    const pause = vi.spyOn(player, "pausePlayback").mockResolvedValue(undefined);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector("[data-music-weight='normal']")).not.toBeNull());
    target.querySelector<HTMLButtonElement>(".snooze-choice")?.click();
    await vi.waitFor(() => expect(player.applyCurrentQueueSnooze).toHaveBeenCalledOnce());
    expect(next).not.toHaveBeenCalled();
    expect(pause).not.toHaveBeenCalled();
    expect(api.bulkSnoozeMusicItems.mock.calls[0][0]).toMatchObject({ itemIds: ["only"], scope: "all-playlists" });
    next.mockRestore();
    pause.mockRestore();
  });

  it("shows the saved duration and removes it when selected again", async () => {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/snoozed.flac", "Snoozed");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["snoozed"];
    const startsAtMs = Date.now() - 1_000;
    api.getMusicInspectorDetail.mockResolvedValueOnce({ snoozes: [{
      id: "snooze-1", itemId: "snoozed", scope: "all-playlists", playlistId: null,
      startsAtMs, endsAtMs: musicSnoozeEndsAt("week", startsAtMs, Intl.DateTimeFormat().resolvedOptions().timeZone),
      reason: "", createdAtMs: startsAtMs,
    }] });
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector(".snooze-choice[aria-pressed='true']")?.textContent).toBe("1 week"));
    expect(target.textContent).not.toContain("Resume");
    target.querySelector<HTMLButtonElement>(".snooze-choice[aria-pressed='true']")?.click();
    await vi.waitFor(() => expect(api.removeMusicSnooze).toHaveBeenCalledWith("snooze-1"));
    expect(api.bulkSnoozeMusicItems).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(target?.querySelector(".snooze-choice[aria-pressed='true']")).toBeNull());
  });

  it("keeps controls visually steady while saving a new Mix frequency", async () => {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/steady.flac", "Steady");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["steady"];
    api.getMusicMembershipMatrix.mockResolvedValueOnce([{ itemId: "steady", playlistId: "a", weight: "normal" }]);
    api.getMusicPlaylistSummaries.mockResolvedValueOnce([{ id: "a", name: "Morning" }]);
    let finishSave: (() => void) | undefined;
    api.bulkEditMusicMemberships.mockImplementationOnce(() => new Promise<void>((resolve) => { finishSave = resolve; }));
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicTrackPreferences } = await import("./MusicTrackPreferences.svelte");
    component = mount(MusicTrackPreferences, { target, props: { onOpen: vi.fn() } });
    await tick();

    target.querySelector<HTMLButtonElement>("button[aria-label='Mix and snooze']")?.click();
    await vi.waitFor(() => expect(target?.querySelector("[data-music-weight='much-more-often']")).not.toBeNull());
    target.querySelector<HTMLButtonElement>("[data-music-weight='much-more-often']")?.click();
    await tick();

    expect(target.querySelector("[data-music-track-preferences-open]")).not.toBeNull();
    expect(target.querySelector<HTMLButtonElement>("[data-music-weight='much-more-often']")?.getAttribute("aria-pressed")).toBe("true");
    expect(target.textContent).toContain("Much more");
    expect(target.textContent).not.toContain("Much more (2x)");
    expect(target.querySelectorAll("[role='group'][aria-label='Mix frequency'] button:disabled")).toHaveLength(0);
    expect(target.querySelector<HTMLButtonElement>("button[aria-label='Apply to']")?.disabled).toBe(false);
    finishSave?.();
    await vi.waitFor(() => expect(target?.querySelector<HTMLButtonElement>("[data-music-weight='much-more-often']")?.getAttribute("aria-disabled")).toBe("false"));
  });
});
