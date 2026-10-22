import { QuickNoteImageError, readQuickNoteImage } from "$lib/api/quick-notes";
import type { QuickNoteImage, QuickNoteImageErrorCode } from "$lib/quick-notes/types";
import { listenForQuickNotesChanges } from "$lib/quick-notes/window-sync";

/** Longest edge of a card thumbnail, enough for wide cards on dense screens. */
export const QUICK_NOTE_THUMBNAIL_EDGE = 960;
/** Concurrent image reads, which keeps a long list of cards from decoding every image at once. */
const READ_CONCURRENCY = 2;
/** How long an image nobody shows keeps its URL, so scrolling back or reopening a note does not reload it. */
const RELEASE_DELAY_MS = 30_000;

/** `thumbnail` is a smaller copy for mosaics; `full` is the original file. */
export type QuickNoteImageVariant = "thumbnail" | "full";

export type QuickNoteImageState =
  | { status: "loading" }
  | { status: "ready"; url: string }
  | { status: "failed"; code: QuickNoteImageErrorCode };

export interface QuickNoteImageCacheDependencies {
  read(image: QuickNoteImage): Promise<ArrayBuffer>;
  /** Makes a smaller copy that fits `edge`; returns null when the image cannot be scaled. */
  thumbnail(blob: Blob, image: QuickNoteImage, edge: number): Promise<Blob | null>;
  createUrl(blob: Blob): string;
  revokeUrl(url: string): void;
  setTimeout(callback: () => void, delayMs: number): ReturnType<typeof setTimeout>;
  clearTimeout(handle: ReturnType<typeof setTimeout>): void;
  concurrency?: number;
  releaseDelayMs?: number;
}

export interface QuickNoteImageCache {
  /**
   * Shows an image: `listener` receives the current state now and on every change until the returned function is
   * called. Every subscription shares one read and one URL per image and variant.
   */
  subscribe(
    image: QuickNoteImage,
    variant: QuickNoteImageVariant,
    listener: (state: QuickNoteImageState) => void,
  ): () => void;
  /** The original file, for copying. */
  blob(image: QuickNoteImage): Promise<Blob>;
  /** Reads failed images that are still shown again, for example after a linked device sent their files. */
  retryFailed(): void;
}

interface Entry {
  image: QuickNoteImage;
  thumbnail: boolean;
  state: QuickNoteImageState;
  blob: Blob | null;
  listeners: Set<(state: QuickNoteImageState) => void>;
  releaseTimer: ReturnType<typeof setTimeout> | null;
  generation: number;
}

/** Whether an image is large enough that mosaics should show a smaller copy. Animated GIFs keep their frames. */
export function quickNoteImageNeedsThumbnail(image: QuickNoteImage): boolean {
  return image.contentType !== "image/gif" && Math.max(image.width, image.height) > QUICK_NOTE_THUMBNAIL_EDGE;
}

/** Size that fits `edge` and keeps the image shape. */
export function quickNoteThumbnailSize(image: QuickNoteImage, edge: number): { width: number; height: number } {
  const ratio = Math.min(1, edge / Math.max(image.width, image.height, 1));
  return {
    width: Math.max(1, Math.round(image.width * ratio)),
    height: Math.max(1, Math.round(image.height * ratio)),
  };
}

function errorCode(error: unknown): QuickNoteImageErrorCode {
  return error instanceof QuickNoteImageError ? error.code : "failed";
}

export function createQuickNoteImageCache(dependencies: QuickNoteImageCacheDependencies): QuickNoteImageCache {
  const concurrency = Math.max(1, dependencies.concurrency ?? READ_CONCURRENCY);
  const releaseDelayMs = Math.max(0, dependencies.releaseDelayMs ?? RELEASE_DELAY_MS);
  const entries = new Map<string, Entry>();
  const waiting: (() => void)[] = [];
  let active = 0;

  async function limited<T>(task: () => Promise<T>): Promise<T> {
    if (active >= concurrency) await new Promise<void>((resolve) => waiting.push(resolve));
    active += 1;
    try {
      return await task();
    } finally {
      active -= 1;
      waiting.shift()?.();
    }
  }

  function key(image: QuickNoteImage, variant: QuickNoteImageVariant): string {
    const thumbnail = variant === "thumbnail" && quickNoteImageNeedsThumbnail(image);
    return `${image.sha256}:${thumbnail ? "thumbnail" : "full"}`;
  }

  function publish(entry: Entry, state: QuickNoteImageState): void {
    entry.state = state;
    for (const listener of [...entry.listeners]) listener(state);
  }

  async function produce(entry: Entry): Promise<Blob> {
    const bytes = await dependencies.read(entry.image);
    const original = new Blob([bytes], { type: entry.image.contentType });
    if (!entry.thumbnail) return original;
    try {
      return await dependencies.thumbnail(original, entry.image, QUICK_NOTE_THUMBNAIL_EDGE) ?? original;
    } catch (error: unknown) {
      console.warn("quick note thumbnail could not be made", error);
      return original;
    }
  }

  function load(entryKey: string, entry: Entry): void {
    const generation = ++entry.generation;
    if (entry.state.status !== "loading") publish(entry, { status: "loading" });
    void limited(async () => {
      // An image nobody shows any more skips its read.
      if (entries.get(entryKey) !== entry || entry.generation !== generation) return;
      try {
        const blob = await produce(entry);
        if (entries.get(entryKey) !== entry || entry.generation !== generation) return;
        entry.blob = blob;
        publish(entry, { status: "ready", url: dependencies.createUrl(blob) });
      } catch (error: unknown) {
        if (entries.get(entryKey) !== entry || entry.generation !== generation) return;
        if (errorCode(error) !== "unavailable") console.warn("quick note image could not be read", error);
        publish(entry, { status: "failed", code: errorCode(error) });
      }
    });
  }

  function release(entryKey: string, entry: Entry): void {
    entry.releaseTimer = null;
    if (entry.listeners.size > 0 || entries.get(entryKey) !== entry) return;
    entries.delete(entryKey);
    entry.generation += 1;
    if (entry.state.status === "ready") dependencies.revokeUrl(entry.state.url);
  }

  return {
    subscribe(image, variant, listener) {
      const entryKey = key(image, variant);
      let entry = entries.get(entryKey);
      if (!entry) {
        entry = {
          image,
          thumbnail: entryKey.endsWith(":thumbnail"),
          state: { status: "loading" },
          blob: null,
          listeners: new Set(),
          releaseTimer: null,
          generation: 0,
        };
        entries.set(entryKey, entry);
        load(entryKey, entry);
      }
      const current = entry;
      if (current.releaseTimer !== null) {
        dependencies.clearTimeout(current.releaseTimer);
        current.releaseTimer = null;
      }
      current.listeners.add(listener);
      listener(current.state);
      return () => {
        if (!current.listeners.delete(listener) || current.listeners.size > 0) return;
        if (current.releaseTimer !== null) dependencies.clearTimeout(current.releaseTimer);
        current.releaseTimer = dependencies.setTimeout(() => release(entryKey, current), releaseDelayMs);
      };
    },
    async blob(image) {
      const full = entries.get(key(image, "full"));
      if (full?.blob) return full.blob;
      const bytes = await limited(() => dependencies.read(image));
      return new Blob([bytes], { type: image.contentType });
    },
    retryFailed() {
      for (const [entryKey, entry] of entries) {
        if (entry.state.status === "failed" && entry.listeners.size > 0) load(entryKey, entry);
      }
    },
  };
}

/** Scales an image through the browser decoder, which applies its orientation like the mosaics do. */
async function browserThumbnail(blob: Blob, image: QuickNoteImage, edge: number): Promise<Blob | null> {
  const url = URL.createObjectURL(blob);
  try {
    const element = new Image();
    element.decoding = "async";
    element.src = url;
    await element.decode();
    const size = quickNoteThumbnailSize(image, edge);
    const canvas = document.createElement("canvas");
    canvas.width = size.width;
    canvas.height = size.height;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingQuality = "high";
    context.drawImage(element, 0, 0, size.width, size.height);
    const type = image.contentType === "image/jpeg" ? "image/jpeg" : "image/webp";
    return await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, 0.86));
  } finally {
    URL.revokeObjectURL(url);
  }
}

let shared: QuickNoteImageCache | null = null;

/** The image cache every Quick notes view in this window shares. */
export function quickNoteImageCache(): QuickNoteImageCache {
  if (shared) return shared;
  const cache = createQuickNoteImageCache({
    read: readQuickNoteImage,
    thumbnail: browserThumbnail,
    createUrl: (blob) => URL.createObjectURL(blob),
    revokeUrl: (url) => URL.revokeObjectURL(url),
    setTimeout: (callback, delayMs) => setTimeout(callback, delayMs),
    clearTimeout: (handle) => clearTimeout(handle),
  });
  shared = cache;
  void listenForQuickNotesChanges(() => cache.retryFailed())
    .catch((error: unknown) => console.warn("quick note images cannot follow other windows", error));
  return cache;
}
