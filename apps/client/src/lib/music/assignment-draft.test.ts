import { describe, expect, it } from "vitest";
import {
  completeMusicAssignmentDrafts,
  musicAssignmentDraftsEqual,
  nextMusicAssignmentPhase,
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
    const withSoundscape = updateMusicAssignmentDraft(drafts, "long-break", { soundscapeId: "rain" });
    expect(persistedMusicAssignmentDrafts(withSoundscape).map((entry) => entry.phase)).toEqual(["long-break"]);
  });

  it("compares sparse and complete drafts by semantic value", () => {
    expect(musicAssignmentDraftsEqual([], completeMusicAssignmentDrafts([]))).toBe(true);
  });

  it("moves phase tabs with wrapping arrow and boundary navigation", () => {
    expect(nextMusicAssignmentPhase("focus", "ArrowLeft")).toBe("long-break");
    expect(nextMusicAssignmentPhase("long-break", "ArrowRight")).toBe("focus");
    expect(nextMusicAssignmentPhase("short-break", "Home")).toBe("focus");
    expect(nextMusicAssignmentPhase("short-break", "End")).toBe("long-break");
  });
});
