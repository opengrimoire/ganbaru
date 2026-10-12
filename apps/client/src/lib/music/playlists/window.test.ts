import { describe, expect, it } from "vitest";
import { musicPlaylistWindow } from "./window";

describe("musicPlaylistWindow", () => {
  it("renders the visible rows with a bounded overscan buffer", () => {
    expect(musicPlaylistWindow(244, 360, 180, 36, 2)).toEqual({
      startIndex: 8,
      endIndex: 17,
      topSpacerHeight: 288,
      bottomSpacerHeight: 8172,
    });
  });

  it("clamps the first window to the start of the playlist", () => {
    expect(musicPlaylistWindow(20, 0, 108, 36, 3)).toEqual({
      startIndex: 0,
      endIndex: 6,
      topSpacerHeight: 0,
      bottomSpacerHeight: 504,
    });
  });

  it("keeps empty playlists empty", () => {
    expect(musicPlaylistWindow(0, 0, 200)).toEqual({
      startIndex: 0,
      endIndex: 0,
      topSpacerHeight: 0,
      bottomSpacerHeight: 0,
    });
  });
});
