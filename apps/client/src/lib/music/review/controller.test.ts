import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  createMusicPlaylist: vi.fn(),
  getMusicInspectorDetail: vi.fn(),
  getMusicIssues: vi.fn(),
  getMusicItemWindow: vi.fn(),
  getMusicPlaylistSummaries: vi.fn(),
  getMusicSourceSummaries: vi.fn(),
  removeMusicMemberships: vi.fn(),
  setMusicReviewState: vi.fn(),
  upsertMusicMemberships: vi.fn(),
}));

vi.mock("$lib/api/music-library", () => api);

import { MusicBuilderInspectorController } from "$lib/music/builder/inspector.svelte";
import { MusicLibraryController, type MusicLibraryControllerApi } from "$lib/music/library/controller.svelte";
import { MusicReviewController } from "./controller.svelte";
import type { MusicInspectorDetail, MusicItemListEntry, MusicPlaylistSummary } from "$lib/music/library/contracts";

const emptyLibraryApi: MusicLibraryControllerApi = {
  itemWindow: vi.fn(async () => ({ items: [], groups: [], totalCount: 0, offset: 0, limit: 50 })),
  playlistSummaries: vi.fn(async () => []),
  sourceSummaries: vi.fn(async () => []),
  issues: vi.fn(async () => []),
};

const playlist = (): MusicPlaylistSummary => ({
  id: "playlist", sortOrder: 0, name: "Focus", icon: "lucide:laptop", shuffleEnabled: true, mixEnabled: false, repeatMode: "all",
  intendedUses: ["focus"], totalCount: 0, eligibleCount: 0, unavailableCount: 0,
  snoozedCount: 0, localCount: 0, onlineCount: 0, version: 1,
});

const detail = (): MusicInspectorDetail => ({
  item: {
    id: "item", identityKey: "local:item", sourceKind: "local-file", mediaKind: "audio",
    youtubeVideoId: null, originalTitle: "Track", originalArtist: "Artist", originalAlbum: "Album",
    originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
    titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
    durationMs: 1_000, availability: "available", reviewState: "unreviewed",
    reviewChangedAtMs: null, reviewDeferredUntilMs: null, discoveredAtMs: 1, updatedAtMs: 1, version: 1,
  },
  locations: [], memberships: [], membershipSkipRanges: [], snoozes: [], signals: [], statistics: null, sourceCollectionIds: [],
});

const listItem = (): MusicItemListEntry => ({
  id: "item", identityKey: "local:item", sourceKind: "local-file", mediaKind: "audio",
  title: "Track", artist: "Artist", album: "Album", localRootId: "root",
  relativePath: "Track.flac", sourceCollectionIds: ["source"], originalArtworkIdentity: null, artworkOverride: null, durationMs: 1_000,
  availability: "available", reviewState: "unreviewed", discoveredAtMs: 1, updatedAtMs: 1, version: 1,
  playlistCount: 0, activeSnoozeCount: 0, lastPlayedAtMs: null, playCount: 0,
  membershipId: null, membershipPosition: null, membershipWeight: null,
  membershipEnabled: null, membershipVersion: null,
});

describe("MusicReviewController", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.upsertMusicMemberships.mockResolvedValue([{ id: "membership", version: 1 }]);
    api.removeMusicMemberships.mockResolvedValue(undefined);
    api.setMusicReviewState.mockResolvedValue({ id: "item", version: 2 });
  });

  it("persists optimistic membership changes and provides contextual Undo", async () => {
    const library = new MusicLibraryController(emptyLibraryApi, () => 2);
    const inspector = new MusicBuilderInspectorController();
    inspector.detail = detail();
    const summary = playlist();
    library.playlistSummaries = [summary];
    const review = new MusicReviewController(library, inspector, () => 2, () => "membership");

    await review.toggleMembership(summary);
    expect(inspector.detail.memberships).toHaveLength(1);
    expect(summary.totalCount).toBe(1);
    expect(api.upsertMusicMemberships).toHaveBeenCalledOnce();

    await library.undoLast();
    expect(inspector.detail.memberships).toHaveLength(0);
    expect(summary.totalCount).toBe(0);
    expect(api.removeMusicMemberships).toHaveBeenCalledWith({ membershipIds: ["membership"] });
  });

  it("serializes a rapid checked then unchecked intent for one playlist", async () => {
    const library = new MusicLibraryController(emptyLibraryApi, () => 2);
    const inspector = new MusicBuilderInspectorController();
    inspector.detail = detail();
    const summary = playlist();
    library.playlistSummaries = [summary];
    const review = new MusicReviewController(library, inspector, () => 2, () => "membership");

    const checked = review.toggleMembership(summary);
    const unchecked = review.toggleMembership(summary);
    await Promise.all([checked, unchecked]);

    expect(inspector.detail.memberships).toHaveLength(0);
    expect(summary.totalCount).toBe(0);
    expect(api.upsertMusicMemberships).toHaveBeenCalledOnce();
    expect(api.removeMusicMemberships).toHaveBeenCalledOnce();
  });

  it("keeps the requested next tree item selected while refreshing review state", async () => {
    const library = new MusicLibraryController(emptyLibraryApi, () => 2);
    const inspector = new MusicBuilderInspectorController();
    inspector.detail = detail();
    const review = new MusicReviewController(library, inspector, () => 2);

    expect(await review.changeReviewState("reviewed", null, "literal-next")).toBe(true);

    expect(library.currentState.selectedItemId).toBe("literal-next");
    expect(api.setMusicReviewState).toHaveBeenCalledWith(expect.objectContaining({ itemId: "item" }));
    expect(emptyLibraryApi.itemWindow).not.toHaveBeenCalled();
  });

  it("hides an ignored item optimistically without refreshing the Review window", async () => {
    let resolveReview!: (value: { id: string; version: number }) => void;
    api.setMusicReviewState.mockImplementationOnce(() => new Promise((resolve) => {
      resolveReview = resolve;
    }));
    const library = new MusicLibraryController(emptyLibraryApi, () => 2);
    library.setVault("vault-1");
    const item = listItem();
    library.windows.review = { items: [item], groups: [], totalCount: 1, offset: 0, limit: 50 };
    library.selectItem("item");
    const inspector = new MusicBuilderInspectorController();
    inspector.detail = detail();
    const review = new MusicReviewController(library, inspector, () => 2);

    const pending = review.changeReviewState("ignored", null, "next");

    expect(item.reviewState).toBe("ignored");
    expect(library.currentState.selectedItemId).toBe("next");
    expect(emptyLibraryApi.itemWindow).not.toHaveBeenCalled();
    resolveReview({ id: "item", version: 2 });
    expect(await pending).toBe(true);
    expect(item.version).toBe(2);
    expect(emptyLibraryApi.itemWindow).not.toHaveBeenCalled();
  });
});
