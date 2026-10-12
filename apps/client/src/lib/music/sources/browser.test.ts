import { describe, expect, it } from "vitest";
import type {
  MusicItemListEntry,
  MusicSourceCollection,
  MusicSourceSummary,
} from "$lib/music/library/contracts";
import {
  buildMusicSourceBrowser,
  allMusicSourceBrowserItems,
  findMusicSourceBrowserNode,
  LOCAL_MUSIC_SOURCE_ID,
  musicSourceBrowserPath,
  musicSourceBrowserItems,
  SAVED_YOUTUBE_VIDEOS_ID,
  YOUTUBE_MUSIC_SOURCE_ID,
} from "./browser";

function item(
  id: string,
  sourceKind: MusicItemListEntry["sourceKind"],
  patch: Partial<MusicItemListEntry> = {},
): MusicItemListEntry {
  return {
    id,
    identityKey: id,
    sourceKind,
    mediaKind: "audio",
    title: id,
    artist: "",
    album: "",
    localRootId: null,
    relativePath: null,
    originalArtworkIdentity: null,
    artworkOverride: null,
    durationMs: null,
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
    ...patch,
    sourceCollectionIds: patch.sourceCollectionIds ?? [],
  };
}

function collection(
  id: string,
  kind: MusicSourceCollection["kind"],
  name: string,
): MusicSourceCollection {
  return {
    id,
    kind,
    identityKey: id,
    name,
    localRootId: kind === "local-root" ? `${id}-root` : null,
    youtubePlaylistId: kind === "youtube-playlist" ? `${id}-list` : null,
    refreshState: "idle",
    lastSuccessfulRefreshAtMs: null,
    previousSuccessfulRefreshAtMs: null,
    lastRefreshErrorCode: null,
    snapshotGeneration: 1,
    createdAtMs: 1,
    updatedAtMs: 1,
    version: 1,
    discoveryEnabled: true,
    removedAtMs: null,
  };
}

function summary(source: MusicSourceCollection): MusicSourceSummary {
  return {
    id: source.id,
    kind: source.kind,
    name: source.name,
    refreshState: "idle",
    lastSuccessfulRefreshAtMs: null,
    localRootId: source.localRootId,
    youtubePlaylistId: source.youtubePlaylistId,
    itemCount: 0,
    missingCount: 0,
    newCount: 0,
    unreviewedCount: 0,
    unavailableCount: 0,
    ambiguousCount: 0,
    openIssueCount: 0,
    health: "healthy",
    discoveryEnabled: true,
    version: 1,
  };
}

describe("music source browser", () => {
  it("keeps two stable roots and places content in folder-shaped children", () => {
    const local = collection("local", "local-root", "Main folder");
    const playlist = collection("playlist", "youtube-playlist", "Focus videos");
    const tree = buildMusicSourceBrowser([
      item("local-track", "local-file", {
        localRootId: "local-root",
        relativePath: "Games/Nier/theme.flac",
        sourceCollectionIds: ["local"],
      }),
      item("playlist-video", "youtube-video", { sourceCollectionIds: ["playlist"] }),
      item("single-video", "youtube-video"),
    ], [local, playlist], [summary(local), summary(playlist)], {
      local: "Local music",
      youtube: "YouTube",
      savedVideos: "Saved videos",
      unlinkedFiles: "Unlinked files",
    });

    expect(tree.map((node) => node.id)).toEqual([LOCAL_MUSIC_SOURCE_ID, YOUTUBE_MUSIC_SOURCE_ID]);
    expect(findMusicSourceBrowserNode(tree, "source-collection:local")?.children[0]?.name).toBe("Games");
    expect(findMusicSourceBrowserNode(tree, "source-collection:playlist")?.itemIds).toEqual(["playlist-video"]);
    expect(findMusicSourceBrowserNode(tree, SAVED_YOUTUBE_VIDEOS_ID)?.itemIds).toEqual(["single-video"]);
    expect(findMusicSourceBrowserNode(tree, YOUTUBE_MUSIC_SOURCE_ID)?.itemIds.toSorted()).toEqual([
      "playlist-video",
      "single-video",
    ]);
  });

  it("returns a complete breadcrumb path for nested local folders", () => {
    const local = collection("local", "local-root", "Main folder");
    const tree = buildMusicSourceBrowser([
      item("track", "local-file", {
        localRootId: "local-root",
        relativePath: "Games/Nier/theme.flac",
      }),
    ], [local], [summary(local)], {
      local: "Local music",
      youtube: "YouTube",
      savedVideos: "Saved videos",
      unlinkedFiles: "Unlinked files",
    });
    const nier = findMusicSourceBrowserNode(tree, "source-folder:local-root:Games/Nier");

    expect(nier).not.toBeNull();
    expect(musicSourceBrowserPath(tree, nier!.id).map((node) => node.name)).toEqual([
      "Local music",
      "Main folder",
      "Games",
      "Nier",
    ]);
    expect(musicSourceBrowserItems(findMusicSourceBrowserNode(tree, LOCAL_MUSIC_SOURCE_ID)!).map((entry) => entry.id)).toEqual(["track"]);
    expect(allMusicSourceBrowserItems(tree).map((entry) => entry.id)).toEqual(["track"]);
  });
});
