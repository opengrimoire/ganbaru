import type {
  MusicItemAvailability,
  MusicItemListEntry,
  MusicReviewState,
} from "$lib/music/library/contracts";

export interface MusicItemSecondaryText {
  primary: string;
  secondary: string;
}

export function formatMusicDuration(durationMs: number | null): string {
  if (durationMs === null || !Number.isFinite(durationMs) || durationMs < 0) return "";
  const totalSeconds = Math.round(durationMs / 1_000);
  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const minutes = totalMinutes % 60;
  const hours = Math.floor(totalMinutes / 60);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function musicItemSecondaryText(
  item: MusicItemListEntry,
  unknownArtist: string,
  noAlbum: string,
): MusicItemSecondaryText {
  return {
    primary: item.artist.trim() || unknownArtist,
    secondary: item.album.trim() || noAlbum,
  };
}

export function musicAvailabilityTone(
  availability: MusicItemAvailability,
): "neutral" | "warning" | "danger" {
  if (availability === "missing" || availability === "unavailable") return "danger";
  if (availability === "ambiguous") return "warning";
  return "neutral";
}

export function musicReviewTone(
  reviewState: MusicReviewState,
): "neutral" | "accent" | "muted" {
  if (reviewState === "unreviewed") return "accent";
  if (reviewState === "ignored") return "muted";
  return "neutral";
}
