// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  applyMusicReviewSelection: vi.fn(),
  bulkEditMusicMemberships: vi.fn(),
  bulkSetMusicReviewState: vi.fn(),
  bulkSnoozeMusicItems: vi.fn(),
  createMusicPlaylist: vi.fn(),
  getMusicInspectorDetail: vi.fn(),
  getMusicIssues: vi.fn(),
  getMusicItemWindow: vi.fn(),
  getMusicMembershipMatrix: vi.fn(),
  getMusicPlaylistSummaries: vi.fn(),
  getMusicSourceSummaries: vi.fn(),
  removeMusicMemberships: vi.fn(),
  setMusicItemSignals: vi.fn(),
  setMusicReviewState: vi.fn(),
  upsertMusicMemberships: vi.fn(),
}));
vi.mock("$lib/api/music-library", () => api);

const artwork = vi.hoisted(() => ({
  musicReviewArtworkDataUrl: vi.fn<(detail: { item: { id: string } }) => Promise<string | null>>(),
}));
vi.mock("$lib/music/review", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/music/review")>(),
  musicReviewArtworkDataUrl: artwork.musicReviewArtworkDataUrl,
}));

import { MusicBulkEditController } from "$lib/music/builder/bulk-edit-controller.svelte";
import { MusicBuilderInspectorController } from "$lib/music/builder/inspector.svelte";
import { createMusicReviewWorkspaceViewState } from "$lib/music/builder/view-state";
import type { MusicInspectorDetail, MusicItemListEntry, MusicReviewState } from "$lib/music/library/contracts";
import { MusicLibraryController, type MusicLibraryControllerApi } from "$lib/music/library/controller.svelte";
import type { MusicReviewAuditionController } from "$lib/music/review/audition.svelte";
import { MusicReviewController } from "$lib/music/review/controller.svelte";
import { buildMusicReviewTree } from "$lib/music/review/tree";
import type { MusicSourcesController } from "$lib/music/sources/controller.svelte";
import MusicReviewWorkspace from "./MusicReviewWorkspace.svelte";

const emptyLibraryApi: MusicLibraryControllerApi = {
  itemWindow: vi.fn(async () => ({ items: [], groups: [], totalCount: 0, offset: 0, limit: 50 })),
  playlistSummaries: vi.fn(async () => []),
  sourceSummaries: vi.fn(async () => []),
  issues: vi.fn(async () => []),
};

function listItem(id: string, reviewState: MusicReviewState): MusicItemListEntry {
  return {
    id, identityKey: `local:${id}`, sourceKind: "local-file", mediaKind: "audio",
    title: id, artist: "Artist", album: "Album", localRootId: "root",
    relativePath: `${id}.flac`, sourceCollectionIds: ["source"], originalArtworkIdentity: null, artworkOverride: null,
    youtubeResolutionState: null, durationMs: 1_000, availability: "available", reviewState,
    discoveredAtMs: 1, updatedAtMs: 1, version: 1, playlistCount: 0, activeSnoozeCount: 0, lastPlayedAtMs: null, playCount: 0,
    membershipId: null, membershipOrderKey: null, membershipWeight: null, membershipEnabled: null, membershipVersion: null,
  };
}

function detail(entry: MusicItemListEntry): MusicInspectorDetail {
  return {
    item: {
      id: entry.id, identityKey: entry.identityKey, sourceKind: entry.sourceKind, mediaKind: entry.mediaKind,
      youtubeVideoId: null, originalTitle: entry.title, originalArtist: entry.artist, originalAlbum: entry.album,
      originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
      titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
      durationMs: entry.durationMs, availability: entry.availability, reviewState: entry.reviewState,
      reviewChangedAtMs: null, reviewDeferredUntilMs: null, discoveredAtMs: 1, updatedAtMs: 1, version: 1,
    },
    locations: [], memberships: [], membershipSkipRanges: [], snoozes: [], signals: [], statistics: null, sourceCollectionIds: [],
  };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((onResolve) => { resolve = onResolve; });
  return { promise, resolve };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 6; index += 1) {
    await Promise.resolve();
    await tick();
  }
}

/** The smallest audition surface the single-item Review paths read, with playback idle. */
function idleAudition(): MusicReviewAuditionController {
  const player = {
    claimSurface: () => () => undefined,
    isPlaying: false,
    currentSource: null,
    localHasVideo: false,
    currentArtworkUrl: null,
    youtubePlaybackStarting: false,
    youtubeKnownDurations: {},
    progressMax: 0,
    progressValue: 0,
    snapshot: { durationMs: null, positionMs: 0, rate: 1 },
  };
  return {
    musicPlayer: player,
    ownsPlayback: false,
    reviewItemId: null,
    errorFor: () => null,
    preview: vi.fn(async () => false),
    dismissError: vi.fn(),
  } as unknown as MusicReviewAuditionController;
}

describe("MusicReviewWorkspace preparing the next item", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
  });

  /** Mounts Review on the middle of three tracks with the last track's artwork load held open. */
  async function mountOnMiddleItem(middleState: MusicReviewState) {
    const unordered = [listItem("a", "unreviewed"), listItem("b", middleState), listItem("c", "unreviewed")];
    const order = buildMusicReviewTree(unordered).flatMap((node) => node.itemIds);
    const items = order.map((itemId) => unordered.find((entry) => entry.id === itemId)!);
    const [previousId, currentId, nextId] = order as [string, string, string];
    const nextArtwork = deferred<string | null>();
    artwork.musicReviewArtworkDataUrl.mockImplementation(async (entry) =>
      entry.item.id === nextId ? nextArtwork.promise : null);

    const library = new MusicLibraryController(emptyLibraryApi, () => 2);
    library.windows = { review: { items, groups: [], totalCount: items.length, offset: 0, limit: 50 } };
    library.selectItem(currentId);
    const inspector = new MusicBuilderInspectorController({
      detail: async (itemId) => detail(items.find((entry) => entry.id === itemId)!),
    });
    const sources = { bindings: [] } as unknown as MusicSourcesController;

    target = document.createElement("div");
    document.body.append(target);
    component = mount(MusicReviewWorkspace, {
      target,
      props: {
        items,
        totalCount: items.length,
        library,
        inspector,
        sources,
        audition: idleAudition(),
        review: new MusicReviewController(library, inspector, () => 2, () => "membership"),
        bulk: new MusicBulkEditController(library, () => 2, () => "membership"),
        selectedItemIds: [],
        selectedFolderIds: [],
        onClearSelection: vi.fn(),
        autoplay: false,
        onAutoplayChange: vi.fn(),
        onOpenPlayer: vi.fn(),
        onEditPlaylist: vi.fn(),
        onDeletePlaylist: vi.fn(),
        onReorderPlaylists: vi.fn(async () => true),
        viewState: createMusicReviewWorkspaceViewState(),
      },
    });
    await settle();
    expect(inspector.detail?.item.id).toBe(currentId);
    return { library, inspector, previousId, nextId, nextArtwork };
  }

  function actionButton(label: string): HTMLButtonElement {
    const button = [...(target?.querySelectorAll<HTMLButtonElement>(".review-action") ?? [])]
      .find((candidate) => candidate.textContent?.trim() === label);
    if (!button) throw new Error(`The ${label} action is rendered`);
    return button;
  }

  function selectPreviousWithKeyboard(): void {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", ctrlKey: true, bubbles: true }));
  }

  it("keeps a selection made while Review later waits for the next item's artwork", async () => {
    const { library, inspector, previousId, nextId, nextArtwork } = await mountOnMiddleItem("unreviewed");

    actionButton("Review later").click();
    await settle();
    expect(artwork.musicReviewArtworkDataUrl).toHaveBeenCalledWith(
      expect.objectContaining({ item: expect.objectContaining({ id: nextId }) }),
      expect.anything(),
    );
    selectPreviousWithKeyboard();
    await settle();
    expect(library.selectedItem?.id).toBe(previousId);

    nextArtwork.resolve(null);
    await settle();

    expect(library.selectedItem?.id).toBe(previousId);
    expect(inspector.detail?.item.id).toBe(previousId);
  });

  it("keeps a selection made while Continue waits for the next item's artwork", async () => {
    const { library, inspector, previousId, nextId, nextArtwork } = await mountOnMiddleItem("reviewed");

    actionButton("Continue").click();
    await settle();
    expect(artwork.musicReviewArtworkDataUrl).toHaveBeenCalledWith(
      expect.objectContaining({ item: expect.objectContaining({ id: nextId }) }),
      expect.anything(),
    );
    selectPreviousWithKeyboard();
    await settle();
    expect(library.selectedItem?.id).toBe(previousId);

    nextArtwork.resolve(null);
    await settle();

    expect(library.selectedItem?.id).toBe(previousId);
    expect(inspector.detail?.item.id).toBe(previousId);
  });
});
