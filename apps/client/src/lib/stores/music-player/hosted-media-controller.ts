import { tick } from "svelte";
import {
  retainHostedMedia,
  unregisterHostedMedia,
} from "$lib/api/music";
import type { MusicSource } from "$lib/music/sources";

export type MusicStaleVisual = {
  kind: "image";
  url: string;
  title: string;
};

export interface MusicHostedMediaState {
  currentSource: MusicSource | null;
  localMediaSrc: string | null;
  localHasVideo: boolean;
  currentArtworkUrl: string | null;
  staleVisual: MusicStaleVisual | null;
}

interface MusicHostedMediaControllerOptions {
  state: MusicHostedMediaState;
  loadedTitle(): string;
  retain?: typeof retainHostedMedia;
  unregister?: typeof unregisterHostedMedia;
}

export interface MusicHostedMediaController {
  nextGeneration(): number;
  startVisualTransition(): boolean;
  finishVisualTransitionAfterPaint(): void;
  clearStaleVisual(): void;
  waitForStaleVisualPaint(): Promise<void>;
  currentUrls(): string[];
  retainCurrent(generation: number): Promise<void>;
  syncRetention(): void;
  unregisterGeneration(mediaUrls: string[], generation: number): Promise<void>;
  releaseAll(): void;
  destroy(): void;
}

/** Owns hosted-media generations and stale artwork retention as one lifecycle. */
export function createMusicHostedMediaController(
  options: MusicHostedMediaControllerOptions,
): MusicHostedMediaController {
  const retain = options.retain ?? retainHostedMedia;
  const unregister = options.unregister ?? unregisterHostedMedia;
  const state = options.state;
  let generation = 0;
  let staleVisualVersion = 0;
  let staleVisualClearTimeoutId: number | null = null;

  function nextGeneration(): number {
    generation += 1;
    return generation;
  }

  function cancelStaleVisualClear(): void {
    if (staleVisualClearTimeoutId === null || typeof window === "undefined") return;
    window.clearTimeout(staleVisualClearTimeoutId);
    staleVisualClearTimeoutId = null;
  }

  function currentVisualSnapshot(): MusicStaleVisual | null {
    if (!state.currentSource || state.currentSource.kind !== "local-file") return null;
    if (state.localHasVideo || !state.currentArtworkUrl) return null;
    return {
      kind: "image",
      url: state.currentArtworkUrl,
      title: options.loadedTitle(),
    };
  }

  function clearStaleVisual(): void {
    staleVisualVersion += 1;
    cancelStaleVisualClear();
    state.staleVisual = null;
  }

  function startVisualTransition(): boolean {
    const visual = currentVisualSnapshot();
    staleVisualVersion += 1;
    if (visual) {
      cancelStaleVisualClear();
      state.staleVisual = visual;
      return true;
    }
    if (state.staleVisual) {
      cancelStaleVisualClear();
      return true;
    }
    clearStaleVisual();
    return false;
  }

  function currentUrls(): string[] {
    return [...new Set([
      state.localMediaSrc,
      state.currentArtworkUrl,
      state.staleVisual?.url ?? null,
    ].filter((url): url is string => Boolean(url)))];
  }

  async function retainCurrent(hostedGeneration: number): Promise<void> {
    await retain(currentUrls(), hostedGeneration);
  }

  function syncRetention(): void {
    void retain(currentUrls(), nextGeneration()).catch(() => null);
  }

  function finishVisualTransition(): void {
    if (!state.staleVisual) return;
    cancelStaleVisualClear();
    state.staleVisual = null;
    syncRetention();
  }

  function finishVisualTransitionAfterPaint(): void {
    if (!state.staleVisual) return;
    const expectedVersion = staleVisualVersion;
    cancelStaleVisualClear();
    if (typeof window === "undefined") {
      finishVisualTransition();
      return;
    }
    staleVisualClearTimeoutId = window.setTimeout(() => {
      staleVisualClearTimeoutId = null;
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => {
          if (staleVisualVersion === expectedVersion) finishVisualTransition();
        });
      });
    }, 80);
  }

  async function waitForStaleVisualPaint(): Promise<void> {
    await tick();
    if (typeof window === "undefined") return;
    await new Promise<void>((resolve) => {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => resolve());
      });
    });
  }

  async function unregisterGeneration(
    mediaUrls: string[],
    hostedGeneration: number,
  ): Promise<void> {
    if (mediaUrls.length === 0) return;
    await unregister(mediaUrls, hostedGeneration).catch(() => null);
  }

  function releaseAll(): void {
    void retain([], nextGeneration()).catch(() => null);
  }

  function destroy(): void {
    clearStaleVisual();
    releaseAll();
  }

  return {
    nextGeneration,
    startVisualTransition,
    finishVisualTransitionAfterPaint,
    clearStaleVisual,
    waitForStaleVisualPaint,
    currentUrls,
    retainCurrent,
    syncRetention,
    unregisterGeneration,
    releaseAll,
    destroy,
  };
}
