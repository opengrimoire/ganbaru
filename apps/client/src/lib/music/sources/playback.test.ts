import { describe, expect, it } from "vitest";
import type { MusicItemListEntry } from "$lib/music/library/contracts";
import { projectMusicSourceQueue } from "./playback";

function item(patch: Partial<MusicItemListEntry>): MusicItemListEntry {
  return {
    id: "track",
    identityKey: "local:track",
    sourceKind: "local-file",
    mediaKind: "audio",
    title: "Track",
    artist: "Artist",
    album: "Album",
    localRootId: "root",
    relativePath: "Album/Track.flac",
    sourceCollectionIds: [],
    originalArtworkIdentity: null,
    artworkOverride: null,
    durationMs: 60_000,
    availability: "available",
    reviewState: "reviewed",
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
    ...patch,
  };
}

describe("music source playback", () => {
  it("projects playable local and YouTube rows into one player queue", () => {
    const queue = projectMusicSourceQueue([
      item({ originalArtworkIdentity: "sidecar:Album/cover.jpg" }),
      item({
        id: "video",
        identityKey: "youtube:video:abc12345678",
        sourceKind: "youtube-video",
        localRootId: null,
        relativePath: null,
        title: "Video",
      }),
    ], [{ rootId: "root", folderPath: "/Music", status: "available" }]);

    expect(queue).toHaveLength(2);
    expect(queue[0]?.source).toMatchObject({ kind: "local-file", path: "/Music/Album/Track.flac", artworkPath: "/Music/Album/cover.jpg" });
    expect(queue[1]?.source).toMatchObject({ kind: "youtube-video", videoId: "abc12345678", title: "Video" });
    expect(queue[0]?.snoozed).toBe(false);
  });

  it("keeps source queue Snooze indicators for playable rows", () => {
    const queue = projectMusicSourceQueue([item({ activeSnoozeCount: 1 })], [
      { rootId: "root", folderPath: "/Music", status: "available" },
    ]);
    expect(queue[0]?.snoozed).toBe(true);
  });

  it("omits unavailable, unbound, and malformed source rows", () => {
    const queue = projectMusicSourceQueue([
      item({ availability: "missing" }),
      item({ id: "unbound", localRootId: "other" }),
      item({ id: "video", sourceKind: "youtube-video", localRootId: null, relativePath: null, identityKey: "invalid" }),
    ], [{ rootId: "root", folderPath: "/Music", status: "available" }]);

    expect(queue).toEqual([]);
  });

  it("lets unresolved YouTube rows resolve when playback starts", () => {
    const queue = projectMusicSourceQueue([
      item({
        id: "video",
        identityKey: "youtube:video:abc12345678",
        sourceKind: "youtube-video",
        localRootId: null,
        relativePath: null,
        availability: "unknown",
      }),
    ], []);

    expect(queue).toHaveLength(1);
    expect(queue[0]?.source).toMatchObject({ kind: "youtube-video", videoId: "abc12345678" });
  });
});
