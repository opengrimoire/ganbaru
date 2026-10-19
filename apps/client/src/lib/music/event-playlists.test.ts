import { describe, expect, it } from "vitest";
import { completeMusicAssignmentDrafts, persistedMusicAssignmentDrafts } from "./assignment-draft";
import type { MusicActivityPhase, MusicContextAssignmentDraft } from "./context-assignment";
import {
  chooseEventPlaylist,
  effectiveEventPlaylistId,
  eventMusicPhases,
  restoreEventPlaylists,
  silenceEventPlaylists,
  snapshotHasEventPlaylist,
} from "./event-playlists";

function draft(
  phase: MusicActivityPhase,
  update: Partial<MusicContextAssignmentDraft> = {},
): MusicContextAssignmentDraft {
  return {
    phase,
    behavior: "inherit",
    playlistId: null,
    soundscapeId: null,
    soundscapePresetId: null,
    soundscapeBehavior: "inherit",
    provenanceKind: "explicit",
    provenanceId: null,
    ...update,
  };
}

const projectSnapshot = completeMusicAssignmentDrafts([
  draft("focus", { behavior: "play-automatically", playlistId: "deep-work", provenanceKind: "copied-project", provenanceId: "project-1" }),
  draft("short-break", { behavior: "pause-music", provenanceKind: "copied-project", provenanceId: "project-1" }),
]);

describe("event playlist choices", () => {
  it("offers only the Focus phase when the event has no Pomodoro", () => {
    expect(eventMusicPhases(false)).toEqual(["focus"]);
    expect(eventMusicPhases(true)).toEqual(["focus", "short-break", "long-break"]);
  });

  it("shows the project playlist until the event overrides it", () => {
    const overrides = completeMusicAssignmentDrafts([]);
    expect(effectiveEventPlaylistId(overrides, projectSnapshot, "focus")).toBe("deep-work");
    expect(effectiveEventPlaylistId(overrides, projectSnapshot, "short-break")).toBeNull();

    const chosen = chooseEventPlaylist(overrides, projectSnapshot, "focus", "lofi");
    expect(effectiveEventPlaylistId(chosen, projectSnapshot, "focus")).toBe("lofi");
  });

  it("treats a silent override as no playlist even when the project has one", () => {
    const silenced = chooseEventPlaylist([], projectSnapshot, "focus", null);
    expect(silenced[0]).toMatchObject({ behavior: "pause-music", playlistId: null });
    expect(effectiveEventPlaylistId(silenced, projectSnapshot, "focus")).toBeNull();
  });

  it("follows the snapshot when an override holds only inherit values", () => {
    const overrides = [draft("focus", { playlistId: "stale" })];
    expect(effectiveEventPlaylistId(overrides, projectSnapshot, "focus")).toBe("deep-work");
  });

  it("lets a soundscape-only override hide the project playlist like the native resolver", () => {
    const overrides = [draft("focus", { soundscapeId: "rain", soundscapeBehavior: "play-selected" })];
    expect(effectiveEventPlaylistId(overrides, projectSnapshot, "focus")).toBeNull();
  });

  it("drops the override when the choice matches the project again", () => {
    const changed = chooseEventPlaylist([], projectSnapshot, "focus", "lofi");
    const reverted = chooseEventPlaylist(changed, projectSnapshot, "focus", "deep-work");
    expect(persistedMusicAssignmentDrafts(reverted)).toEqual([]);

    const silentAgain = chooseEventPlaylist([], projectSnapshot, "short-break", null);
    expect(persistedMusicAssignmentDrafts(silentAgain)).toEqual([]);
  });

  it("clears ambient sound fields when a playlist is chosen", () => {
    const overrides = [draft("focus", { behavior: "keep-current-music", soundscapeId: "rain", soundscapeBehavior: "play-selected" })];
    const chosen = chooseEventPlaylist(overrides, projectSnapshot, "focus", "lofi");
    expect(chosen[0]).toMatchObject({
      behavior: "play-automatically",
      playlistId: "lofi",
      soundscapeId: null,
      soundscapePresetId: null,
      soundscapeBehavior: "inherit",
    });
  });

  it("leaves other phases untouched", () => {
    const overrides = [draft("long-break", { behavior: "play-automatically", playlistId: "rest" })];
    const chosen = chooseEventPlaylist(overrides, projectSnapshot, "focus", "lofi");
    expect(chosen[2]).toEqual(overrides[0]);
  });

  it("silences only the listed phases that play music", () => {
    const overrides = [draft("long-break", { behavior: "play-automatically", playlistId: "rest" })];
    const silenced = silenceEventPlaylists(overrides, projectSnapshot, ["focus"]);
    expect(effectiveEventPlaylistId(silenced, projectSnapshot, "focus")).toBeNull();
    expect(effectiveEventPlaylistId(silenced, projectSnapshot, "long-break")).toBe("rest");
    expect(silenced[1]).toEqual(draft("short-break"));
  });

  it("restores project playlists for the listed phases", () => {
    const silenced = silenceEventPlaylists([], projectSnapshot, eventMusicPhases(true));
    expect(snapshotHasEventPlaylist(projectSnapshot, ["focus"])).toBe(true);
    expect(snapshotHasEventPlaylist(projectSnapshot, ["short-break", "long-break"])).toBe(false);

    const restored = restoreEventPlaylists(silenced, ["focus"]);
    expect(effectiveEventPlaylistId(restored, projectSnapshot, "focus")).toBe("deep-work");
    expect(persistedMusicAssignmentDrafts(restored)).toEqual([]);
  });
});
