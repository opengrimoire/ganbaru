import { describe, expect, it } from "vitest";
import type { MusicMembershipMatrixEntry, MusicSnooze } from "./contracts";
import { musicMembershipsForScope, musicSnoozesForScope, musicWeightForScope } from "./track-preferences";

const membership = (playlistId: string): MusicMembershipMatrixEntry => ({
  itemId: "song",
  playlistId,
  weight: "normal",
});

describe("music track preferences", () => {
  it("targets every current membership or one selected playlist", () => {
    const memberships = [membership("a"), membership("b")];
    expect(musicMembershipsForScope(memberships, null)).toEqual(memberships);
    expect(musicMembershipsForScope(memberships, "b")).toEqual([memberships[1]]);
    expect(musicMembershipsForScope(memberships, "missing")).toEqual([]);
  });

  it("shows no selected die when likelihood differs across playlists or is unavailable", () => {
    expect(musicWeightForScope([membership("a"), membership("b")])).toBe("normal");
    expect(musicWeightForScope([membership("a"), { ...membership("b"), weight: "rarely" }])).toBeNull();
    expect(musicWeightForScope([])).toBeNull();
  });

  it("resumes only snoozes in the selected scope", () => {
    const snooze = (id: string, playlistId: string | null, endsAtMs: number | null): MusicSnooze => ({
      id, itemId: "song", scope: playlistId ? "playlist" : "all-playlists", playlistId,
      startsAtMs: 100, endsAtMs, reason: "", createdAtMs: 100,
    });
    const snoozes = [snooze("global", null, null), snooze("a", "a", 300), snooze("b", "b", null), snooze("old", "a", 150)];
    expect(musicSnoozesForScope(snoozes, null, 200).map((entry) => entry.id)).toEqual(["global", "a", "b"]);
    expect(musicSnoozesForScope(snoozes, "a", 200).map((entry) => entry.id)).toEqual(["a"]);
    expect(musicSnoozesForScope(snoozes, "b", 200).map((entry) => entry.id)).toEqual(["b"]);
  });
});
