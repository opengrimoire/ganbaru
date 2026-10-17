import { describe, expect, it } from "vitest";
import { parseMusicTransferPreview, parseMusicTransferResult, type MusicTransferPreview } from "./interchange";

export const transferPreview: MusicTransferPreview = {
  format: "json", revision: "a".repeat(64), roots: [{ id: "root", name: "Songs" }], availableRoots: [],
  boundRootIds: [], contextAssignmentCount: 0, newPlaylists: 1, matchedPlaylists: 0, newItems: 1,
  matchedItems: 2, duplicateItems: 1, missingLocalBindings: 1, conflicts: [], unsupported: [],
  localCount: 0, youtubeCount: 0, unsupportedCount: 0, unresolvedLocalCount: 0,
};

describe("native Music transfer contracts", () => {
  it("accepts only compact reviewed native results", () => {
    expect(parseMusicTransferPreview(transferPreview)).toEqual(transferPreview);
    expect(parseMusicTransferResult({ playlistCount: 1, itemCount: 2, membershipCount: 3, assignmentCount: 0 }))
      .toEqual({ playlistCount: 1, itemCount: 2, membershipCount: 3, assignmentCount: 0 });
  });

  it("rejects invalid revisions, oversized root lists, unknown formats and unsafe counts", () => {
    for (const patch of [
      { revision: "arbitrary" }, { format: "xml" }, { newItems: -1 }, { matchedItems: 0.5 },
      { localCount: Number.NaN }, { newPlaylists: "1" }, { roots: Array(501).fill({ id: "root", name: "Songs" }) },
      { availableRoots: [{ id: 1, name: "Songs" }] }, { unsupported: [null] },
    ]) expect(() => parseMusicTransferPreview({ ...transferPreview, ...patch })).toThrow();
    expect(() => parseMusicTransferResult({ playlistCount: 1, itemCount: 2, membershipCount: -1, assignmentCount: 0 })).toThrow();
  });
});
