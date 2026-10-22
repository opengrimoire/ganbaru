import { describe, expect, it } from "vitest";
import type { MusicPlaylistPlaybackEntry } from "$lib/music/library/contracts";
import {
  evaluateMusicQueueEntry,
  projectMusicPlaylistPlayback,
} from "./playback";

const entry = (patch: Partial<MusicPlaylistPlaybackEntry> = {}): MusicPlaylistPlaybackEntry => ({
  membershipId: "membership-1", itemId: "item-1", identityKey: "local:item-1", sourceKind: "local-file",
  youtubeVideoId: null, youtubeResolutionState: null, title: "Track", originalArtworkIdentity: null, artworkOverride: null,
  availability: "available", rootId: "root", relativePath: "album/track.flac",
  orderKey: "a0", weight: "normal", enabled: true, startMs: null, endMs: null, volumeMilli: null, rateMilli: null,
  snoozed: false, snoozedUntil: null, snoozedIndefinitely: false, skipRanges: [], ...patch,
});

const context = { nowMs: 1_700_000_000_000, online: true };

describe("saved playlist playback policy", () => {
  it("keeps resolvable entries while reporting one typed eligibility reason", () => {
    const projection = projectMusicPlaylistPlayback([
      entry(),
      entry({ membershipId: "disabled", itemId: "disabled", orderKey: "a1", enabled: false }),
      entry({ membershipId: "snoozed", itemId: "snoozed", orderKey: "a2", snoozed: true, snoozedIndefinitely: true }),
      entry({ membershipId: "unbound", itemId: "unbound", orderKey: "a3", rootId: "missing-root" }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], context);
    expect(projection.entries).toHaveLength(3);
    expect(projection.eligibleIndices).toEqual([0]);
    expect(projection.skipped).toMatchObject({ disabled: 1, snoozed: 1, "unbound-root": 1 });
  });

  it("resolves sidecar and override artwork for local playlist sources", () => {
    const projection = projectMusicPlaylistPlayback([
      entry({ originalArtworkIdentity: "sidecar:album/cover.jpg" }),
      entry({
        membershipId: "override",
        itemId: "override",
        orderKey: "a1",
        artworkOverride: "/custom/artwork.png",
      }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], context);

    expect(projection.sources[0]).toMatchObject({
      path: "/music/album/track.flac",
      artworkPath: "/music/album/cover.jpg",
    });
    expect(projection.sources[1]).toMatchObject({ artworkPath: "/custom/artwork.png" });
  });

  it("allows explicit play to bypass Snooze without bypassing disabled state", () => {
    const projection = projectMusicPlaylistPlayback([
      entry({ snoozed: true, snoozedIndefinitely: true }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], { ...context, explicitItemId: "item-1" });
    expect(projection.eligibleIndices).toEqual([0]);
    expect(evaluateMusicQueueEntry({ ...projection.entries[0], enabled: false }, { ...context, explicitItemId: "item-1" }).reason).toBe("disabled");
  });

  it("uses the same typed evaluator for phase-specific eligibility", () => {
    const projection = projectMusicPlaylistPlayback([
      entry(),
      entry({ itemId: "phase-item", membershipId: "phase-item", orderKey: "a1" }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], {
      ...context,
      phaseAllowedItemIds: new Set(["phase-item"]),
    });
    expect(projection.eligibleIndices).toEqual([1]);
    expect(projection.skipped["phase-constraint"]).toBe(1);
  });

  it("orders entries by order key and converts stored thousandths to multipliers", () => {
    const projection = projectMusicPlaylistPlayback([
      entry({ membershipId: "late", itemId: "late", orderKey: "a1" }),
      entry({ membershipId: "tie-b", itemId: "tie-b", orderKey: "a0" }),
      entry({ membershipId: "tie-a", itemId: "tie-a", orderKey: "a0", volumeMilli: 250, rateMilli: 1_500 }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], context);

    expect(projection.itemIds).toEqual(["tie-a", "tie-b", "late"]);
    expect(projection.entries[0]).toMatchObject({ volume: 0.25, rate: 1.5 });
    expect(projection.entries[1]).toMatchObject({ volume: null, rate: null });
  });

  it("uses the local subset while offline and reports skipped online entries", () => {
    const projection = projectMusicPlaylistPlayback([
      entry(),
      entry({ membershipId: "online", itemId: "online", identityKey: "youtube:abc", sourceKind: "youtube-video", youtubeVideoId: "abcdefghijk", youtubeResolutionState: "ready", rootId: null, relativePath: null, orderKey: "a1" }),
    ], [{ rootId: "root", folderPath: "/music", status: "available" }], { ...context, online: false });
    expect(projection.eligibleIndices).toEqual([0]);
    expect(projection.skipped.offline).toBe(1);
  });

});
