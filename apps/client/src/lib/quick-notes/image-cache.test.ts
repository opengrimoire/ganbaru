import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QuickNoteImageError } from "$lib/api/quick-notes";
import {
  createQuickNoteImageCache,
  QUICK_NOTE_THUMBNAIL_EDGE,
  quickNoteImageNeedsThumbnail,
  quickNoteThumbnailSize,
  type QuickNoteImageCacheDependencies,
  type QuickNoteImageState,
} from "./image-cache";
import type { QuickNoteImage } from "./types";

const RELEASE_DELAY_MS = 1_000;

function image(sha256: string, width = 400, height = 300, contentType: QuickNoteImage["contentType"] = "image/png"): QuickNoteImage {
  return { sha256, contentType, byteSize: 3, width, height };
}

interface Harness {
  dependencies: QuickNoteImageCacheDependencies;
  read: ReturnType<typeof vi.fn<QuickNoteImageCacheDependencies["read"]>>;
  thumbnail: ReturnType<typeof vi.fn<QuickNoteImageCacheDependencies["thumbnail"]>>;
  revoked: string[];
}

function harness(overrides: Partial<QuickNoteImageCacheDependencies> = {}): Harness {
  let urls = 0;
  const revoked: string[] = [];
  const read = vi.fn<QuickNoteImageCacheDependencies["read"]>(async () => new Uint8Array([1, 2, 3]).buffer);
  const thumbnail = vi.fn<QuickNoteImageCacheDependencies["thumbnail"]>(
    async () => new Blob([new Uint8Array([9])], { type: "image/webp" }),
  );
  const dependencies: QuickNoteImageCacheDependencies = {
    read,
    thumbnail,
    createUrl: () => `blob:${++urls}`,
    revokeUrl: (url) => revoked.push(url),
    setTimeout: (callback, delayMs) => setTimeout(callback, delayMs),
    clearTimeout: (handle) => clearTimeout(handle),
    releaseDelayMs: RELEASE_DELAY_MS,
    ...overrides,
  };
  return { dependencies, read, thumbnail, revoked };
}

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("Quick note thumbnails", () => {
  it("only scales large still images", () => {
    expect(quickNoteImageNeedsThumbnail(image("a", 4000, 3000))).toBe(true);
    expect(quickNoteImageNeedsThumbnail(image("a", 400, 300))).toBe(false);
    expect(quickNoteImageNeedsThumbnail(image("a", 4000, 3000, "image/gif"))).toBe(false);
  });

  it("keeps the image shape within the edge", () => {
    expect(quickNoteThumbnailSize(image("a", 4000, 2000), 1000)).toEqual({ width: 1000, height: 500 });
    expect(quickNoteThumbnailSize(image("a", 300, 6000), 1000)).toEqual({ width: 50, height: 1000 });
    expect(quickNoteThumbnailSize(image("a", 500, 400), 1000)).toEqual({ width: 500, height: 400 });
    expect(quickNoteThumbnailSize(image("a", 1, 100_000), 1000)).toEqual({ width: 1, height: 1000 });
  });
});

describe("Quick note image cache", () => {
  it("shares one read and one URL between subscribers", async () => {
    const { dependencies, read } = harness();
    const cache = createQuickNoteImageCache(dependencies);
    const first: QuickNoteImageState[] = [];
    const second: QuickNoteImageState[] = [];
    cache.subscribe(image("a"), "thumbnail", (state) => first.push(state));
    cache.subscribe(image("a"), "full", (state) => second.push(state));
    await vi.advanceTimersByTimeAsync(0);

    expect(read).toHaveBeenCalledOnce();
    expect(first).toEqual([{ status: "loading" }, { status: "ready", url: "blob:1" }]);
    expect(second).toEqual(first);
  });

  it("scales large images for mosaics and keeps the original when scaling fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { dependencies, read, thumbnail } = harness();
    thumbnail.mockRejectedValueOnce(new Error("decode failed"));
    const cache = createQuickNoteImageCache(dependencies);
    const large = image("large", 4000, 3000);
    const states: QuickNoteImageState[] = [];
    cache.subscribe(large, "thumbnail", (state) => states.push(state));
    cache.subscribe(large, "full", () => undefined);
    await vi.advanceTimersByTimeAsync(0);

    expect(read).toHaveBeenCalledTimes(2);
    expect(thumbnail).toHaveBeenCalledWith(expect.any(Blob), large, QUICK_NOTE_THUMBNAIL_EDGE);
    expect(states.at(-1)?.status).toBe("ready");
    expect(warn).toHaveBeenCalledOnce();
  });

  it("keeps an unused image for the release delay before revoking its URL", async () => {
    const { dependencies, read, revoked } = harness();
    const cache = createQuickNoteImageCache(dependencies);
    const stop = cache.subscribe(image("a"), "full", () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    stop();
    await vi.advanceTimersByTimeAsync(RELEASE_DELAY_MS - 1);
    const again = cache.subscribe(image("a"), "full", () => undefined);
    await vi.advanceTimersByTimeAsync(RELEASE_DELAY_MS);
    expect(read).toHaveBeenCalledOnce();
    expect(revoked).toEqual([]);

    again();
    await vi.advanceTimersByTimeAsync(RELEASE_DELAY_MS);
    expect(revoked).toEqual(["blob:1"]);
    cache.subscribe(image("a"), "full", () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("reports read errors and reads still shown failures again on retry", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { dependencies, read } = harness();
    read.mockRejectedValueOnce(new QuickNoteImageError("unavailable", "file is on another device"));
    read.mockRejectedValueOnce(new Error("io"));
    const cache = createQuickNoteImageCache(dependencies);
    const missing: QuickNoteImageState[] = [];
    const broken: QuickNoteImageState[] = [];
    cache.subscribe(image("missing"), "full", (state) => missing.push(state));
    const stopBroken = cache.subscribe(image("broken"), "full", (state) => broken.push(state));
    await vi.advanceTimersByTimeAsync(0);

    expect(missing.at(-1)).toEqual({ status: "failed", code: "unavailable" });
    expect(broken.at(-1)).toEqual({ status: "failed", code: "failed" });
    expect(warn).toHaveBeenCalledOnce();

    stopBroken();
    cache.retryFailed();
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledTimes(3);
    expect(missing.slice(-2)).toEqual([{ status: "loading" }, { status: "ready", url: "blob:1" }]);
  });

  it("limits concurrent reads and skips reads nobody waits for", async () => {
    const pending = deferred<ArrayBuffer>();
    const { dependencies, read } = harness({ concurrency: 1, releaseDelayMs: 0 });
    read.mockReturnValueOnce(pending.promise);
    const cache = createQuickNoteImageCache(dependencies);
    cache.subscribe(image("a"), "full", () => undefined);
    const stopB = cache.subscribe(image("b"), "full", () => undefined);
    cache.subscribe(image("c"), "full", () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    expect(read).toHaveBeenCalledOnce();

    stopB();
    await vi.advanceTimersByTimeAsync(0);
    pending.resolve(new Uint8Array([1]).buffer);
    await vi.advanceTimersByTimeAsync(0);
    expect(read.mock.calls.map(([entry]) => entry.sha256)).toEqual(["a", "c"]);
  });

  it("returns the original file for copying from the cache or a fresh read", async () => {
    const { dependencies, read } = harness();
    const cache = createQuickNoteImageCache(dependencies);
    cache.subscribe(image("a"), "full", () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    const cached = await cache.blob(image("a"));
    expect(cached.type).toBe("image/png");
    expect(read).toHaveBeenCalledOnce();

    const fresh = await cache.blob(image("b", 400, 300, "image/jpeg"));
    expect(fresh.type).toBe("image/jpeg");
    expect(read).toHaveBeenCalledTimes(2);
  });
});
