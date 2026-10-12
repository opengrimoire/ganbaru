import { describe, expect, it } from "vitest";
import { coverPlaybackHostRect } from "./playback-host-layout";

describe("coverPlaybackHostRect", () => {
  it("expands fractional bounds to complete physical pixels", () => {
    expect(coverPlaybackHostRect({
      left: 10.25,
      top: 20.75,
      right: 110.2,
      bottom: 76.1,
    }, 2)).toEqual({
      left: 10,
      top: 20.5,
      width: 100.5,
      height: 56,
    });
  });

  it("preserves bounds already aligned to the device pixel grid", () => {
    expect(coverPlaybackHostRect({
      left: 10.5,
      top: 20,
      right: 110,
      bottom: 76.5,
    }, 2)).toEqual({
      left: 10.5,
      top: 20,
      width: 99.5,
      height: 56.5,
    });
  });

  it("falls back to a one-to-one pixel ratio for invalid scale values", () => {
    expect(coverPlaybackHostRect({
      left: 1.2,
      top: 2.8,
      right: 5.1,
      bottom: 8.2,
    }, 0)).toEqual({
      left: 1,
      top: 2,
      width: 5,
      height: 7,
    });
  });
});
