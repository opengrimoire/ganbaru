import { describe, expect, it } from "vitest";
import {
  completeMusicAssignmentDrafts,
  musicAssignmentDraftsEqual,
  persistedMusicAssignmentDrafts,
  updateMusicAssignmentDraft,
} from "./assignment-draft";

describe("music assignment drafts", () => {
  it("completes sparse persisted rows in stable phase order", () => {
    const drafts = completeMusicAssignmentDrafts([{
      phase: "short-break",
      behavior: "pause-music",
      playlistId: null,
      soundscapeId: null,
      soundscapePresetId: null,
      soundscapeBehavior: "inherit",
      provenanceKind: "explicit",
      provenanceId: null,
    }]);
    expect(drafts.map((entry) => entry.phase)).toEqual(["focus", "short-break", "long-break"]);
    expect(drafts.map((entry) => entry.behavior)).toEqual(["inherit", "pause-music", "inherit"]);
  });

  it("clears a stale playlist when behavior no longer uses one", () => {
    const drafts = completeMusicAssignmentDrafts([]);
    const playing = updateMusicAssignmentDraft(drafts, "focus", {
      behavior: "play-automatically",
      playlistId: "playlist-1",
    });
    expect(updateMusicAssignmentDraft(playing, "focus", { behavior: "pause-music" })[0].playlistId).toBeNull();
  });

  it("drops untouched inherit rows but retains soundscape-only intent", () => {
    const drafts = completeMusicAssignmentDrafts([]);
    const withSoundscape = updateMusicAssignmentDraft(drafts, "long-break", { soundscapeBehavior: "play-selected", soundscapeId: "rain" });
    expect(persistedMusicAssignmentDrafts(withSoundscape).map((entry) => entry.phase)).toEqual(["long-break"]);
  });

  it("clears soundscape references when the phase stops selecting a sound", () => {
    const drafts = completeMusicAssignmentDrafts([]);
    const withPreset = updateMusicAssignmentDraft(drafts, "focus", { soundscapeBehavior: "play-selected", soundscapePresetId: "preset-1" });
    const silenced = updateMusicAssignmentDraft(withPreset, "focus", { soundscapeBehavior: "pause-soundscape" })[0];
    expect(silenced.soundscapePresetId).toBeNull();
    expect(silenced.soundscapeId).toBeNull();
    expect(persistedMusicAssignmentDrafts(withPreset).map((entry) => entry.soundscapePresetId)).toEqual(["preset-1"]);
  });

  it("compares sparse and complete drafts by semantic value", () => {
    expect(musicAssignmentDraftsEqual([], completeMusicAssignmentDrafts([]))).toBe(true);
  });
});
