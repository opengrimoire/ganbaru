import { describe, expect, it, vi } from "vitest";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";
import { MusicPlaylistSummaryCache } from "./summary-cache.svelte";

const playlist = (id: string): MusicPlaylistSummary => ({
  id,
  sortOrder: 0,
  name: id,
  icon: "lucide:list-music",
  shuffleEnabled: false,
  mixEnabled: false,
  repeatMode: "all",
  intendedUses: [],
  totalCount: 1,
  eligibleCount: 1,
  unavailableCount: 0,
  snoozedCount: 0,
  localCount: 1,
  onlineCount: 0,
  version: 1,
});

describe("MusicPlaylistSummaryCache", () => {
  it("loads playlist summaries once and reuses them for later consumers", async () => {
    const summaries = vi.fn(async () => [playlist("focus")]);
    const cache = new MusicPlaylistSummaryCache({ summaries }, () => 100);
    cache.setVault("vault-1");

    expect(await cache.load()).toBe(true);
    expect(await cache.load()).toBe(true);
    expect(cache.playlists.map((entry) => entry.id)).toEqual(["focus"]);
    expect(summaries).toHaveBeenCalledOnce();
  });

  it("refreshes the retained list without clearing it first", async () => {
    const summaries = vi.fn()
      .mockResolvedValueOnce([playlist("focus")])
      .mockResolvedValueOnce([playlist("focus"), playlist("break")]);
    const cache = new MusicPlaylistSummaryCache({ summaries }, () => 100);
    cache.setVault("vault-1");
    await cache.load();

    const refreshing = cache.refresh();
    expect(cache.playlists.map((entry) => entry.id)).toEqual(["focus"]);
    expect(await refreshing).toBe(true);
    expect(cache.playlists.map((entry) => entry.id)).toEqual(["focus", "break"]);
  });

  it("drops summaries when the active vault changes", async () => {
    const cache = new MusicPlaylistSummaryCache({ summaries: vi.fn(async () => [playlist("focus")]) });
    cache.setVault("vault-1");
    await cache.load();

    cache.setVault("vault-2");

    expect(cache.loaded).toBe(false);
    expect(cache.playlists).toEqual([]);
  });
});
