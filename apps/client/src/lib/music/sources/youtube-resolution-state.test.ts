import { describe, expect, it } from "vitest";
import type { MusicYouTubeSourcePreview } from "./youtube-source-resolver";
import {
  IDLE_YOUTUBE_RESOLUTION,
  isCurrentYouTubeResolution,
  transitionYouTubeResolution,
  type YouTubeResolutionState,
} from "./youtube-resolution-state";

const preview: MusicYouTubeSourcePreview = {
  kind: "youtube-video", videoId: "abcDEF_1234", playlistId: null, title: "Video", channel: "Channel",
  durationMs: 1_000, videoIds: ["abcDEF_1234"], videos: [], metadataTruncated: false, duplicateCount: 0,
};

function resolving(controller = new AbortController()): Extract<YouTubeResolutionState, { phase: "resolving" }> {
  return { phase: "resolving", controller };
}

describe("transitionYouTubeResolution", () => {
  it("starts from every phase and replaces an earlier outcome", () => {
    const controller = new AbortController();
    const states: YouTubeResolutionState[] = [
      IDLE_YOUTUBE_RESOLUTION,
      resolving(),
      { phase: "resolved", preview },
      { phase: "failed", error: "offline" },
    ];
    for (const state of states) {
      expect(transitionYouTubeResolution(state, { type: "started", controller })).toEqual({ phase: "resolving", controller });
    }
  });

  it("settles only the running request", () => {
    const current = resolving();
    const stale = new AbortController();
    expect(isCurrentYouTubeResolution(current, current.controller)).toBe(true);
    expect(isCurrentYouTubeResolution(current, stale)).toBe(false);
    expect(transitionYouTubeResolution(current, { type: "resolved", controller: stale, preview })).toBe(current);
    expect(transitionYouTubeResolution(current, { type: "failed", controller: stale, error: "x" })).toBe(current);
    expect(transitionYouTubeResolution(current, { type: "settled", controller: stale })).toBe(current);
  });

  it("records the running request's preview or error", () => {
    const current = resolving();
    expect(transitionYouTubeResolution(current, { type: "resolved", controller: current.controller, preview }))
      .toEqual({ phase: "resolved", preview });
    expect(transitionYouTubeResolution(current, { type: "failed", controller: current.controller, error: "offline" }))
      .toEqual({ phase: "failed", error: "offline" });
  });

  it("returns to idle when the running request settles without an outcome", () => {
    const current = resolving();
    expect(transitionYouTubeResolution(current, { type: "settled", controller: current.controller }))
      .toBe(IDLE_YOUTUBE_RESOLUTION);
  });

  it("ignores a settle or outcome once the request already finished", () => {
    const controller = new AbortController();
    const resolved: YouTubeResolutionState = { phase: "resolved", preview };
    expect(transitionYouTubeResolution(resolved, { type: "settled", controller })).toBe(resolved);
    expect(transitionYouTubeResolution(resolved, { type: "failed", controller, error: "late" })).toBe(resolved);
  });

  it("cancels only a running request and keeps a finished outcome", () => {
    expect(transitionYouTubeResolution(resolving(), { type: "cancelled" })).toBe(IDLE_YOUTUBE_RESOLUTION);
    const resolved: YouTubeResolutionState = { phase: "resolved", preview };
    const failed: YouTubeResolutionState = { phase: "failed", error: "offline" };
    expect(transitionYouTubeResolution(resolved, { type: "cancelled" })).toBe(resolved);
    expect(transitionYouTubeResolution(failed, { type: "cancelled" })).toBe(failed);
    expect(transitionYouTubeResolution(IDLE_YOUTUBE_RESOLUTION, { type: "cancelled" })).toBe(IDLE_YOUTUBE_RESOLUTION);
  });
});
