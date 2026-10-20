// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicSoundscapeSnapshot, MusicSoundscapeState } from "$lib/music/soundscape/contracts";

const api = vi.hoisted(() => ({
  getDeviceId: vi.fn<() => Promise<string>>(),
  getMusicSoundscapes: vi.fn(),
  getMusicSoundscapeGroups: vi.fn(),
  getMusicSoundscapePresets: vi.fn(),
  getMusicSoundscapeState: vi.fn<() => Promise<MusicSoundscapeState>>(),
  getSoundscapeSnapshot: vi.fn<() => Promise<MusicSoundscapeSnapshot>>(),
  startSoundscape: vi.fn(),
  updateMusicSoundscapeState: vi.fn(),
  stopSoundscape: vi.fn(),
  pauseSoundscape: vi.fn(),
  resumeSoundscape: vi.fn(),
}));
vi.mock("$lib/api/soundscape", () => api);

const state: MusicSoundscapeState = {
  activeSoundscapeId: "noise", activeIds: ["noise"], multipleEnabled: false,
  generatedLevel: null, localLevel: null, desiredPlaying: true, automaticIntent: true,
  volume: 0.4, updatedAtMs: 1, version: 2,
};
const idle: MusicSoundscapeSnapshot = { status: "idle", sourceId: null, volume: 0.4, errorCode: null };

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  api.getDeviceId.mockResolvedValue("device");
  api.getMusicSoundscapes.mockResolvedValue([{
    id: "noise", sourceKind: "generated-noise", generatedKind: "white", availability: "available",
    localPath: null, bundledIdentity: null, name: "Noise", icon: "lucide:audio-lines",
    groupId: null, createdAtMs: 1, updatedAtMs: 1, version: 1,
  }]);
  api.getMusicSoundscapeGroups.mockResolvedValue([]);
  api.getMusicSoundscapePresets.mockResolvedValue([]);
  api.getMusicSoundscapeState.mockResolvedValue(state);
  api.getSoundscapeSnapshot.mockResolvedValue(idle);
});

describe("native background playback projection", () => {
  it("does not replay a persisted automatic selection when native phase admission has not started it", async () => {
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    expect(store.persisted?.automaticIntent).toBe(true);
    expect(store.snapshot.status).toBe("idle");
    expect(api.startSoundscape).not.toHaveBeenCalled();
    expect(api.updateMusicSoundscapeState).not.toHaveBeenCalled();
  });

  it("projects accepted output without replaying even a legacy manual selection already playing natively", async () => {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false });
    api.getSoundscapeSnapshot.mockResolvedValue({ ...idle, status: "playing", sourceId: "noise" });
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    expect(store.snapshot.sourceId).toBe("noise");
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });

  it("reports a multi-sound selection as an error when multiple sounds are disabled", async () => {
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, activeIds: ["noise", "rain"], automaticIntent: false, desiredPlaying: false });
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    await expect(store.togglePlayback()).resolves.toBeUndefined();
    expect(store.error).toBe("Multiple soundscape sounds are disabled.");
    expect(store.playbackPending).toBe(false);
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });

  it("drops an old native read after a vault switch and leaves output shutdown with the native owner", async () => {
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
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

  it("layers the available sounds of a preset even when multiple selection was off", async () => {
    const rain = { id: "rain", sourceKind: "local-loop", generatedKind: null, availability: "available", localPath: "/rain.ogg", bundledIdentity: null, name: "Rain", icon: "lucide:audio-lines", groupId: null, createdAtMs: 1, updatedAtMs: 1, version: 1 };
    const fire = { ...rain, id: "fire", availability: "missing", localPath: null };
    api.getMusicSoundscapes.mockResolvedValue([{ id: "noise", sourceKind: "generated-noise", generatedKind: "white", availability: "available", localPath: null, bundledIdentity: null, name: "Noise", icon: "lucide:audio-lines", groupId: null, createdAtMs: 1, updatedAtMs: 1, version: 1 }, rain, fire]);
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false, desiredPlaying: false });
    api.startSoundscape.mockResolvedValue({ ...idle, status: "playing", sourceId: "noise" });
    api.updateMusicSoundscapeState.mockImplementation(async (write: MusicSoundscapeState) => ({ ...state, ...write, automaticIntent: false, version: 3 }));
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    await store.playPreset({ soundscapeIds: ["noise", "fire", "rain"] });
    expect(api.startSoundscape.mock.calls[0][0]).toMatchObject({ sourceId: "noise", extraSources: [{ sourceId: "rain" }] });
    expect(api.updateMusicSoundscapeState).toHaveBeenCalledWith(expect.objectContaining({ activeIds: ["noise", "rain"], multipleEnabled: true, desiredPlaying: true }));
    expect(store.activePreset).toBeNull();
    store.presets = [{ id: "focus", name: "Focus", icon: "lucide:moon", soundscapeIds: ["rain", "noise"], createdAtMs: 1, updatedAtMs: 1, version: 1 }];
    expect(store.activePreset?.id).toBe("focus");
  });

  it("reports a preset with no playable sound without starting output", async () => {
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    await store.playPreset({ soundscapeIds: ["gone"] });
    expect(store.error).toBe("This sound needs to be repaired before it can play.");
    expect(api.startSoundscape).not.toHaveBeenCalled();
  });
});

describe("isolated preset preview", () => {
  const rain = { id: "rain", sourceKind: "local-loop", generatedKind: null, availability: "available", localPath: "/rain.ogg", bundledIdentity: null, name: "Rain", icon: "lucide:audio-lines", groupId: null, createdAtMs: 1, updatedAtMs: 1, version: 1 };
  const playing: MusicSoundscapeSnapshot = { ...idle, status: "playing", sourceId: "noise" };

  async function playingStore(snapshot: MusicSoundscapeSnapshot = playing) {
    api.getMusicSoundscapes.mockResolvedValue([{ ...rain, id: "noise", sourceKind: "generated-noise", generatedKind: "white", localPath: null, name: "Noise" }, rain]);
    api.getMusicSoundscapeState.mockResolvedValue({ ...state, automaticIntent: false, desiredPlaying: snapshot.status === "playing" });
    api.getSoundscapeSnapshot.mockResolvedValue(snapshot);
    api.pauseSoundscape.mockResolvedValue({ ...snapshot, status: "paused" });
    api.resumeSoundscape.mockResolvedValue(playing);
    api.stopSoundscape.mockResolvedValue(idle);
    api.startSoundscape.mockImplementation(async (request: { sourceId: string }) => ({ ...playing, sourceId: request.sourceId }));
    const { getSoundscapeStore } = await import("$lib/stores/soundscape.svelte");
    const store = getSoundscapeStore();
    await store.initialize();
    return store;
  }

  it("plays the draft and then restores the saved selection without writing it", async () => {
    const store = await playingStore();
    const saved = store.persisted;
    await store.beginPreview();
    expect(api.pauseSoundscape).toHaveBeenCalledOnce();
    await store.previewSounds(["rain"]);
    expect(api.startSoundscape).toHaveBeenLastCalledWith(expect.objectContaining({ sourceId: "rain", extraSources: [] }));
    await store.endPreview();
    expect(api.startSoundscape).toHaveBeenLastCalledWith(expect.objectContaining({ sourceId: "noise" }));
    expect(api.updateMusicSoundscapeState).not.toHaveBeenCalled();
    expect(store.persisted).toBe(saved);
    expect(store.previewing).toBe(false);
  });

  it("resumes the paused output when the preview never played anything", async () => {
    const store = await playingStore();
    await store.beginPreview();
    await store.previewSounds([]);
    await store.endPreview();
    expect(api.startSoundscape).not.toHaveBeenCalled();
    expect(api.resumeSoundscape).toHaveBeenCalledOnce();
  });

  it("stays silent afterwards when nothing was playing before", async () => {
    const store = await playingStore(idle);
    await store.beginPreview();
    expect(api.pauseSoundscape).not.toHaveBeenCalled();
    await store.previewSounds(["noise", "rain"]);
    await store.endPreview();
    expect(api.startSoundscape).toHaveBeenCalledOnce();
    expect(api.stopSoundscape).toHaveBeenCalledOnce();
    expect(api.resumeSoundscape).not.toHaveBeenCalled();
  });

  it("plays a request made before the preview began and coalesces rapid changes to the latest", async () => {
    const store = await playingStore();
    void store.previewSounds(["noise"]);
    const begun = store.beginPreview();
    void store.previewSounds(["noise", "rain"]);
    void store.previewSounds(["rain"]);
    await store.previewSounds(["rain"]);
    await begun;
    expect(api.startSoundscape).toHaveBeenCalledOnce();
    expect(api.startSoundscape).toHaveBeenCalledWith(expect.objectContaining({ sourceId: "rain" }));
  });
});
