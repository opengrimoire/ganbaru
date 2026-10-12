import { describe, expect, it } from "vitest";
import {
  parseBindingResult,
  parseInspectorDetail,
  parseIssues,
  parseItemWindow,
  parsePlaylistSummaries,
  parseRefreshJobProgress,
  parseRelinkPlanSummary,
  parseRelinkPlanWindow,
  parseSourceRemovalImpact,
  parseSourceSummaries,
  parseYouTubeSnapshotResult,
  parseWriteReceipt,
} from "./contracts";
import { normalizeMusicLibraryError } from "$lib/api/music-library";

const item = {
  id: "item-1",
  identityKey: "local:item-1",
  sourceKind: "local-file",
  mediaKind: "audio",
  title: "Focus",
  artist: "Composer",
  album: "Soundtrack",
  localRootId: "root-1",
  relativePath: "Games/Nier/Focus.flac",
  sourceCollectionIds: ["source-1"],
  originalArtworkIdentity: "sidecar:Games/Nier/cover.jpg",
  artworkOverride: null,
  durationMs: 120_000,
  availability: "available",
  reviewState: "unreviewed",
  discoveredAtMs: 1,
  updatedAtMs: 1,
  version: 1,
  playlistCount: 0,
  activeSnoozeCount: 0,
  lastPlayedAtMs: null,
  playCount: 0,
  membershipId: null,
  membershipPosition: null,
  membershipWeight: null,
  membershipEnabled: null,
  membershipVersion: null,
};

describe("music library contracts", () => {
  it("accepts a complete bounded item window", () => {
    expect(parseItemWindow({
      items: [item],
      groups: [{ key: "local-file", count: 1 }],
      totalCount: 1,
      offset: 0,
      limit: 50,
    }).items[0]).toMatchObject({ title: "Focus", relativePath: "Games/Nier/Focus.flac" });
  });

  it("rejects malformed enums and numeric fields", () => {
    expect(() => parseItemWindow({
      items: [{ ...item, sourceKind: "streaming-service" }],
      groups: [], totalCount: 1, offset: 0, limit: 50,
    })).toThrow("sourceKind is not supported");
    expect(() => parseWriteReceipt({ id: "playlist-1", version: "1" })).toThrow("version must be a safe integer");
  });

  it("rejects partially shaped summaries instead of applying defaults", () => {
    expect(() => parsePlaylistSummaries([{
      id: "playlist-1",
      name: "Focus",
      icon: "lucide:list-music",
      shuffleEnabled: false,
      mixEnabled: false,
      repeatMode: "all",
      intendedUses: [],
      sortOrder: 0,
      totalCount: 1,
    }])).toThrow("eligibleCount must be a safe integer");
  });

  it("validates nested inspector rows", () => {
    expect(() => parseInspectorDetail({
      item: {
        id: "item-1", identityKey: "local:item-1", sourceKind: "local-file", mediaKind: "audio",
        youtubeVideoId: null, originalTitle: "Focus", originalArtist: "", originalAlbum: "",
        originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
        titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
        durationMs: null, availability: "available", reviewState: "reviewed", reviewChangedAtMs: null, reviewDeferredUntilMs: null,
        discoveredAtMs: 1, updatedAtMs: 1, version: 1,
      },
      locations: [], memberships: [], membershipSkipRanges: [], snoozes: [], signals: ["invented"],
      statistics: null, sourceCollectionIds: [],
    })).toThrow("signals[0] is not supported");
  });

  it("validates device binding state", () => {
    expect(parseBindingResult({ rootId: "root-1", folderPath: null, status: "needs-relink" }))
      .toEqual({ rootId: "root-1", folderPath: null, status: "needs-relink" });
    expect(() => parseBindingResult({ rootId: "root-1", folderPath: null, status: "lost" }))
      .toThrow("status is not supported");
  });

  it("validates persistent refresh progress without inventing counters", () => {
    const progress = parseRefreshJobProgress({
      jobId: "refresh-1", collectionId: "collection-1", rootId: "root-1",
      kind: "local-root", state: "running", generation: 2,
      discoveredCount: 6000, processedCount: 128, skippedCount: 3,
      issueCount: 1, truncatedCount: 0, absenceDetermined: false,
      statusMessage: "Cataloging discovered media.", requestedAtMs: 1,
      startedAtMs: 2, finishedAtMs: null, updatedAtMs: 3,
    });
    expect(progress.processedCount).toBe(128);
    expect(() => parseRefreshJobProgress({ ...progress, state: "stuck" }))
      .toThrow("state is not supported");
  });

  it("validates YouTube snapshot counters", () => {
    expect(parseYouTubeSnapshotResult({
      collectionId: "youtube-1",
      canonicalItemCount: 3,
      newlyDiscoveredCount: 2,
      repeatedVideoCount: 1,
      generation: 4,
    })).toMatchObject({ canonicalItemCount: 3, generation: 4 });
    expect(() => parseYouTubeSnapshotResult({
      collectionId: "youtube-1",
      canonicalItemCount: "3",
      newlyDiscoveredCount: 2,
      repeatedVideoCount: 1,
      generation: 4,
    })).toThrow("canonicalItemCount must be a safe integer");
  });

  it("validates actionable source health and repair context", () => {
    const source = parseSourceSummaries([{
      id: "source-1", kind: "local-root", name: "Soundtracks", refreshState: "partial",
      lastSuccessfulRefreshAtMs: 10, localRootId: "root-1", youtubePlaylistId: null,
      itemCount: 5, missingCount: 1, newCount: 2, unreviewedCount: 3,
      unavailableCount: 0, ambiguousCount: 1, openIssueCount: 2,
      health: "issues", discoveryEnabled: true, version: 4,
    }])[0];
    expect(source).toMatchObject({ health: "issues", localRootId: "root-1", unreviewedCount: 3 });
    const issue = parseIssues([{
      id: "issue-1", issueKind: "relink-ambiguous", itemId: null, playlistId: null,
      collectionId: "source-1", rootId: "root-1", relativePath: "Album/Track.mp3",
      actionRequired: true, message: "Choose a match.", createdAtMs: 10,
    }])[0];
    expect(issue).toMatchObject({ collectionId: "source-1", actionRequired: true });
  });

  it("validates relink windows and source removal impacts", () => {
    expect(parseRelinkPlanSummary({
      id: "plan-1", rootId: "root-1", state: "ready", exactCount: 1,
      likelyCount: 2, ambiguousCount: 1, missingCount: 1, newCount: 3,
      createdAtMs: 10, updatedAtMs: 11,
    }).state).toBe("ready");
    expect(parseRelinkPlanWindow({
      entries: [{
        id: "entry-1", matchKind: "ambiguous", oldLocationId: null,
        suggestedItemId: null, candidateRelativePath: "Track.mp3",
        candidateItemIds: ["item-1", "item-2"], fileSizeBytes: 100,
        resolvedItemId: null, resolvedAtMs: null,
      }],
      totalCount: 1, offset: 0, limit: 20,
    }).entries[0]?.candidateItemIds).toHaveLength(2);
    expect(parseSourceRemovalImpact({
      collectionId: "source-1", itemCount: 3, membershipCount: 1,
      sharedItemCount: 1, orphanedItemCount: 1, activeRefreshCount: 0,
    }).orphanedItemCount).toBe(1);
  });

  it("normalizes structured backend errors without trusting arbitrary fields", () => {
    const structured = normalizeMusicLibraryError({
      code: "stale-write",
      message: "Playlist changed",
      field: "expectedVersion",
    });
    expect(structured).toMatchObject({
      code: "stale-write",
      message: "Playlist changed",
      field: "expectedVersion",
    });
    expect(normalizeMusicLibraryError({ code: "root", message: 42 }).code).toBe("unknown");
  });
});
