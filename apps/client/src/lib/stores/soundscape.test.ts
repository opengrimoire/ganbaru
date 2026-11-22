import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MusicSoundscapeDefinition, MusicSoundscapePreset } from "$lib/music/soundscape/contracts";

const mocks = vi.hoisted(() => ({
  soundscapes: vi.fn<() => Promise<MusicSoundscapeDefinition[]>>(),
  presets: vi.fn<() => Promise<MusicSoundscapePreset[]>>(),
}));

vi.mock("$lib/api/soundscape", () => ({
  getMusicSoundscapes: mocks.soundscapes,
  getMusicSoundscapePresets: mocks.presets,
}));

const { getSoundscapeStore } = await import("./soundscape.svelte");

describe("soundscape store catalog loading", () => {
  beforeEach(() => {
    mocks.soundscapes.mockReset();
    mocks.presets.mockReset();
    mocks.presets.mockResolvedValue([]);
  });

  it("loads the catalog again after a vault switch cancels the read in flight", async () => {
    const store = getSoundscapeStore();
    let releaseFirst: (definitions: MusicSoundscapeDefinition[]) => void = () => undefined;
    mocks.soundscapes.mockImplementationOnce(() => new Promise((resolve) => { releaseFirst = resolve; }));
    const first = store.ensureCatalog();
    expect(store.loading).toBe(true);

    await store.switchVault(null);
    expect(store.loading).toBe(false);
    releaseFirst([]);
    await first;
    expect(store.loading).toBe(false);

    mocks.soundscapes.mockResolvedValueOnce([]);
    await store.ensureCatalog();
    expect(mocks.soundscapes).toHaveBeenCalledTimes(2);
    expect(store.catalogReady).toBe(true);
    expect(store.loading).toBe(false);
  });
});
