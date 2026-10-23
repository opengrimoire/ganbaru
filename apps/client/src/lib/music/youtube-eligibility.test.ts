import { describe, expect, it } from "vitest";
import { musicItemCountsAsPlayable, youtubeResolutionAllowsPlayback } from "./youtube-eligibility";

describe("youtube resolution eligibility", () => {
  it.each([
    ["ready", true],
    ["resolving", true],
    ["timed-out", true],
    [null, true],
    ["embedding-blocked", false],
    ["unavailable", false],
  ] as const)("treats %s as playable: %s", (state, playable) => {
    expect(youtubeResolutionAllowsPlayback(state)).toBe(playable);
  });

  it("counts YouTube videos by resolution state and local files by device availability", () => {
    expect(musicItemCountsAsPlayable({ sourceKind: "youtube-video", availability: "unknown", youtubeResolutionState: "ready" })).toBe(true);
    expect(musicItemCountsAsPlayable({ sourceKind: "youtube-video", availability: "available", youtubeResolutionState: "unavailable" })).toBe(false);
    expect(musicItemCountsAsPlayable({ sourceKind: "local-file", availability: "available", youtubeResolutionState: null })).toBe(true);
    expect(musicItemCountsAsPlayable({ sourceKind: "local-file", availability: "unknown", youtubeResolutionState: null })).toBe(false);
  });
});
