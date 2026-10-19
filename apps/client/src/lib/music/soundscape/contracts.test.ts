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

  it("rejects unknown sources and nonfinite volume", () => {
    const definition = { id: "x", sourceKind: "youtube", generatedKind: null, bundledIdentity: null, name: "X", icon: "lucide:audio-lines", groupId: null, availability: "available", localPath: null, createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(() => parseMusicSoundscapes([definition])).toThrow("sourceKind");
    expect(() => parseMusicSoundscapes([{ ...definition, sourceKind: "local-loop", icon: "invalid" }])).toThrow("icon");
    const state = { activeSoundscapeId: null, activeIds: [], multipleEnabled: false, generatedLevel: null, localLevel: null, desiredPlaying: false, volume: 0.1, updatedAtMs: 1, version: 1 };
    expect(() => parseMusicSoundscapeState({ ...state, volume: Number.NaN })).toThrow("volume");
    expect(parseMusicSoundscapeState({ ...state, generatedLevel: 1.25 }).generatedLevel).toBe(1.25);
    expect(parseMusicSoundscapeState({ ...state, automaticIntent: true }).automaticIntent).toBe(true);
    expect(parseMusicSoundscapeState(state).automaticIntent).toBe(false);
    expect(() => parseMusicSoundscapeState({ ...state, automaticIntent: "true" })).toThrow("automaticIntent");
    expect(() => parseMusicSoundscapeState({ ...state, localLevel: 2.1 })).toThrow("section level");
  });

  it("parses native snapshots without exposing local paths", () => {
    expect(parseMusicSoundscapeSnapshot({ status: "playing", sourceId: "rain", volume: 0.4, errorCode: null })).toEqual({ status: "playing", sourceId: "rain", volume: 0.4, errorCode: null });
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

  it("parses presets in their stored sound order, keeps emptied presets, and rejects repeated sets", () => {
    const preset = { id: "evening", name: "Evening", icon: "lucide:moon", soundscapeIds: ["rain", "fire"], createdAtMs: 1, updatedAtMs: 1, version: 1 };
    expect(parseMusicSoundscapePresets([preset])[0].soundscapeIds).toEqual(["rain", "fire"]);
    expect(parseMusicSoundscapePresets([{ ...preset, soundscapeIds: [] }])[0].soundscapeIds).toEqual([]);
    expect(() => parseMusicSoundscapePresets([{ ...preset, soundscapeIds: ["rain", "rain"] }])).toThrow("soundscapeIds");
    expect(() => parseMusicSoundscapePresets([{ ...preset, soundscapeIds: Array.from({ length: 17 }, (_, index) => `sound-${index}`) }])).toThrow("soundscapeIds");
  });
});
