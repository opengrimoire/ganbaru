import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicInspectorDetail } from "$lib/music/library/contracts";
import type { MusicReviewPlaybackCheckpoint } from "$lib/stores/music-player.svelte";

const checkpoint = vi.hoisted((): MusicReviewPlaybackCheckpoint => ({ checkpointId: "playlist-checkpoint" }));
const player = vi.hoisted(() => ({
  contextOwner: "manual" as "manual" | "review" | "calendar-event" | "pomodoro",
  snapshot: { positionMs: 0 },
  suspendForReview: vi.fn(async () => checkpoint),
  restoreAfterReview: vi.fn(async (_checkpoint: MusicReviewPlaybackCheckpoint): Promise<void> => undefined),
  loadReviewItem: vi.fn(async () => undefined),
  seekToMs: vi.fn(async () => undefined),
}));

vi.mock("$lib/stores/music-player.svelte", () => ({
  getMusicPlayer: () => player,
}));

import { MusicReviewAuditionController } from "./audition.svelte";

function detail(): MusicInspectorDetail {
  return {
    item: {
      id: "review-item", identityKey: "review-item", sourceKind: "local-file", mediaKind: "audio",
      youtubeVideoId: null, originalTitle: "Review track", originalArtist: "Artist", originalAlbum: "Album",
      originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
      titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
      durationMs: 1000, availability: "available", reviewState: "unreviewed",
      reviewChangedAtMs: null, reviewDeferredUntilMs: null, discoveredAtMs: 1, updatedAtMs: 1, version: 1,
    },
    locations: [{
      id: "location", itemId: "review-item", rootId: "root", relativePath: "review.flac",
      fileSizeBytes: 1, modifiedAtMs: 1, lightweightFingerprint: "a", strongFingerprint: "b",
      availability: "available", lastSeenGeneration: 1, firstSeenAtMs: 1, updatedAtMs: 1,
    }],
    memberships: [], membershipSkipRanges: [], snoozes: [], signals: [], statistics: null, sourceCollectionIds: [],
  };
}

describe("MusicReviewAuditionController", () => {
  beforeEach(() => {
    player.contextOwner = "manual";
    player.snapshot.positionMs = 0;
    vi.clearAllMocks();
  });

  it("suspends current playback once and restores it when Review closes", async () => {
    const audition = new MusicReviewAuditionController();
    const bindings = [{ rootId: "root", folderPath: "/Music", status: "available" as const }];

    expect(await audition.preview(detail(), bindings, true)).toBe(true);
    expect(player.suspendForReview).toHaveBeenCalledOnce();
    expect(player.loadReviewItem).toHaveBeenCalledWith("review-item", true);

    await audition.restore();

    expect(player.restoreAfterReview).toHaveBeenCalledWith(checkpoint);
    expect(audition.active).toBe(false);
  });

  it("discards suspended playback when another playlist takes ownership", async () => {
    const audition = new MusicReviewAuditionController();
    await audition.preview(detail(), [{ rootId: "root", folderPath: "/Music", status: "available" }], true);

    audition.discard();

    expect(player.restoreAfterReview).not.toHaveBeenCalled();
    expect(audition.active).toBe(false);
  });

  it("waits for restoration before starting another temporary preview", async () => {
    let finishRestore!: () => void;
    player.restoreAfterReview.mockImplementationOnce(() => new Promise<void>((resolve) => {
      finishRestore = resolve;
    }));
    const audition = new MusicReviewAuditionController();
    const bindings = [{ rootId: "root", folderPath: "/Music", status: "available" as const }];
    await audition.preview(detail(), bindings, true);

    const restoring = audition.restore();
    const reopening = audition.preview(detail(), bindings, true);
    await Promise.resolve();
    expect(player.suspendForReview).toHaveBeenCalledOnce();

    finishRestore();
    await restoring;
    await reopening;

    expect(player.suspendForReview).toHaveBeenCalledTimes(2);
  });
});
