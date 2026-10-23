import { describe, expect, it } from "vitest";
import { parseMusicSoundscapePresets, presetWriteWire, parseMusicSoundscapeSnapshot, parseMusicSoundscapes, parseMusicSoundscapeState } from "./contracts";
import { orderedGeneratedSounds } from "./presentation";

describe("soundscape boundary contracts", () => {
  it("parses generated and device-local definitions", () => {
    expect(parseMusicSoundscapes([{
      id: "generated-pink-noise",
      sourceKind: "generated-noise",
      generatedKind: "pink",
      bundledIdentity: null,
      name: "Pink noise",
      icon: "lucide:audio-lines",
     
      availability: "available",
      localPath: null,
      createdAtMs: 1,
      updatedAtMs: 1,
      version: 1,
    }])).toHaveLength(1);
  });

  it("rejects unknown sources and malformed icons", () => {
    const definition = { id: "x", sourceKind: "youtube", generatedKind: null, bundledIdentity: null, name: "X", icon: "lucide:audio-lines", availability: "available", localPath: null, createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(() => parseMusicSoundscapes([definition])).toThrow("sourceKind");
    expect(() => parseMusicSoundscapes([{ ...definition, sourceKind: "local-loop", icon: "invalid" }])).toThrow("icon");
  });

  it("parses the active preset and rejects playback without one", () => {
    const state = { automaticIntent: false, activePresetId: "rain", desiredPlaying: true, updatedAtMs: 1, version: 1 };
    expect(parseMusicSoundscapeState(state)).toEqual(state);
    expect(parseMusicSoundscapeState({ ...state, activePresetId: null, desiredPlaying: false }).activePresetId).toBeNull();
    expect(() => parseMusicSoundscapeState({ ...state, activePresetId: null })).toThrow("without a preset");
    expect(() => parseMusicSoundscapeState({ ...state, automaticIntent: "true" })).toThrow("automaticIntent");
  });

  it("parses native snapshots without exposing local paths", () => {
    expect(parseMusicSoundscapeSnapshot({ status: "playing", sourceId: "rain", errorCode: null })).toEqual({ status: "playing", sourceId: "rain", errorCode: null });
  });

  it("accepts picker icons, rejects malformed icons, and keeps generated sounds in intensity order", () => {
    const preset = { id: "rain", name: "Rain", icon: "lucide:cloud-rain", levelMilli: 1000, sounds: [], createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(parseMusicSoundscapePresets([preset])[0].icon).toBe("lucide:cloud-rain");
    expect(parseMusicSoundscapePresets([{ ...preset, icon: "emoji:🌧️" }])[0].icon).toBe("emoji:🌧️");
    expect(() => parseMusicSoundscapePresets([{ ...preset, icon: "unknown" }])).toThrow("icon");
    const definitions = ["white", "brown", "pink"].map((generatedKind) => ({
      id: generatedKind, sourceKind: "generated-noise", generatedKind, bundledIdentity: null,
      name: generatedKind, icon: "lucide:audio-lines", availability: "available", localPath: null,
      createdAtMs: 1, updatedAtMs: 1, version: 1,
    }));
    expect(orderedGeneratedSounds(parseMusicSoundscapes(definitions)).map((entry) => entry.generatedKind)).toEqual(["brown", "pink", "white"]);
  });

  it("parses preset mixes in stored order, keeps emptied presets, keeps layers merged past the cap, and rejects repeats and out-of-range levels", () => {
    const preset = { id: "evening", name: "Evening", icon: "lucide:moon", levelMilli: 500, sounds: [{ soundscapeId: "rain", levelMilli: 1000 }, { soundscapeId: "fire", levelMilli: 300 }], createdAtMs: 1, updatedAtMs: 1, version: 1 };
    const parsed = parseMusicSoundscapePresets([preset])[0];
    expect(parsed.level).toBe(0.5);
    expect(parsed.sounds).toEqual([{ soundscapeId: "rain", level: 1 }, { soundscapeId: "fire", level: 0.3 }]);
    expect(parseMusicSoundscapePresets([{ ...preset, sounds: [] }])[0].sounds).toEqual([]);
    expect(parseMusicSoundscapePresets([{ ...preset, levelMilli: 2000 }])[0].level).toBe(2);
    expect(() => parseMusicSoundscapePresets([{ ...preset, sounds: [preset.sounds[0], preset.sounds[0]] }])).toThrow("repeats");
    const merged = Array.from({ length: 17 }, (_, index) => ({ soundscapeId: `sound-${index}`, levelMilli: 1000 }));
    expect(parseMusicSoundscapePresets([{ ...preset, sounds: merged }])[0].sounds.map((sound) => sound.soundscapeId)).toEqual(merged.map((sound) => sound.soundscapeId));
    expect(() => parseMusicSoundscapePresets([{ ...preset, levelMilli: 2001 }])).toThrow("levelMilli");
    expect(() => parseMusicSoundscapePresets([{ ...preset, levelMilli: -1 }])).toThrow("levelMilli");
    expect(() => parseMusicSoundscapePresets([{ ...preset, sounds: [{ soundscapeId: "rain", levelMilli: 1001 }] }])).toThrow("levelMilli");
    expect(() => parseMusicSoundscapePresets([{ ...preset, levelMilli: 500.5 }])).toThrow("integer");
    expect(() => parseMusicSoundscapePresets([{ ...preset, levelMilli: undefined, level: 0.5 }])).toThrow("levelMilli");
  });

  it("serializes preset saves as rounded thousandths", () => {
    const wire = presetWriteWire({
      id: "evening", name: "Evening", icon: "lucide:moon", level: 1.2346, sounds: [{ soundscapeId: "rain", level: 0.3333 }],
      expectedVersion: 2, updatedAtMs: 10,
    });
    expect(wire).toEqual({
      id: "evening", name: "Evening", icon: "lucide:moon", levelMilli: 1235, sounds: [{ soundscapeId: "rain", levelMilli: 333 }],
      expectedVersion: 2, updatedAtMs: 10,
    });
    expect(wire).not.toHaveProperty("level");
  });
});
