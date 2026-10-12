import { describe, expect, it } from "vitest";
import { shouldRecoverSoundscapeOutput } from "./lifecycle";

describe("soundscape output lifecycle", () => {
  it("recovers desired playback after errors or likely suspend", () => {
    expect(shouldRecoverSoundscapeOutput({ desiredPlaying: true, status: "error", hiddenForMs: 0 })).toBe(true);
    expect(shouldRecoverSoundscapeOutput({ desiredPlaying: true, status: "playing", hiddenForMs: 30_000 })).toBe(true);
  });

  it("does no output work while stopped or briefly backgrounded", () => {
    expect(shouldRecoverSoundscapeOutput({ desiredPlaying: false, status: "error", hiddenForMs: 60_000 })).toBe(false);
    expect(shouldRecoverSoundscapeOutput({ desiredPlaying: true, status: "playing", hiddenForMs: 2_000 })).toBe(false);
  });
});
