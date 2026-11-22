// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  MusicItemWindow,
  MusicItemWindowRequest,
  MusicPlaylist,
  MusicPlaylistSummary,
} from "$lib/music/library/contracts";
import type { MusicPlaylistController } from "$lib/music/playlists/controller.svelte";
import { getMusicSourcesController } from "$lib/music/sources/controller.svelte";

const harness = vi.hoisted(() => ({
  controllers: [] as Array<{ detail: { id: string } | null }>,
  heldPlaylistWindows: new Map<string, () => void>(),
  holdPlaylistIds: new Set<string>(),
}));

/** Builds a playlist summary with only the fields that matter here varying. */
function playlistSummary(id: string, name: string, orderKey: string): MusicPlaylistSummary {
  return {
    id, name, icon: "music", shuffleEnabled: true, mixEnabled: false, repeatMode: "off", intendedUses: [],
    orderKey, totalCount: 0, eligibleCount: 0, unavailableCount: 0, snoozedCount: 0, localCount: 0,
    onlineCount: 0, version: 1,
  };
}

const summaries = [playlistSummary("playlist-a", "Alpha", "a"), playlistSummary("playlist-b", "Beta", "b")];

/** Returns an empty item window for any request. */
function emptyWindow(): MusicItemWindow {
  return { items: [], groups: [], totalCount: 0, offset: 0, limit: 100 };
}

vi.mock("$lib/api/music-library", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/music-library")>(),
  getMusicItemWindow: (request: MusicItemWindowRequest): Promise<MusicItemWindow> => {
    const playlistId = request.playlistId;
    if (request.destination === "playlist" && playlistId && harness.holdPlaylistIds.has(playlistId)) {
      return new Promise((resolve) => harness.heldPlaylistWindows.set(playlistId, () => resolve(emptyWindow())));
    }
    return Promise.resolve(emptyWindow());
  },
  getMusicPlaylistSummaries: () => Promise.resolve(summaries),
  getMusicSourceSummaries: () => Promise.resolve([]),
  getMusicIssues: () => Promise.resolve([]),
  getMusicPlaylist: (playlistId: string): Promise<MusicPlaylist> => {
    const summary = summaries.find((entry) => entry.id === playlistId);
    if (!summary) return Promise.reject(new Error("missing playlist"));
    return Promise.resolve({
      id: summary.id, name: summary.name, icon: summary.icon, shuffleEnabled: summary.shuffleEnabled,
      mixEnabled: summary.mixEnabled, repeatMode: summary.repeatMode, intendedUses: summary.intendedUses,
      orderKey: summary.orderKey, createdAtMs: 0, updatedAtMs: 0, version: summary.version,
    });
  },
}));

vi.mock("$lib/music/playlists/controller.svelte", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/music/playlists/controller.svelte")>();
  return {
    ...actual,
    createMusicPlaylistController: (
      ...args: Parameters<typeof actual.createMusicPlaylistController>
    ): MusicPlaylistController => {
      const controller = actual.createMusicPlaylistController(...args);
      harness.controllers.push(controller);
      return controller;
    },
  };
});

vi.mock("$lib/vault/active-vault", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/vault/active-vault")>(),
  requireActiveVaultIdentity: () => "vault-1",
  onActiveVaultIdentityChange: () => () => undefined,
}));

class ResizeObserverStub {
  observe(): void {}
  disconnect(): void {}
}

/** Lets pending promise chains settle and the page rerender. */
async function settle(): Promise<void> {
  for (let index = 0; index < 10; index += 1) await new Promise((resolve) => setTimeout(resolve, 0));
  await tick();
}

describe("MusicPlaylistBuilderPage navigation", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    harness.controllers.length = 0;
    harness.heldPlaylistWindows.clear();
    harness.holdPlaylistIds.clear();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  /** Clicks the playlist row with the given name in the context panel. */
  function clickPlaylistRow(name: string): void {
    const row = [...(target?.querySelectorAll<HTMLButtonElement>("button.context-row") ?? [])]
      .find((button) => button.textContent?.includes(name));
    expect(row).toBeDefined();
    row?.click();
  }

  it("keeps a slower earlier playlist open from replacing the playlist shown after it", async () => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
    const sources = getMusicSourcesController();
    vi.spyOn(sources, "load").mockResolvedValue(true);
    target = document.createElement("div");
    document.body.append(target);
    const { default: MusicPlaylistBuilderPage } = await import("./MusicPlaylistBuilderPage.svelte");
    component = mount(MusicPlaylistBuilderPage, { target, props: { onOpenPlayer: () => undefined } });
    await settle();
    target.querySelector<HTMLButtonElement>('button[aria-label^="Playlists"]')?.click();
    await settle();

    clickPlaylistRow("Beta");
    await settle();
    const playlist = harness.controllers[0];
    expect(playlist?.detail?.id).toBe("playlist-b");

    harness.holdPlaylistIds.add("playlist-a");
    clickPlaylistRow("Alpha");
    await settle();
    clickPlaylistRow("Beta");
    await settle();
    harness.heldPlaylistWindows.get("playlist-a")?.();
    await settle();

    expect(playlist?.detail?.id).toBe("playlist-b");
  }, 15_000);
});
