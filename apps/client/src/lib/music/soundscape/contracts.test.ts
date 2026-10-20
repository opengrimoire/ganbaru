import { describe, expect, it } from "vitest";
import { parseMusicSoundscapeGroups, parseMusicSoundscapePresets, parseMusicSoundscapeSnapshot, parseMusicSoundscapes, parseMusicSoundscapeState } from "./contracts";
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
      groupId: null,
      availability: "available",
      localPath: null,
      createdAtMs: 1,
      updatedAtMs: 1,
      version: 1,
    }])).toHaveLength(1);
  });

  it("rejects unknown sources and malformed icons", () => {
    const definition = { id: "x", sourceKind: "youtube", generatedKind: null, bundledIdentity: null, name: "X", icon: "lucide:audio-lines", groupId: null, availability: "available", localPath: null, createdAtMs: 1, updatedAtMs: 1, version: 1 };
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
    const group = { id: "rain", name: "Rain", icon: "lucide:cloud-rain", createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(parseMusicSoundscapeGroups([group])[0].name).toBe("Rain");
    expect(parseMusicSoundscapeGroups([{ ...group, icon: "emoji:🌧️" }])[0].icon).toBe("emoji:🌧️");
    expect(() => parseMusicSoundscapeGroups([{ ...group, icon: "unknown" }])).toThrow("icon");
    const definitions = ["white", "brown", "pink"].map((generatedKind) => ({
      id: generatedKind, sourceKind: "generated-noise", generatedKind, bundledIdentity: null,
      name: generatedKind, icon: "lucide:audio-lines", groupId: null, availability: "available", localPath: null,
      createdAtMs: 1, updatedAtMs: 1, version: 1,
    }));
    expect(orderedGeneratedSounds(parseMusicSoundscapes(definitions)).map((entry) => entry.generatedKind)).toEqual(["brown", "pink", "white"]);
  });

  it("parses preset mixes in stored order, keeps emptied presets, and rejects repeats and out-of-range levels", () => {
    const preset = { id: "evening", name: "Evening", icon: "lucide:moon", level: 0.5, sounds: [{ soundscapeId: "rain", level: 1 }, { soundscapeId: "fire", level: 0.3 }], createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(parseMusicSoundscapePresets([preset])[0].sounds).toEqual(preset.sounds);
    expect(parseMusicSoundscapePresets([{ ...preset, sounds: [] }])[0].sounds).toEqual([]);
    expect(parseMusicSoundscapePresets([{ ...preset, level: 2 }])[0].level).toBe(2);
    expect(() => parseMusicSoundscapePresets([{ ...preset, sounds: [preset.sounds[0], preset.sounds[0]] }])).toThrow("repeats");
    expect(() => parseMusicSoundscapePresets([{ ...preset, sounds: Array.from({ length: 17 }, (_, index) => ({ soundscapeId: `sound-${index}`, level: 1 })) }])).toThrow("sounds");
    expect(() => parseMusicSoundscapePresets([{ ...preset, level: 2.1 }])).toThrow("level");
    expect(() => parseMusicSoundscapePresets([{ ...preset, sounds: [{ soundscapeId: "rain", level: 1.2 }] }])).toThrow("level");
    expect(() => parseMusicSoundscapePresets([{ ...preset, level: Number.NaN }])).toThrow("level");
  });
});
