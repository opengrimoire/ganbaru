import { describe, expect, it } from "vitest";
import { IDLE_MUSIC_AUDITION, transitionMusicAudition, type MusicAuditionState } from "./audition-state";

const checkpoint = { checkpointId: "checkpoint" };
const suspending: MusicAuditionState = { phase: "suspending" };
const auditioning: MusicAuditionState = { phase: "auditioning", checkpoint, itemId: null };

describe("transitionMusicAudition", () => {
  it("suspends from idle and keeps an audition that already started", () => {
    expect(transitionMusicAudition(IDLE_MUSIC_AUDITION, { type: "suspendStarted" })).toEqual(suspending);
    expect(transitionMusicAudition(suspending, { type: "suspendStarted" })).toBe(suspending);
    expect(transitionMusicAudition(auditioning, { type: "suspendStarted" })).toBe(auditioning);
  });

  it("holds the checkpoint only for a suspension that is still running", () => {
    expect(transitionMusicAudition(suspending, { type: "suspended", checkpoint })).toEqual(auditioning);
    expect(transitionMusicAudition(IDLE_MUSIC_AUDITION, { type: "suspended", checkpoint })).toBe(IDLE_MUSIC_AUDITION);
    expect(transitionMusicAudition(auditioning, { type: "suspended", checkpoint: { checkpointId: "other" } }))
      .toBe(auditioning);
  });

  it("records the loaded item only while auditioning", () => {
    const loaded = transitionMusicAudition(auditioning, { type: "itemLoaded", itemId: "a" });
    expect(loaded).toEqual({ phase: "auditioning", checkpoint, itemId: "a" });
    expect(transitionMusicAudition(loaded, { type: "itemLoaded", itemId: "a" })).toBe(loaded);
    expect(transitionMusicAudition(loaded, { type: "itemLoaded", itemId: "b" })).toEqual({
      phase: "auditioning", checkpoint, itemId: "b",
    });
    expect(transitionMusicAudition(suspending, { type: "itemLoaded", itemId: "a" })).toBe(suspending);
    expect(transitionMusicAudition(IDLE_MUSIC_AUDITION, { type: "itemLoaded", itemId: "a" })).toBe(IDLE_MUSIC_AUDITION);
  });

  it("ends from every phase", () => {
    expect(transitionMusicAudition(suspending, { type: "ended" })).toBe(IDLE_MUSIC_AUDITION);
    expect(transitionMusicAudition(auditioning, { type: "ended" })).toBe(IDLE_MUSIC_AUDITION);
    expect(transitionMusicAudition(IDLE_MUSIC_AUDITION, { type: "ended" })).toBe(IDLE_MUSIC_AUDITION);
  });
});
