// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  MusicSoundscapeDefinition,
  MusicSoundscapePreset,
  MusicSoundscapePresetWrite,
  MusicSoundscapeSnapshot,
  MusicSoundscapeState,
  MusicSoundscapeStateWrite,
} from "$lib/music/soundscape/contracts";

const api = vi.hoisted(() => ({
  getMusicSoundscapes: vi.fn(),
  getMusicSoundscapePresets: vi.fn(),
  getMusicSoundscapeState: vi.fn<() => Promise<MusicSoundscapeState>>(),
  getSoundscapeSnapshot: vi.fn<() => Promise<MusicSoundscapeSnapshot>>(),
  startSoundscape: vi.fn(),
  updateMusicSoundscapeState: vi.fn(),
  stopSoundscape: vi.fn(),
  pauseSoundscape: vi.fn(),
  resumeSoundscape: vi.fn(),
  setSoundscapeLevels: vi.fn(),
  setSoundscapePresetLevel: vi.fn(),
  saveMusicSoundscapePresetLevel: vi.fn(),
  upsertMusicSoundscapePreset: vi.fn(),
}));
vi.mock("$lib/api/soundscape", () => api);

const state: MusicSoundscapeState = {
  automaticIntent: true, activePresetId: "calm", desiredPlaying: true, updatedAtMs: 1, version: 2,
};
const idle: MusicSoundscapeSnapshot = { status: "idle", sourceId: null, errorCode: null };
const playing: MusicSoundscapeSnapshot = { ...idle, status: "playing", sourceId: "noise" };

function definition(id: string, overrides: Partial<MusicSoundscapeDefinition> = {}): MusicSoundscapeDefinition {
  return {
    id, sourceKind: "local-loop", generatedKind: null, availability: "available", localPath: `/${id}.ogg`,
    bundledIdentity: null, name: id, icon: "lucide:audio-lines", createdAtMs: 1, updatedAtMs: 1, version: 1,
    ...overrides,
  };
}

function preset(id: string, sounds: [string, number][], level = 0.5): MusicSoundscapePreset {
  return {
    id, name: id, icon: "lucide:layers", level, sounds: sounds.map(([soundscapeId, soundLevel]) => ({ soundscapeId, level: soundLevel })),
    createdAtMs: 1, updatedAtMs: 1, version: 1,
  };
}

const noise = definition("noise", { sourceKind: "generated-noise", generatedKind: "white", localPath: null });
const rain = definition("rain");
const fire = definition("fire", { availability: "missing", localPath: null });
const calm = preset("calm", [["noise", 1], ["fire", 1], ["rain", 0.4]], 0.8);

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  api.getMusicSoundscapes.mockResolvedValue([noise, rain, fire]);
  api.getMusicSoundscapePresets.mockResolvedValue([calm]);
  api.getMusicSoundscapeState.mockResolvedValue(state);
  api.getSoundscapeSnapshot.mockResolvedValue(idle);
  api.startSoundscape.mockImplementation(async (request: { sourceId: string }) => ({ ...playing, sourceId: request.sourceId }));
  api.pauseSoundscape.mockResolvedValue({ ...playing, status: "paused" });
  api.resumeSoundscape.mockResolvedValue(playing);
  api.stopSoundscape.mockResolvedValue(idle);
  api.updateMusicSoundscapeState.mockImplementation(async (write: MusicSoundscapeStateWrite) => ({
    ...state, activePresetId: write.activePresetId, desiredPlaying: write.desiredPlaying, automaticIntent: false, version: write.expectedVersion + 1,
  }));
  api.saveMusicSoundscapePresetLevel.mockImplementation(async (id: string, level: number, updatedAtMs: number) => ({ ...calm, id, level, updatedAtMs, version: 2 }));
  api.upsertMusicSoundscapePreset.mockImplementation(async (write: MusicSoundscapePresetWrite) => ({
    id: write.id, name: write.name, icon: write.icon, level: write.level, sounds: write.sounds,
    createdAtMs: write.updatedAtMs, updatedAtMs: write.updatedAtMs, version: 1,
  }));
});

afterEach(() => {
  vi.useRealTimers();
});

async function loadedStore() {
  const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
  const store = getSoundscapeStore();
  await store.initialize();
  return store;
}

describe("native soundscape playback projection", () => {
  it("does not replay a persisted automatic selection when native phase admission has not started it", async () => {
    const store = await loadedStore();
    expect(store.persisted?.automaticIntent).toBe(true);
    expect(store.snapshot.status).toBe("idle");
    expect(api.startSoundscape).not.toHaveBeenCalled();
    expect(api.updateMusicSoundscapeState).not.toHaveBeenCalled();
  });

  it("projects output that is already playing natively without starting it again", async () => {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false });
    api.getSoundscapeSnapshot.mockResolvedValue(playing);
    const store = await loadedStore();
    expect(store.snapshot.sourceId).toBe("noise");
    expect(store.activePreset?.id).toBe("calm");
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });

  it("keeps a manual preset chosen but silent after a launch and saves it as not playing", async () => {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false });
    const store = await loadedStore();
    expect(api.startSoundscape).not.toHaveBeenCalled();
    expect(api.updateMusicSoundscapeState).toHaveBeenCalledWith(expect.objectContaining({ activePresetId: "calm", desiredPlaying: false }));
    expect(store.activePreset?.id).toBe("calm");
    expect(store.persisted?.desiredPlaying).toBe(false);
    expect(store.error).toBeNull();
  });

  it("drops an old native read after a vault switch and leaves output shutdown with the native owner", async () => {
    const store = await loadedStore();
    let finish: ((value: MusicSoundscapeState) => void) | undefined;
    api.getMusicSoundscapeState.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const refresh = store.refreshAcceptedOutput();
    await store.switchVault(null);
    finish?.({ ...state, version: 3 });
    await refresh;
    expect(store.persisted).toBeNull();
    expect(store.snapshot.status).toBe("idle");
    expect(api.stopSoundscape).not.toHaveBeenCalled();
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });

  it("layers the available sounds of a preset with their mix levels and saves it as the active preset", async () => {
    const store = await loadedStore();
    await store.playPreset(calm);
    expect(api.startSoundscape).toHaveBeenCalledWith(expect.objectContaining({
      sourceId: "noise", level: 1, presetLevel: 0.8, extraSources: [expect.objectContaining({ sourceId: "rain", level: 0.4 })],
    }));
    expect(api.updateMusicSoundscapeState).toHaveBeenCalledWith(expect.objectContaining({ activePresetId: "calm", desiredPlaying: true }));
    expect(store.activePreset?.id).toBe("calm");
  });

  it("reports a preset with no playable sound without starting output", async () => {
    const store = await loadedStore();
    await store.playPreset(preset("gone", [["gone", 1]]));
    expect(store.error).toBe("This preset needs to be repaired before it can play.");
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });
});

describe("preset level relative to the Music volume", () => {
  it("moves the playing output at once and saves the settled level into the preset once", async () => {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false });
    api.getSoundscapeSnapshot.mockResolvedValue(playing);
    const store = await loadedStore();
    vi.useFakeTimers();
    await store.setPresetLevel(calm, 1.2);
    await store.setPresetLevel(calm, 1.5);
    expect(api.setSoundscapePresetLevel).toHaveBeenLastCalledWith(1.5);
    expect(store.activePreset?.level).toBe(1.5);
    expect(api.saveMusicSoundscapePresetLevel).not.toHaveBeenCalled();
    await vi.runAllTimersAsync();
    expect(api.saveMusicSoundscapePresetLevel).toHaveBeenCalledOnce();
    expect(api.saveMusicSoundscapePresetLevel).toHaveBeenCalledWith("calm", 1.5, expect.any(Number));
    expect(store.activePreset?.version).toBe(2);
  });

  it("clamps the level to the slider range", async () => {
    const store = await loadedStore();
    vi.useFakeTimers();
    await store.setPresetLevel(calm, 5);
    expect(store.presets[0].level).toBe(2);
    await store.setPresetLevel(calm, -1);
    expect(store.presets[0].level).toBe(0);
  });

  it("saves a preset that is not playing without touching output", async () => {
    const store = await loadedStore();
    vi.useFakeTimers();
    await store.setPresetLevel(calm, 0.3);
    await vi.runAllTimersAsync();
    expect(api.setSoundscapePresetLevel).not.toHaveBeenCalled();
    expect(api.saveMusicSoundscapePresetLevel).toHaveBeenCalledWith("calm", 0.3, expect.any(Number));
  });
});

describe("single sounds as one-sound presets", () => {
  it("creates the one-sound preset at the default level the first time a sound plays and pauses it on the next press", async () => {
    api.getMusicSoundscapePresets.mockResolvedValue([]);
    const store = await loadedStore();
    await store.playSound(rain, "Rain");
    expect(api.upsertMusicSoundscapePreset).toHaveBeenCalledWith(expect.objectContaining({
      name: "Rain", level: 1, sounds: [{ soundscapeId: "rain", level: 1 }], expectedVersion: null,
    }));
    expect(store.activePreset?.sounds).toEqual([{ soundscapeId: "rain", level: 1 }]);
    expect(store.snapshot.status).toBe("playing");
    await store.playSound(rain, "Rain");
    expect(api.upsertMusicSoundscapePreset).toHaveBeenCalledOnce();
    expect(api.pauseSoundscape).toHaveBeenCalledOnce();
  });

  it("reuses an existing one-sound preset instead of creating another", async () => {
    const solo = preset("solo", [["rain", 1]], 1.4);
    api.getMusicSoundscapePresets.mockResolvedValue([solo]);
    const store = await loadedStore();
    await expect(store.makePresetFromSound(rain, "Rain")).resolves.toEqual(solo);
    expect(api.upsertMusicSoundscapePreset).not.toHaveBeenCalled();
  });

  it("gives a one-layer soundscape the layer's own icon", async () => {
    api.getMusicSoundscapePresets.mockResolvedValue([]);
    const store = await loadedStore();
    const made = await store.makePresetFromSound(noise, "White noise");
    expect(made.icon).toBe(noise.icon);
  });
});

describe("isolated preset preview", () => {
  async function playingStore(snapshot: MusicSoundscapeSnapshot = playing) {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false, desiredPlaying: snapshot.status === "playing" });
    api.getSoundscapeSnapshot.mockResolvedValue(snapshot);
    return loadedStore();
  }

  const rainOnly = { sounds: [{ soundscapeId: "rain", level: 1 }], level: 0.5 };

  it("plays the draft and then restores the saved preset without writing it", async () => {
    const store = await playingStore();
    const saved = store.persisted;
    await store.beginPreview();
    expect(api.pauseSoundscape).toHaveBeenCalledOnce();
    await store.previewMix(rainOnly);
    expect(api.startSoundscape).toHaveBeenLastCalledWith(expect.objectContaining({ sourceId: "rain", extraSources: [], presetLevel: 0.5 }));
    await store.endPreview();
    expect(api.startSoundscape).toHaveBeenLastCalledWith(expect.objectContaining({ sourceId: "noise", presetLevel: 0.8 }));
    expect(api.updateMusicSoundscapeState).not.toHaveBeenCalled();
    expect(store.persisted).toBe(saved);
    expect(store.previewing).toBe(false);
  });

  it("adjusts levels of the same sounds without restarting the preview", async () => {
    const store = await playingStore();
    await store.beginPreview();
    await store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }, { soundscapeId: "rain", level: 1 }], level: 0.5 });
    await store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }, { soundscapeId: "rain", level: 0.2 }], level: 0.5 });
    await store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }, { soundscapeId: "rain", level: 0.2 }], level: 1.1 });
    expect(api.startSoundscape).toHaveBeenCalledOnce();
    expect(api.setSoundscapeLevels).toHaveBeenCalledExactlyOnceWith([1, 0.2]);
    expect(api.setSoundscapePresetLevel).toHaveBeenCalledExactlyOnceWith(1.1);
  });

  it("restarts the preview when the sounds change", async () => {
    const store = await playingStore();
    await store.beginPreview();
    await store.previewMix(rainOnly);
    await store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }], level: 0.5 });
    expect(api.startSoundscape).toHaveBeenCalledTimes(2);
    expect(api.startSoundscape).toHaveBeenLastCalledWith(expect.objectContaining({ sourceId: "noise" }));
  });

  it("resumes the paused output when the preview never played anything", async () => {
    const store = await playingStore();
    await store.beginPreview();
    await store.previewMix(null);
    await store.endPreview();
    expect(api.startSoundscape).not.toHaveBeenCalled();
    expect(api.resumeSoundscape).toHaveBeenCalledOnce();
  });

  it("stays silent afterwards when nothing was playing before", async () => {
    const store = await playingStore(idle);
    await store.beginPreview();
    expect(api.pauseSoundscape).not.toHaveBeenCalled();
    await store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }, { soundscapeId: "rain", level: 1 }], level: 0.5 });
    await store.endPreview();
    expect(api.startSoundscape).toHaveBeenCalledOnce();
    expect(api.stopSoundscape).toHaveBeenCalledOnce();
    expect(api.resumeSoundscape).not.toHaveBeenCalled();
  });

  it("plays a request made before the preview began and coalesces rapid changes to the latest", async () => {
    const store = await playingStore();
    void store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }], level: 0.5 });
    const begun = store.beginPreview();
    void store.previewMix({ sounds: [{ soundscapeId: "noise", level: 1 }, { soundscapeId: "rain", level: 1 }], level: 0.5 });
    void store.previewMix(rainOnly);
    await store.previewMix(rainOnly);
    await begun;
    expect(api.startSoundscape).toHaveBeenCalledOnce();
    expect(api.startSoundscape).toHaveBeenCalledWith(expect.objectContaining({ sourceId: "rain" }));
  });

  it("keeps preset level changes away from the output while a preview plays", async () => {
    const store = await playingStore();
    await store.beginPreview();
    await store.previewMix(rainOnly);
    vi.useFakeTimers();
    await store.setPresetLevel(calm, 1.6);
    expect(api.setSoundscapePresetLevel).not.toHaveBeenCalled();
  });
});
