// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import { resolveMusicYouTubeSource, type MusicYouTubeResolverDependencies } from "./youtube-source-resolver";
import { parseMusicSourceInput, youtubeVideoSourceFromId } from "$lib/music/sources";

function dependencies(
  message: Record<string, unknown>,
): Partial<MusicYouTubeResolverDependencies> {
  let listener: ((event: MessageEvent<unknown>) => void) | null = null;
  return {
    hostUrl: async () => "https://resolver.local/host",
    token: () => "token-1",
    createFrame: () => document.createElement("iframe"),
    addMessageListener: (next) => { listener = next; },
    removeMessageListener: () => { listener = null; },
    mountFrame: (frame) => {
      document.body.append(frame);
      const load = new URL(frame.src).searchParams.get("load");
      queueMicrotask(() => listener?.(new MessageEvent("message", {
        source: frame.contentWindow,
        data: { token: "token-1", load, ...message },
      })));
    },
    removeFrame: (frame) => frame.remove(),
    metadata: async () => ({ playlist: null, videos: [], truncated: false }),
  };
}

describe("YouTube source resolver", () => {
  it("resolves video metadata through the isolated trusted host", async () => {
    const source = youtubeVideoSourceFromId("abcDEF_1234");
    const preview = await resolveMusicYouTubeSource(source, new AbortController().signal, 1_000, dependencies({
      type: "ganbaru-ai-youtube-state",
      status: "paused",
      positionMs: 0,
      durationMs: 95_000,
      videoId: "abcDEF_1234",
      title: "Resolved title",
      channel: "Composer",
    }));
    expect(preview).toMatchObject({
      kind: "youtube-video",
      title: "Resolved title",
      channel: "Composer",
      videoIds: ["abcDEF_1234"],
    });
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("enriches playlist ids with playlist and video names", async () => {
    const parsed = parseMusicSourceInput("https://youtube.com/playlist?list=PLdR7m7PFLzQ5uOyE0psTM3pCleLxeKaCj");
    if (parsed.source?.kind !== "youtube-playlist") throw new Error("Expected a playlist source");
    const preview = await resolveMusicYouTubeSource(parsed.source, new AbortController().signal, 1_000, {
      ...dependencies({
        type: "ganbaru-ai-youtube-playlist",
        playlistId: "PLdR7m7PFLzQ5uOyE0psTM3pCleLxeKaCj",
        videoIds: ["01L4CFQdrWA", "O4iot2Jy_D0"],
        index: 0,
      }),
      metadata: async () => ({
        playlist: { videoId: null, title: "Made in Abyss complete OST", channel: "AsK_0" },
        videos: [
          { videoId: "01L4CFQdrWA", title: "Endless Embrace", channel: "MYTH & ROID - Topic" },
        ],
        truncated: false,
      }),
    });

    expect(preview.title).toBe("Made in Abyss complete OST");
    expect(preview.channel).toBe("AsK_0");
    expect(preview.videos).toEqual([
      expect.objectContaining({ videoId: "01L4CFQdrWA", title: "Endless Embrace", metadataResolved: true }),
      expect.objectContaining({ videoId: "O4iot2Jy_D0", title: "O4iot2Jy_D0", metadataResolved: false }),
    ]);
  });

  it("reports embedding restrictions and always removes the resolver frame", async () => {
    const source = youtubeVideoSourceFromId("abcDEF_1234");
    await expect(resolveMusicYouTubeSource(source, new AbortController().signal, 1_000, dependencies({
      type: "ganbaru-ai-youtube-error",
      code: 150,
    }))).rejects.toThrow("does not allow embedded playback");
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("does not mount a frame when already cancelled", async () => {
    const controller = new AbortController();
    controller.abort();
    const mountFrame = vi.fn();
    await expect(resolveMusicYouTubeSource(
      youtubeVideoSourceFromId("abcDEF_1234"),
      controller.signal,
      1_000,
      { ...dependencies({}), mountFrame },
    )).rejects.toThrow("cancelled");
    expect(mountFrame).not.toHaveBeenCalled();
  });
});
