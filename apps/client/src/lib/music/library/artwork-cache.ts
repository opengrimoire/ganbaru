import { getYouTubeThumbnail, loadArtworkDataUrl, loadEmbeddedArtworkDataUrl } from "$lib/api/music";

const MAX_CACHED_ARTWORK = 96;
const MAX_CACHED_EMBEDDED_ARTWORK = 12;
const MAX_CACHED_YOUTUBE_THUMBNAILS = 96;
const YOUTUBE_MEMORY_CACHE_MS = 20 * 60 * 60 * 1000;
const YOUTUBE_MISSING_RETRY_MS = 10 * 60 * 1000;
const artworkCache = new Map<string, Promise<string | null>>();
const embeddedArtworkCache = new Map<string, Promise<string | null>>();
interface YouTubeThumbnailCacheEntry {
  promise: Promise<string | null>;
  expiresAt: number;
}
const youtubeThumbnailCache = new Map<string, YouTubeThumbnailCacheEntry>();

/** Loads a validated artwork image once and bounds retained data URLs. */
export function musicArtworkDataUrl(path: string): Promise<string | null> {
  const cached = artworkCache.get(path);
  if (cached) {
    artworkCache.delete(path);
    artworkCache.set(path, cached);
    return cached;
  }
  const request = loadArtworkDataUrl(path).catch(() => null);
  artworkCache.set(path, request);
  while (artworkCache.size > MAX_CACHED_ARTWORK) {
    const oldest = artworkCache.keys().next().value;
    if (typeof oldest !== "string") break;
    artworkCache.delete(oldest);
  }
  return request;
}

export function clearMusicArtworkCache(): void {
  artworkCache.clear();
  embeddedArtworkCache.clear();
  youtubeThumbnailCache.clear();
}

/** Reuses visible YouTube thumbnail requests while native storage handles disk expiry. */
export function musicYouTubeThumbnailDataUrl(videoId: string): Promise<string | null> {
  const cached = youtubeThumbnailCache.get(videoId);
  if (cached && cached.expiresAt > Date.now()) {
    youtubeThumbnailCache.delete(videoId);
    youtubeThumbnailCache.set(videoId, cached);
    return cached.promise;
  }
  const entry: YouTubeThumbnailCacheEntry = {
    promise: Promise.resolve(null),
    expiresAt: Date.now() + YOUTUBE_MEMORY_CACHE_MS,
  };
  entry.promise = getYouTubeThumbnail(videoId).then((url) => {
    entry.expiresAt = Date.now() + (url ? YOUTUBE_MEMORY_CACHE_MS : YOUTUBE_MISSING_RETRY_MS);
    return url;
  }).catch((error: unknown) => {
    console.warn(`Could not load YouTube thumbnail for ${videoId}`, error);
    entry.expiresAt = Date.now() + YOUTUBE_MISSING_RETRY_MS;
    return null;
  });
  youtubeThumbnailCache.set(videoId, entry);
  while (youtubeThumbnailCache.size > MAX_CACHED_YOUTUBE_THUMBNAILS) {
    const oldest = youtubeThumbnailCache.keys().next().value;
    if (typeof oldest !== "string") break;
    youtubeThumbnailCache.delete(oldest);
  }
  return entry.promise;
}

/** Extracts embedded artwork once and bounds retained builder preview data URLs. */
export function musicEmbeddedArtworkDataUrl(path: string, identity = path): Promise<string | null> {
  const cached = embeddedArtworkCache.get(identity);
  if (cached) {
    embeddedArtworkCache.delete(identity);
    embeddedArtworkCache.set(identity, cached);
    return cached;
  }
  const request = loadEmbeddedArtworkDataUrl(path).catch(() => null);
  embeddedArtworkCache.set(identity, request);
  while (embeddedArtworkCache.size > MAX_CACHED_EMBEDDED_ARTWORK) {
    const oldest = embeddedArtworkCache.keys().next().value;
    if (typeof oldest !== "string") break;
    embeddedArtworkCache.delete(oldest);
  }
  return request;
}
