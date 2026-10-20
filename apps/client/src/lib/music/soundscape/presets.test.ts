import { describe, expect, it } from "vitest";
import { MAX_SOUNDSCAPE_LAYERS, type MusicSoundscapeDefinition } from "./contracts";
import { presetSoundNames, sameSoundSet, togglePresetDraftSound } from "./presets";

function sound(id: string, name: string): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    groupId: null, availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

describe("background sound preset drafts", () => {
  it("adds sounds at the end and removes them without reordering the rest", () => {
    expect(togglePresetDraftSound([], "rain")).toEqual(["rain"]);
    expect(togglePresetDraftSound(["rain", "cafe"], "fire")).toEqual(["rain", "cafe", "fire"]);
    expect(togglePresetDraftSound(["rain", "cafe", "fire"], "cafe")).toEqual(["rain", "fire"]);
  });

  it("refuses to grow past the layer limit but still allows removal at the limit", () => {
    const full = Array.from({ length: MAX_SOUNDSCAPE_LAYERS }, (_, index) => `sound-${index}`);
    expect(togglePresetDraftSound(full, "extra")).toEqual(full);
    expect(togglePresetDraftSound(full, "sound-0")).toHaveLength(MAX_SOUNDSCAPE_LAYERS - 1);
  });

  it("compares sound sets regardless of order", () => {
    expect(sameSoundSet(["rain", "cafe"], ["cafe", "rain"])).toBe(true);
    expect(sameSoundSet(["rain"], ["rain", "cafe"])).toBe(false);
    expect(sameSoundSet(["rain", "fire"], ["rain", "cafe"])).toBe(false);
  });

  it("names sounds in preset order and skips removed sounds", () => {
    const definitions = [sound("cafe", "Café"), sound("rain", "Porch rain")];
    expect(presetSoundNames(["rain", "gone", "cafe"], definitions, (entry) => entry.name)).toEqual(["Porch rain", "Café"]);
  });
});
