import { describe, expect, it } from "vitest";
import {
  moveMusicPlaylistOrder,
  musicPlaylistGridInsertion,
  musicPlaylistGridLayout,
} from "./order";

const playlists = ["a", "b", "c", "d"].map((id) => ({ id }));

describe("music playlist order", () => {
  it("moves playlists in either direction without mutating the source", () => {
    expect(moveMusicPlaylistOrder(playlists, "b", 3).map(({ id }) => id)).toEqual(["a", "c", "d", "b"]);
    expect(moveMusicPlaylistOrder(playlists, "d", 1).map(({ id }) => id)).toEqual(["a", "d", "b", "c"]);
    expect(playlists.map(({ id }) => id)).toEqual(["a", "b", "c", "d"]);
  });

  it("bounds edge targets and preserves unknown ids", () => {
    expect(moveMusicPlaylistOrder(playlists, "c", -10).map(({ id }) => id)).toEqual(["c", "a", "b", "d"]);
    expect(moveMusicPlaylistOrder(playlists, "b", 99).map(({ id }) => id)).toEqual(["a", "c", "d", "b"]);
    expect(moveMusicPlaylistOrder(playlists, "missing", 1)).toEqual(playlists);
  });

  it("chooses the nearest two-column slot from the dragged card position", () => {
    const slots = [
      { index: 0, left: 0, top: 0 },
      { index: 1, left: 220, top: 0 },
      { index: 2, left: 0, top: 64 },
      { index: 3, left: 220, top: 64 },
    ];

    expect(musicPlaylistGridInsertion(slots, 170, 4, 0).index).toBe(1);
    expect(musicPlaylistGridInsertion(slots, 8, 48, 0).index).toBe(2);
    expect(musicPlaylistGridInsertion(slots, 110, 0, 0).index).toBe(0);
  });

  it("lays playlists out in stable measured rows", () => {
    expect(musicPlaylistGridLayout(552, [52, 60, 48, 54])).toEqual({
      positions: [
        { left: 0, top: 0, width: 272 },
        { left: 280, top: 0, width: 272 },
        { left: 0, top: 68, width: 272 },
        { left: 280, top: 68, width: 272 },
      ],
      height: 122,
      columns: 2,
    });
  });
});
