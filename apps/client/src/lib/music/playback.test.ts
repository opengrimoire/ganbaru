import { describe, expect, it } from "vitest";
import {
  clampRate,
  clampVolume,
  formatPlaybackTime,
  formatVolumePercent,
  localMediaSeekTargetMs,
  normalizeLocalPlayableStartMs,
  stableStatusDuringYouTubeBuffering,
} from "./playback";

describe("formatPlaybackTime", () => {
  it("formats minute and hour durations", () => {
    expect(formatPlaybackTime(61_000)).toBe("1:01");
    expect(formatPlaybackTime(3_661_000)).toBe("1:01:01");
    expect(formatPlaybackTime(null)).toBe("0:00");
  });
});

describe("stableStatusDuringYouTubeBuffering", () => {
  it("keeps playback controls stable during normal YouTube buffering", () => {
    expect(stableStatusDuringYouTubeBuffering("playing", "loading")).toBe("playing");
    expect(stableStatusDuringYouTubeBuffering("paused", "loading")).toBe("paused");
    expect(stableStatusDuringYouTubeBuffering("loading", "loading")).toBe("loading");
  });

  it("accepts non-buffering YouTube status changes", () => {
    expect(stableStatusDuringYouTubeBuffering("playing", "paused")).toBe("paused");
    expect(stableStatusDuringYouTubeBuffering("paused", "playing")).toBe("playing");
  });
});

describe("clamp helpers", () => {
  it("keeps volume and rate in supported ranges", () => {
    expect(clampVolume(-1)).toBe(0);
    expect(clampVolume(2)).toBe(1);
    expect(clampRate(0.1)).toBe(0.25);
    expect(clampRate(4)).toBe(2);
  });

  it("formats volume using the normal volume cap", () => {
    expect(formatVolumePercent(0.75)).toBe("75%");
    expect(formatVolumePercent(1.25)).toBe("100%");
  });

  it("keeps local media seeks out of a non-playable prefix", () => {
    const playableStartMs = normalizeLocalPlayableStartMs(62_200);

    expect(playableStartMs).toBe(62_200);
    expect(localMediaSeekTargetMs(0, playableStartMs)).toBe(62_200);
    expect(localMediaSeekTargetMs(10_000, playableStartMs)).toBe(62_200);
    expect(localMediaSeekTargetMs(72_200, playableStartMs)).toBe(72_200);
  });

});
