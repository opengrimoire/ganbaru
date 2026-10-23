import { describe, expect, it } from "vitest";
import { youtubeResolutionAllowsPlayback } from "./youtube-eligibility";

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
});
