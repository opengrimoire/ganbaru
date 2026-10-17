import { describe, expect, it } from "vitest";
import type { MusicItemListEntry } from "$lib/music/library/contracts";
import { musicListArtworkSource } from "./list-artwork";

function item(patch: Partial<MusicItemListEntry> = {}): MusicItemListEntry {
  return {
    id: "track-1", identityKey: "local:track-1", sourceKind: "local-file", mediaKind: "audio",
    title: "Track", artist: "Artist", album: "Album", localRootId: "root-1", relativePath: "album/track.flac",
    originalArtworkIdentity: null, artworkOverride: null, durationMs: null, availability: "available",
    reviewState: "reviewed", discoveredAtMs: 1, updatedAtMs: 1, version: 1, playlistCount: 1,
    activeSnoozeCount: 0, lastPlayedAtMs: null, playCount: 0, membershipId: "membership-1",
    membershipPosition: 0, membershipWeight: "normal", membershipEnabled: true, membershipVersion: 1,
    ...patch,
    sourceCollectionIds: patch.sourceCollectionIds ?? [],
  };
}

const bindings = [{ rootId: "root-1", folderPath: "/Music", status: "available" as const }];

describe("music list artwork", () => {
  it("prefers an explicit artwork override", () => {
    expect(musicListArtworkSource(item({ artworkOverride: "/custom/cover.png" }), bindings)).toEqual({
      kind: "file",
      path: "/custom/cover.png",
    });
  });

  it("resolves scanner-detected sidecar artwork", () => {
    expect(musicListArtworkSource(item({ originalArtworkIdentity: "sidecar:album/cover.jpg" }), bindings)).toEqual({
      kind: "file",
      path: "/Music/album/cover.jpg",
    });
  });

  it("resolves embedded artwork through the media file", () => {
    expect(musicListArtworkSource(item({ originalArtworkIdentity: "embedded:sha256:cover" }), bindings)).toEqual({
      kind: "embedded",
      path: "/Music/album/track.flac",
      identity: "embedded:sha256:cover",
    });
  });
});
