import { describe, expect, it } from "vitest";
import {
  formatMusicDuration,
  musicAvailabilityTone,
  musicItemSecondaryText,
  musicReviewTone,
} from "./presentation";
import type { MusicItemListEntry } from "$lib/music/library/contracts";

const item: MusicItemListEntry = {
  id: "track-1",
  identityKey: "local:track-1",
  sourceKind: "local-file",
  mediaKind: "audio",
  title: "Track",
  artist: "",
  album: "",
  localRootId: "root-1",
  relativePath: "Track.flac",
  sourceCollectionIds: ["source-1"],
  originalArtworkIdentity: null,
  artworkOverride: null,
  durationMs: 65_000,
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

describe("music builder presentation", () => {
  it("formats short and long durations without invalid output", () => {
    expect(formatMusicDuration(65_000)).toBe("1:05");
    expect(formatMusicDuration(3_723_000)).toBe("1:02:03");
    expect(formatMusicDuration(null)).toBe("");
    expect(formatMusicDuration(-1)).toBe("");
  });

  it("provides useful metadata fallbacks", () => {
    expect(musicItemSecondaryText(item, "Unknown artist", "No album")).toEqual({
      primary: "Unknown artist",
      secondary: "No album",
    });
  });

  it("maps issue and review states to semantic tones", () => {
    expect(musicAvailabilityTone("missing")).toBe("danger");
    expect(musicAvailabilityTone("ambiguous")).toBe("warning");
    expect(musicAvailabilityTone("unknown")).toBe("neutral");
    expect(musicAvailabilityTone("available")).toBe("neutral");
    expect(musicReviewTone("unreviewed")).toBe("accent");
    expect(musicReviewTone("ignored")).toBe("muted");
  });
});
