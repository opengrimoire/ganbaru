import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import { MAX_SOUNDSCAPE_LAYERS, type MusicSoundscapeDefinition, type MusicSoundscapePreset } from "./contracts";
import {
  formatPresetLevelAdjustment,
  presetDisplayName,
  presetLevelAdjustment,
  presetLevelFromAdjustment,
  setPresetDraftSoundLevel,
  singleSoundPreset,
  soundDisplayName,
  togglePresetDraftSound,
} from "./presets";

function sound(id: string, name: string): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, bundledIdentity: null, name, icon: "lucide:audio-lines",
    availability: "available", localPath: `/sounds/${id}.ogg`, createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

function preset(id: string, name: string, soundIds: string[]): MusicSoundscapePreset {
  return { id, name, icon: "lucide:cloud", level: 0.5, sounds: soundIds.map((soundscapeId) => ({ soundscapeId, level: 1 })), createdAtMs: 1, updatedAtMs: 1, version: 1 };
}

const ids = (sounds: readonly { soundscapeId: string }[]) => sounds.map((entry) => entry.soundscapeId);

describe("soundscape preset drafts", () => {
  it("adds sounds at full share at the end and removes them without reordering the rest", () => {
    expect(togglePresetDraftSound([], "rain")).toEqual([{ soundscapeId: "rain", level: 1 }]);
    const draft = [{ soundscapeId: "rain", level: 0.4 }, { soundscapeId: "cafe", level: 1 }];
    expect(ids(togglePresetDraftSound(draft, "fire"))).toEqual(["rain", "cafe", "fire"]);
    expect(togglePresetDraftSound([...draft, { soundscapeId: "fire", level: 1 }], "cafe")).toEqual([{ soundscapeId: "rain", level: 0.4 }, { soundscapeId: "fire", level: 1 }]);
  });

  it("refuses to grow past the layer limit but still allows removal at the limit", () => {
    const full = Array.from({ length: MAX_SOUNDSCAPE_LAYERS }, (_, index) => ({ soundscapeId: `sound-${index}`, level: 1 }));
    expect(togglePresetDraftSound(full, "extra")).toEqual(full);
    expect(togglePresetDraftSound(full, "sound-0")).toHaveLength(MAX_SOUNDSCAPE_LAYERS - 1);
  });

  it("changes one sound's share and clamps it from silent to full", () => {
    const draft = [{ soundscapeId: "rain", level: 1 }, { soundscapeId: "cafe", level: 1 }];
    expect(setPresetDraftSoundLevel(draft, "cafe", 0.25)).toEqual([{ soundscapeId: "rain", level: 1 }, { soundscapeId: "cafe", level: 0.25 }]);
    expect(setPresetDraftSoundLevel(draft, "rain", 1.4)[0].level).toBe(1);
    expect(setPresetDraftSoundLevel(draft, "rain", -0.2)[0].level).toBe(0);
  });

  it("finds an existing one-sound preset and ignores mixes that contain the sound", () => {
    const presets = [preset("mix", "Mix", ["rain", "cafe"]), preset("solo", "Solo", ["rain"])];
    expect(singleSoundPreset(presets, "rain")?.id).toBe("solo");
    expect(singleSoundPreset(presets, "cafe")).toBeNull();
  });
});

describe("soundscape preset levels", () => {
  it("maps the slider center to the Music volume and its ends to silence and double", () => {
    expect(presetLevelFromAdjustment(0)).toBe(1);
    expect(presetLevelFromAdjustment(-100)).toBe(0);
    expect(presetLevelFromAdjustment(-50)).toBe(0.5);
    expect(presetLevelFromAdjustment(100)).toBe(2);
    expect(presetLevelAdjustment(0.5)).toBe(-50);
    expect(presetLevelAdjustment(1.5)).toBe(50);
  });

  it("clamps slider positions outside the supported range", () => {
    expect(presetLevelFromAdjustment(-150)).toBe(0);
    expect(presetLevelFromAdjustment(140)).toBe(2);
  });

  it("formats slider positions as signed percents in the reader's locale", () => {
    expect(formatPresetLevelAdjustment("en", 25)).toBe("+25%");
    expect(formatPresetLevelAdjustment("en", -100)).toBe("-100%");
    expect(formatPresetLevelAdjustment("en", 0)).toBe("0%");
    expect(formatPresetLevelAdjustment("es", 25)).toBe("+25\u00a0%");
  });
});

describe("soundscape preset names", () => {
  const t = ((key: string) => `translated:${key}`) as Translate;

  it("localizes seeded rain presets until the listener renames them", () => {
    expect(presetDisplayName({ id: "preset-generated-pink-noise", name: "Rain (moderate)" }, t)).toBe("translated:music.soundscape.rainName.pink");
    expect(presetDisplayName({ id: "preset-generated-pink-noise", name: "Desk rain" }, t)).toBe("Desk rain");
    expect(presetDisplayName({ id: "evening", name: "Rain (moderate)" }, t)).toBe("Rain (moderate)");
  });
});

describe("soundscape layer names", () => {
  const t = ((key: string) => `translated:${key}`) as Translate;

  it("names generated layers by their localized noise and local layers by their stored name", () => {
    expect(soundDisplayName({ ...sound("generated-brown-noise", "Brown noise"), sourceKind: "generated-noise", generatedKind: "brown", localPath: null }, t)).toBe("translated:music.soundscape.generatedName.brown");
    expect(soundDisplayName(sound("thunder", "Thunder"), t)).toBe("Thunder");
  });
});
