import { describe, expect, it, vi } from "vitest";

const { musicArtworkDataUrl, musicEmbeddedArtworkDataUrl, musicYouTubeThumbnailDataUrl } = vi.hoisted(() => ({
  musicArtworkDataUrl: vi.fn(async (path: string) => `file:${path}`),
  musicEmbeddedArtworkDataUrl: vi.fn(async (path: string) => `embedded:${path}`),
  musicYouTubeThumbnailDataUrl: vi.fn(async (videoId: string) => `youtube:${videoId}`),
}));

vi.mock("$lib/music/library/artwork-cache", () => ({
  musicArtworkDataUrl,
  musicEmbeddedArtworkDataUrl,
  musicYouTubeThumbnailDataUrl,
}));

import {
  musicReviewArtworkDataUrl,
  musicReviewDurationMs,
  musicReviewSource,
  nextMusicWeight,
  parseMusicReviewAutoplay,
  sortReviewPlaylists,
} from "./review";
import type { MusicInspectorDetail, MusicPlaylistSummary } from "$lib/music/library/contracts";

const playlist = (id: string, name: string): MusicPlaylistSummary => ({
  id, sortOrder: 0, name, icon: "lucide:list-music", shuffleEnabled: true, mixEnabled: false, repeatMode: "all", intendedUses: [], totalCount: 0,
  eligibleCount: 0, unavailableCount: 0, snoozedCount: 0, localCount: 0, onlineCount: 0, version: 1,
});

function detail(sourceKind: "local-file" | "youtube-video"): MusicInspectorDetail {
  return {
    item: {
      id: "item", identityKey: "item", sourceKind, mediaKind: "audio",
      youtubeVideoId: sourceKind === "youtube-video" ? "abc12345" : null,
      originalTitle: "Title", originalArtist: "Artist", originalAlbum: "Album",
      originalTrackNumber: null, originalArtworkIdentity: null, youtubeResolutionState: null,
      titleOverride: null, artistOverride: null, albumOverride: null, artworkOverride: null,
      durationMs: 1000, availability: "available", reviewState: "unreviewed",
      reviewChangedAtMs: null, reviewDeferredUntilMs: null, discoveredAtMs: 1, updatedAtMs: 1, version: 1,
    },
    locations: sourceKind === "local-file" ? [{
      id: "location", itemId: "item", rootId: "root", relativePath: "album/song.flac",
      fileSizeBytes: 1, modifiedAtMs: 1, lightweightFingerprint: "a", strongFingerprint: "b",
      availability: "available", lastSeenGeneration: 1, firstSeenAtMs: 1, updatedAtMs: 1,
    }] : [],
    memberships: [], membershipSkipRanges: [], snoozes: [], signals: [], statistics: null, sourceCollectionIds: [],
  };
}

describe("music review helpers", () => {
  it("uses the duration learned on first playback before a stale Review item refreshes", () => {
    expect(musicReviewDurationMs(null, 96_000, null)).toBe(96_000);
    expect(musicReviewDurationMs(null, 96_000, 94_000)).toBe(96_000);
    expect(musicReviewDurationMs(97_000, 96_000, 94_000)).toBe(97_000);
    expect(musicReviewDurationMs(null, null, 94_000)).toBe(94_000);
    expect(musicReviewDurationMs(null, null, null)).toBe(0);
  });

  it("defaults unsafe stored autoplay values", () => {
    expect(parseMusicReviewAutoplay("true")).toBe(false);
  });

  it("resolves local and YouTube canonical items into playable sources", () => {
    expect(musicReviewSource(detail("local-file"), [{ rootId: "root", folderPath: "/Music", status: "available" }]))
      .toMatchObject({ kind: "local-file", path: "/Music/album/song.flac" });
    expect(musicReviewSource(detail("youtube-video"), []))
      .toMatchObject({ kind: "youtube-video", videoId: "abc12345" });
  });

  it("resolves scanner-detected sidecar artwork for review playback", () => {
    const localDetail = detail("local-file");
    localDetail.item.originalArtworkIdentity = "sidecar:album/cover.jpg";

    expect(musicReviewSource(localDetail, [{ rootId: "root", folderPath: "/Music", status: "available" }]))
      .toMatchObject({
        kind: "local-file",
        path: "/Music/album/song.flac",
        artworkPath: "/Music/album/cover.jpg",
      });
  });

  it("shows cached YouTube artwork in Review while respecting a chosen override", async () => {
    const youtubeDetail = detail("youtube-video");
    youtubeDetail.item.youtubeVideoId = "abcDEF_1234";

    expect(await musicReviewArtworkDataUrl(youtubeDetail, [])).toBe("youtube:abcDEF_1234");
    expect(musicYouTubeThumbnailDataUrl).toHaveBeenCalledWith("abcDEF_1234");

    youtubeDetail.item.artworkOverride = "/Music/custom.jpg";
    expect(await musicReviewArtworkDataUrl(youtubeDetail, [])).toBe("file:/Music/custom.jpg");
    expect(musicArtworkDataUrl).toHaveBeenCalledWith("/Music/custom.jpg");
  });

  it("keeps playlist ordering independent from selection while filtering", () => {
    expect(sortReviewPlaylists([playlist("a", "Work"), playlist("b", "Reading")], "")
      .map((entry) => entry.id)).toEqual(["b", "a"]);
    expect(sortReviewPlaylists([playlist("a", "Work"), playlist("b", "Reading")], "read")
      .map((entry) => entry.id)).toEqual(["b"]);
  });

  it("cycles through all probability weights", () => {
    expect(nextMusicWeight("much-more-often")).toBe("rarely");
  });
});
