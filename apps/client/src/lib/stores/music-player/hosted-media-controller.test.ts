import { describe, expect, it, vi } from "vitest";
import { localFileSourceFromPath } from "$lib/music/sources";
import {
  createMusicHostedMediaController,
  type MusicHostedMediaState,
} from "./hosted-media-controller";

describe("Music hosted media controller", () => {
  it("unregisters only the stale load generation", async () => {
    const state: MusicHostedMediaState = {
      currentSource: localFileSourceFromPath("/music/a.mp3", "a"),
      localMediaSrc: "http://media/a",
      localHasVideo: false,
      currentArtworkUrl: "http://media/art-a",
      staleVisual: null,
    };
    const unregister = vi.fn(async () => undefined);
    const controller = createMusicHostedMediaController({
      state,
      loadedTitle: () => "a",
      retain: vi.fn(async () => undefined),
      unregister,
    });
    const staleGeneration = controller.nextGeneration();
    const currentGeneration = controller.nextGeneration();

    await controller.unregisterGeneration(
      ["http://media/a", "http://media/art-a"],
      staleGeneration,
    );

    expect(unregister).toHaveBeenCalledWith(
      ["http://media/a", "http://media/art-a"],
      staleGeneration,
    );
    expect(currentGeneration).toBeGreaterThan(staleGeneration);
  });

  it("retains stale artwork until the replacement paints", () => {
    const state: MusicHostedMediaState = {
      currentSource: localFileSourceFromPath("/music/a.mp3", "a"),
      localMediaSrc: null,
      localHasVideo: false,
      currentArtworkUrl: "http://media/art-a",
      staleVisual: null,
    };
    const controller = createMusicHostedMediaController({
      state,
      loadedTitle: () => "Track A",
      retain: vi.fn(async () => undefined),
      unregister: vi.fn(async () => undefined),
    });

    expect(controller.startVisualTransition()).toBe(true);
    expect(state.staleVisual).toEqual({
      kind: "image",
      url: "http://media/art-a",
      title: "Track A",
    });
    expect(controller.currentUrls()).toContain("http://media/art-a");
  });
});
