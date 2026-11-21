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

describe("MusicReviewAuditionController interleavings", () => {
  const bindings = [{ rootId: "root", folderPath: "/Music", status: "available" as const }];

  beforeEach(() => {
    player.contextOwner = "manual";
    player.snapshot.positionMs = 0;
    vi.clearAllMocks();
  });

  it("is inactive and owns nothing before the first preview", () => {
    const audition = new MusicReviewAuditionController();
    expect(audition.active).toBe(false);
    expect(audition.reviewItemId).toBeNull();
    expect(audition.ownsPlayback).toBe(false);
  });

  it("is active while playback suspends and owns playback once the item loads", async () => {
    let finishSuspend!: (value: MusicReviewPlaybackCheckpoint) => void;
    player.suspendForReview.mockImplementationOnce(() => new Promise((resolve) => { finishSuspend = resolve; }));
    const audition = new MusicReviewAuditionController();

    const previewing = audition.preview(detail(), bindings, false);
    expect(audition.active).toBe(true);
    expect(audition.reviewItemId).toBeNull();

    finishSuspend(checkpoint);
    expect(await previewing).toBe(true);
    expect(audition.reviewItemId).toBe("review-item");
    expect(audition.ownsPlayback).toBe(true);
    expect(player.loadReviewItem).toHaveBeenCalledWith("review-item", false);
  });

  it("restores a checkpoint that arrives after Review was discarded and reports the preview as stale", async () => {
    let finishSuspend!: (value: MusicReviewPlaybackCheckpoint) => void;
    player.suspendForReview.mockImplementationOnce(() => new Promise((resolve) => { finishSuspend = resolve; }));
    const audition = new MusicReviewAuditionController();

    const previewing = audition.preview(detail(), bindings, true);
    audition.discard();
    finishSuspend(checkpoint);

    expect(await previewing).toBe(false);
    expect(player.restoreAfterReview).toHaveBeenCalledWith(checkpoint);
    expect(player.loadReviewItem).not.toHaveBeenCalled();
    expect(audition.active).toBe(false);
  });

  it("ends the audition when playback cannot be suspended", async () => {
    player.suspendForReview.mockRejectedValueOnce(new Error("busy"));
    const audition = new MusicReviewAuditionController();

    expect(await audition.preview(detail(), bindings, true)).toBe(false);
    expect(audition.active).toBe(false);
    expect(player.loadReviewItem).not.toHaveBeenCalled();
  });

  it("keeps the suspended checkpoint when an item fails to load, so closing Review still restores it", async () => {
    player.loadReviewItem.mockRejectedValueOnce(new Error("missing file"));
    const audition = new MusicReviewAuditionController();

    expect(await audition.preview(detail(), bindings, true)).toBe(false);
    expect(audition.active).toBe(true);
    expect(audition.reviewItemId).toBeNull();
    expect(audition.error).not.toBeNull();

    await audition.restore();
    expect(player.restoreAfterReview).toHaveBeenCalledWith(checkpoint);
  });

  it("suspends only once across previews of different items", async () => {
    const audition = new MusicReviewAuditionController();
    await audition.preview(detail(), bindings, true);
    const other = detail();
    other.item = { ...other.item, id: "other-item" };

    expect(await audition.preview(other, bindings, true)).toBe(true);
    expect(player.suspendForReview).toHaveBeenCalledOnce();
    expect(audition.reviewItemId).toBe("other-item");
  });

  it("hands playback to an automation boundary and resumes the item where it stopped on the next preview", async () => {
    const audition = new MusicReviewAuditionController();
    await audition.preview(detail(), bindings, true);
    player.contextOwner = "review";
    player.snapshot.positionMs = 42_000;

    audition.supersedeForBoundary("pomodoro");

    expect(player.contextOwner).toBe("pomodoro");
    expect(audition.active).toBe(false);
    expect(player.restoreAfterReview).not.toHaveBeenCalled();

    await audition.preview(detail(), bindings, true);
    expect(player.seekToMs).toHaveBeenCalledWith(42_000);
  });

  it("restores nothing when Review closes before any playback was suspended", async () => {
    const audition = new MusicReviewAuditionController();
    await audition.restore();
    expect(player.restoreAfterReview).not.toHaveBeenCalled();
    expect(audition.active).toBe(false);
  });
});
