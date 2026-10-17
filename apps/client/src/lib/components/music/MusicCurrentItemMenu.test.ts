// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { localFileSourceFromPath } from "$lib/music/sources";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
import { getMusicPlayer } from "$lib/stores/music-player.svelte";

const api = vi.hoisted(() => ({
  bulkEditMusicMemberships: vi.fn().mockResolvedValue({ changedCount: 2 }),
  getMusicMembershipMatrix: vi.fn().mockResolvedValue([{ itemId: "song", playlistId: "a", weight: "normal" }]),
  getMusicPlaylistSummaries: vi.fn(),
}));

vi.mock("$lib/api/music-library", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/music-library")>(),
  ...api,
}));

/** Build the playlist data required by the current-track chooser. */
function playlist(id: string, name: string): MusicPlaylistSummary {
  return {
    id, name, icon: "music", shuffleEnabled: false, mixEnabled: false, repeatMode: "off",
    intendedUses: [], sortOrder: id === "a" ? 0 : 1, totalCount: 0, eligibleCount: 0,
    unavailableCount: 0, snoozedCount: 0, localCount: 0, onlineCount: 0, version: 1,
  };
}

describe("MusicCurrentItemMenu", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    target = undefined;
    component = undefined;
    const player = getMusicPlayer();
    player.currentSource = null;
    player.queue = [];
    player.activeQueueItemIds = [];
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  async function render(onOpenBuilder = vi.fn(), nativeSelectedIndex = 0): Promise<HTMLDivElement> {
    const player = getMusicPlayer();
    const source = localFileSourceFromPath("/music/song.flac", "Song");
    player.currentSource = source;
    player.queue = [source];
    player.activeQueueItemIds = ["song"];
    // The accepted native projection supplies selection independently of display rows.
    vi.spyOn(player, "currentQueueIndex", "get").mockReturnValue(nativeSelectedIndex);
    api.getMusicPlaylistSummaries.mockResolvedValue([playlist("a", "Morning"), playlist("b", "Evening")]);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicCurrentItemMenu } = await import("./MusicCurrentItemMenu.svelte");
    component = mount(MusicCurrentItemMenu, { target, props: { onOpenBuilder } });
    await tick();
    return target;
  }

  it("stages additions and removals until Save and applies them together", async () => {
    const view = await render();
    view.querySelector<HTMLButtonElement>("button[aria-label='Current track playlists']")?.click();
    await vi.waitFor(() => expect(view.querySelectorAll("input[type='checkbox']")).toHaveLength(2));
    const morning = view.querySelector<HTMLInputElement>("input[aria-label='Remove from Morning']");
    const evening = view.querySelector<HTMLInputElement>("input[aria-label='Add to Evening']");
    expect(morning?.checked).toBe(true);
    expect(evening?.checked).toBe(false);
    expect(view.textContent).not.toContain("tracks");
    morning?.click();
    evening?.click();
    expect(api.bulkEditMusicMemberships).not.toHaveBeenCalled();
    const save = [...view.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "Save");
    await vi.waitFor(() => expect(save?.disabled).toBe(false));
    save?.click();
    await vi.waitFor(() => expect(api.bulkEditMusicMemberships).toHaveBeenCalledOnce());
    expect(api.bulkEditMusicMemberships.mock.calls[0][0]).toMatchObject({
      itemIds: ["song"], addPlaylistIds: ["b"], removePlaylistIds: ["a"], weightPlaylistIds: [], weight: null,
    });
  });

  it("discards an unsaved draft when closed and opens the builder", async () => {
    const onOpenBuilder = vi.fn();
    const view = await render(onOpenBuilder);
    const trigger = view.querySelector<HTMLButtonElement>("button[aria-label='Current track playlists']");
    trigger?.click();
    await vi.waitFor(() => expect(view.querySelector("input[aria-label='Add to Evening']")).not.toBeNull());
    view.querySelector<HTMLInputElement>("input[aria-label='Add to Evening']")?.click();
    trigger?.click();
    trigger?.click();
    await vi.waitFor(() => expect(view.querySelector<HTMLInputElement>("input[aria-label='Add to Evening']")?.checked).toBe(false));
    expect(api.bulkEditMusicMemberships).not.toHaveBeenCalled();
    const builder = [...view.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.includes("Open in playlist builder"));
    builder?.click();
    expect(onOpenBuilder).toHaveBeenCalledExactlyOnceWith("song");
  });

  it("opens the builder without a selected item when native selection is absent", async () => {
    const onOpenBuilder = vi.fn();
    const view = await render(onOpenBuilder, -1);
    view.querySelector<HTMLButtonElement>("button[aria-label='Playlist builder']")?.click();
    expect(onOpenBuilder).toHaveBeenCalledExactlyOnceWith(null);
    expect(api.getMusicMembershipMatrix).not.toHaveBeenCalled();
    expect(api.bulkEditMusicMemberships).not.toHaveBeenCalled();
  });
});
