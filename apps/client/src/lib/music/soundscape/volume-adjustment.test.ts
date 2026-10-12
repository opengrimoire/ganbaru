import { describe, expect, it } from "vitest";
import { soundscapeLevelFromAdjustment, soundscapeVolumeAdjustment } from "./volume-adjustment";

describe("soundscape volume adjustment", () => {
  it("shows an unset or neutral multiplier at the center", () => {
    expect(soundscapeVolumeAdjustment(null)).toBe(0);
    expect(soundscapeVolumeAdjustment(1)).toBe(0);
    expect(soundscapeLevelFromAdjustment(0)).toBeNull();
  });

  it("maps decreases and increases to relative section levels", () => {
    expect(soundscapeLevelFromAdjustment(-100)).toBe(0);
    expect(soundscapeLevelFromAdjustment(-50)).toBe(0.5);
    expect(soundscapeLevelFromAdjustment(50)).toBe(1.5);
    expect(soundscapeLevelFromAdjustment(100)).toBe(2);
    expect(soundscapeVolumeAdjustment(0)).toBe(-100);
    expect(soundscapeVolumeAdjustment(1.5)).toBe(50);
    expect(soundscapeVolumeAdjustment(2)).toBe(100);
  });
});
