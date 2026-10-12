import type {
  LocalRootBinding,
  MusicInspectorDetail,
  MusicPlaylistSummary,
  MusicWeight,
} from "$lib/music/library/contracts";
import { musicArtworkDataUrl, musicEmbeddedArtworkDataUrl, musicYouTubeThumbnailDataUrl } from "$lib/music/library/artwork-cache";
import { orderMusicPlaylists } from "$lib/music/playlists/system";
import { resolveLocalMusicPath } from "$lib/music/platform-paths";
import { localFileSourceFromPath, parseMusicSourceInput, type MusicSource } from "./sources";

export function parseMusicReviewAutoplay(value: unknown): boolean {
  return value === true;
}

/** Uses newly learned playback metadata while a retained Review item is still stale. */
export function musicReviewDurationMs(
  liveDurationMs: number | null,
  learnedDurationMs: number | null,
  savedDurationMs: number | null,
): number {
  for (const durationMs of [liveDurationMs, learnedDurationMs, savedDurationMs]) {
    if (durationMs !== null && Number.isFinite(durationMs) && durationMs > 0) return durationMs;
  }
  return 0;
}

export function musicReviewSource(
  detail: MusicInspectorDetail,
  bindings: readonly LocalRootBinding[],
): MusicSource | null {
  const item = detail.item;
  if (item.sourceKind === "youtube-video" && item.youtubeVideoId) {
    return parseMusicSourceInput(`https://www.youtube.com/watch?v=${item.youtubeVideoId}`).source;
  }
  const availableLocation = detail.locations.find((location) => location.availability === "available");
  if (!availableLocation) return null;
  const folder = bindings.find((binding) => binding.rootId === availableLocation.rootId)?.folderPath;
  if (!folder) return null;
  const originalSidecar = item.originalArtworkIdentity?.startsWith("sidecar:")
    ? item.originalArtworkIdentity.slice("sidecar:".length)
    : null;
  const artworkPath = item.artworkOverride
    ?? (originalSidecar ? resolveLocalMusicPath(folder, originalSidecar) : null);
  return localFileSourceFromPath(
    resolveLocalMusicPath(folder, availableLocation.relativePath),
    item.titleOverride ?? item.originalTitle,
    artworkPath,
  );
}

/** Loads builder-only review artwork without starting another media decoder. */
export function musicReviewArtworkDataUrl(
  detail: MusicInspectorDetail,
  bindings: readonly LocalRootBinding[],
): Promise<string | null> {
  if (detail.item.artworkOverride) return musicArtworkDataUrl(detail.item.artworkOverride);
  if (detail.item.sourceKind === "youtube-video" && detail.item.youtubeVideoId) {
    return musicYouTubeThumbnailDataUrl(detail.item.youtubeVideoId);
  }
  const source = musicReviewSource(detail, bindings);
  if (!source || source.kind !== "local-file") return Promise.resolve(null);
  if (source.artworkPath) return musicArtworkDataUrl(source.artworkPath);
  if (detail.item.originalArtworkIdentity?.startsWith("embedded:")) {
    return musicEmbeddedArtworkDataUrl(source.path, detail.item.originalArtworkIdentity);
  }
  return Promise.resolve(null);
}

export function sortReviewPlaylists(
  playlists: readonly MusicPlaylistSummary[],
  search: string,
): MusicPlaylistSummary[] {
  const query = search.trim().toLocaleLowerCase();
  return orderMusicPlaylists(
    playlists.filter((playlist) => !query || playlist.name.toLocaleLowerCase().includes(query)),
  );
}

export function nextMusicWeight(weight: MusicWeight): MusicWeight {
  const weights: MusicWeight[] = ["rarely", "less-often", "normal", "more-often", "much-more-often"];
  return weights[(weights.indexOf(weight) + 1) % weights.length] ?? "normal";
}

export function isMusicReviewEditableTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement
    && Boolean(target.closest("input, textarea, select, [contenteditable='true']"));
}
