import { beforeEach, describe, expect, it, vi } from "vitest";

const { getYouTubeThumbnail, loadArtworkDataUrl, loadEmbeddedArtworkDataUrl } = vi.hoisted(() => ({
  getYouTubeThumbnail: vi.fn(async (videoId: string) => `data:image/jpeg,${videoId}`),
  loadArtworkDataUrl: vi.fn(async (path: string) => `data:image/mock,${path}`),
  loadEmbeddedArtworkDataUrl: vi.fn(async (path: string) => `data:image/embedded,${path}`),
}));

vi.mock("$lib/api/music", () => ({ getYouTubeThumbnail, loadArtworkDataUrl, loadEmbeddedArtworkDataUrl }));

import {
  clearMusicArtworkCache,
  musicArtworkDataUrl,
  musicEmbeddedArtworkDataUrl,
  musicYouTubeThumbnailDataUrl,
} from "./artwork-cache";

describe("music artwork cache", () => {
  beforeEach(() => {
    clearMusicArtworkCache();
    loadArtworkDataUrl.mockClear();
    loadEmbeddedArtworkDataUrl.mockClear();
    getYouTubeThumbnail.mockClear();
  });

  it("retains at most 96 artwork requests and evicts the least recently used entry", async () => {
    for (let index = 0; index < 97; index += 1) {
      await musicArtworkDataUrl(`/artwork/${index}.jpg`);
    }
    expect(loadArtworkDataUrl).toHaveBeenCalledTimes(97);

    await musicArtworkDataUrl("/artwork/96.jpg");
    expect(loadArtworkDataUrl).toHaveBeenCalledTimes(97);

    await musicArtworkDataUrl("/artwork/0.jpg");
    expect(loadArtworkDataUrl).toHaveBeenCalledTimes(98);
  });

  it("bounds prefetched embedded covers independently", async () => {
    for (let index = 0; index < 13; index += 1) {
      await musicEmbeddedArtworkDataUrl(`/music/${index}.flac`);
    }
    expect(loadEmbeddedArtworkDataUrl).toHaveBeenCalledTimes(13);

    await musicEmbeddedArtworkDataUrl("/music/12.flac");
    expect(loadEmbeddedArtworkDataUrl).toHaveBeenCalledTimes(13);

    await musicEmbeddedArtworkDataUrl("/music/0.flac");
    expect(loadEmbeddedArtworkDataUrl).toHaveBeenCalledTimes(14);
  });

  it("shares embedded cover extraction across tracks with the same artwork identity", async () => {
    const first = await musicEmbeddedArtworkDataUrl("/music/album/track-1.flac", "embedded:sha256:cover");
    const second = await musicEmbeddedArtworkDataUrl("/music/album/track-2.flac", "embedded:sha256:cover");

    expect(first).toBe(second);
    expect(loadEmbeddedArtworkDataUrl).toHaveBeenCalledOnce();
  });

  it("shares visible YouTube requests and bounds their memory cache", async () => {
    const first = musicYouTubeThumbnailDataUrl("abcDEF_1234");
    const second = musicYouTubeThumbnailDataUrl("abcDEF_1234");
    expect(first).toBe(second);
    await first;
    expect(getYouTubeThumbnail).toHaveBeenCalledOnce();

    for (let index = 0; index < 96; index += 1) {
      await musicYouTubeThumbnailDataUrl(String(index).padStart(11, "0"));
    }
    await musicYouTubeThumbnailDataUrl("abcDEF_1234");
    expect(getYouTubeThumbnail).toHaveBeenCalledTimes(98);
  });

  it("rechecks native storage when a long-running thumbnail memory entry expires", async () => {
    const now = Date.now();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      await musicYouTubeThumbnailDataUrl("abcDEF_1234");
      clock.mockReturnValue(now + 21 * 60 * 60 * 1000);
      await musicYouTubeThumbnailDataUrl("abcDEF_1234");
      expect(getYouTubeThumbnail).toHaveBeenCalledTimes(2);
    } finally {
      clock.mockRestore();
    }
  });
});
