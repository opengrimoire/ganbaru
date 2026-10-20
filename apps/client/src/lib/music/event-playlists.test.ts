import { describe, expect, it } from "vitest";
import { completeMusicAssignmentDrafts, persistedMusicAssignmentDrafts } from "./assignment-draft";
import type { MusicActivityPhase, MusicContextAssignmentDraft } from "./context-assignment";
import {
  chooseEventPlaylist,
  chooseEventSound,
  effectiveEventPlaylistId,
  effectiveEventSound,
  eventMusicPhases,
  eventSoundPlays,
  restoreEventMusic,
  silenceEventMusic,
  snapshotHasEventMusic,
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

const rainySnapshot = completeMusicAssignmentDrafts([
  draft("focus", { soundscapeBehavior: "play-selected", soundscapePresetId: "evening", provenanceKind: "copied-project" }),
  draft("short-break", { soundscapeBehavior: "pause-soundscape", provenanceKind: "copied-project" }),
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

  it("keeps the project playlist under a sound-only override like the native per-field resolver", () => {
    const overrides = [draft("focus", { soundscapeId: "rain", soundscapeBehavior: "play-selected" })];
    expect(effectiveEventPlaylistId(overrides, projectSnapshot, "focus")).toBe("deep-work");
    expect(effectiveEventSound(overrides, projectSnapshot, "focus")).toEqual({ kind: "sound", soundscapeId: "rain" });
  });

  it("drops the override when the choice matches the project again", () => {
    const changed = chooseEventPlaylist([], projectSnapshot, "focus", "lofi");
    const reverted = chooseEventPlaylist(changed, projectSnapshot, "focus", "deep-work");
    expect(persistedMusicAssignmentDrafts(reverted)).toEqual([]);

    const silentAgain = chooseEventPlaylist([], projectSnapshot, "short-break", null);
    expect(persistedMusicAssignmentDrafts(silentAgain)).toEqual([]);
  });

  it("keeps the phase's soundscape when a playlist is chosen or reverted", () => {
    const overrides = [draft("focus", { behavior: "keep-current-music", soundscapeId: "rain", soundscapeBehavior: "play-selected" })];
    const chosen = chooseEventPlaylist(overrides, projectSnapshot, "focus", "lofi");
    expect(chosen[0]).toMatchObject({
      behavior: "play-automatically",
      playlistId: "lofi",
      soundscapeId: "rain",
      soundscapeBehavior: "play-selected",
    });
    const reverted = chooseEventPlaylist(chosen, projectSnapshot, "focus", "deep-work");
    expect(reverted[0]).toMatchObject({ behavior: "inherit", playlistId: null, soundscapeId: "rain", soundscapeBehavior: "play-selected" });
  });

  it("leaves other phases untouched", () => {
    const overrides = [draft("long-break", { behavior: "play-automatically", playlistId: "rest" })];
    const chosen = chooseEventPlaylist(overrides, projectSnapshot, "focus", "lofi");
    expect(chosen[2]).toEqual(overrides[0]);
  });
});

describe("event soundscape choices", () => {
  it("shows the project sound until the event overrides it", () => {
    expect(effectiveEventSound([], rainySnapshot, "focus")).toEqual({ kind: "preset", presetId: "evening" });
    expect(effectiveEventSound([], rainySnapshot, "short-break")).toEqual({ kind: "silence" });
    expect(effectiveEventSound([], rainySnapshot, "long-break")).toEqual({ kind: "keep" });

    const chosen = chooseEventSound([], rainySnapshot, "long-break", { kind: "sound", soundscapeId: "rain" });
    expect(chosen[2]).toMatchObject({ behavior: "inherit", soundscapeBehavior: "play-selected", soundscapeId: "rain", soundscapePresetId: null });
    expect(effectiveEventSound(chosen, rainySnapshot, "long-break")).toEqual({ kind: "sound", soundscapeId: "rain" });
  });

  it("stores Don't change over a project sound as keeping the current sound", () => {
    const kept = chooseEventSound([], rainySnapshot, "focus", { kind: "keep" });
    expect(kept[0]).toMatchObject({ soundscapeBehavior: "keep-current-soundscape", soundscapeId: null, soundscapePresetId: null });
    expect(effectiveEventSound(kept, rainySnapshot, "focus")).toEqual({ kind: "keep" });
    expect(persistedMusicAssignmentDrafts(kept).map((assignment) => assignment.phase)).toEqual(["focus"]);
  });

  it("drops the sound override when the choice matches the project again without touching music", () => {
    const changed = chooseEventPlaylist(chooseEventSound([], rainySnapshot, "focus", { kind: "silence" }), rainySnapshot, "focus", "lofi");
    const reverted = chooseEventSound(changed, rainySnapshot, "focus", { kind: "preset", presetId: "evening" });
    expect(reverted[0]).toMatchObject({ behavior: "play-automatically", playlistId: "lofi", soundscapeBehavior: "inherit", soundscapePresetId: null });

    const keepAgain = chooseEventSound([], rainySnapshot, "long-break", { kind: "keep" });
    expect(persistedMusicAssignmentDrafts(keepAgain)).toEqual([]);
  });

  it("counts only sounds and presets as playing", () => {
    expect(eventSoundPlays({ kind: "sound", soundscapeId: "rain" })).toBe(true);
    expect(eventSoundPlays({ kind: "preset", presetId: "evening" })).toBe(true);
    expect(eventSoundPlays({ kind: "silence" })).toBe(false);
    expect(eventSoundPlays({ kind: "keep" })).toBe(false);
  });
});

describe("event music toggle", () => {
  it("silences only the listed phases that play music", () => {
    const overrides = [draft("long-break", { behavior: "play-automatically", playlistId: "rest" })];
    const silenced = silenceEventMusic(overrides, projectSnapshot, ["focus"]);
    expect(effectiveEventPlaylistId(silenced, projectSnapshot, "focus")).toBeNull();
    expect(effectiveEventPlaylistId(silenced, projectSnapshot, "long-break")).toBe("rest");
    expect(silenced[1]).toEqual(draft("short-break"));
  });

  it("silences soundscape with music and leaves quiet phases alone", () => {
    const silenced = silenceEventMusic([], rainySnapshot, eventMusicPhases(true));
    expect(effectiveEventSound(silenced, rainySnapshot, "focus")).toEqual({ kind: "silence" });
    expect(silenced[1]).toEqual(draft("short-break"));
    expect(silenced[2]).toEqual(draft("long-break"));
    expect(eventMusicPhases(true).some((phase) => eventSoundPlays(effectiveEventSound(silenced, rainySnapshot, phase)))).toBe(false);
  });

  it("restores project playlists and sounds for the listed phases", () => {
    const silenced = silenceEventMusic([], projectSnapshot, eventMusicPhases(true));
    expect(snapshotHasEventMusic(projectSnapshot, ["focus"])).toBe(true);
    expect(snapshotHasEventMusic(projectSnapshot, ["short-break", "long-break"])).toBe(false);
    expect(snapshotHasEventMusic(rainySnapshot, ["focus"])).toBe(true);
    expect(snapshotHasEventMusic(rainySnapshot, ["short-break"])).toBe(false);

    const restored = restoreEventMusic(silenced, ["focus"]);
    expect(effectiveEventPlaylistId(restored, projectSnapshot, "focus")).toBe("deep-work");
    expect(persistedMusicAssignmentDrafts(restored)).toEqual([]);
  });
});
