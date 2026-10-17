import type { Translate } from "$lib/i18n/translator.svelte";
import type { MusicPlaylistSummary } from "$lib/music/library/contracts";

export const SYSTEM_MUSIC_PLAYLIST_IDS = [
  "playlist-default-start-of-day",
  "playlist-default-work-focus",
  "playlist-default-work-ganbare",
  "playlist-default-break-calm",
  "playlist-default-break-active",
  "playlist-default-meditate",
  "playlist-default-exercise",
  "playlist-default-hygiene",
  "playlist-default-chores",
  "playlist-default-cooking",
  "playlist-default-commute",
] as const;

export type SystemMusicPlaylistId = (typeof SYSTEM_MUSIC_PLAYLIST_IDS)[number];

export function isSystemMusicPlaylistId(id: string): id is SystemMusicPlaylistId {
  return SYSTEM_MUSIC_PLAYLIST_IDS.includes(id as SystemMusicPlaylistId);
}

export function orderMusicPlaylists(playlists: readonly MusicPlaylistSummary[]): MusicPlaylistSummary[] {
  return playlists.toSorted((left, right) => {
    return left.sortOrder - right.sortOrder || left.name.localeCompare(right.name) || left.id.localeCompare(right.id);
  });
}

export function systemMusicPlaylistName(id: string, storedName: string, t: Translate): string {
  if (id === "playlist-default-start-of-day") return t("music.builder.defaultPlaylist.startOfDay");
  if (id === "playlist-default-work-focus") return t("music.builder.defaultPlaylist.workFocus");
  if (id === "playlist-default-work-ganbare") return t("music.builder.defaultPlaylist.workGanbare");
  if (id === "playlist-default-break-calm") return t("music.builder.defaultPlaylist.breakCalm");
  if (id === "playlist-default-break-active") return t("music.builder.defaultPlaylist.breakActive");
  if (id === "playlist-default-meditate") return t("music.builder.defaultPlaylist.meditate");
  if (id === "playlist-default-exercise") return t("music.builder.defaultPlaylist.exercise");
  if (id === "playlist-default-hygiene") return t("music.builder.defaultPlaylist.hygiene");
  if (id === "playlist-default-chores") return t("music.builder.defaultPlaylist.chores");
  if (id === "playlist-default-cooking") return t("music.builder.defaultPlaylist.cooking");
  if (id === "playlist-default-commute") return t("music.builder.defaultPlaylist.commute");
  return storedName;
}

export function partitionMusicPlaylists(playlists: MusicPlaylistSummary[]): {
  defaults: MusicPlaylistSummary[];
  custom: MusicPlaylistSummary[];
} {
  const byId = new Map(playlists.map((playlist) => [playlist.id, playlist]));
  return {
    defaults: orderMusicPlaylists(SYSTEM_MUSIC_PLAYLIST_IDS.flatMap((id) => {
      const playlist = byId.get(id);
      return playlist ? [playlist] : [];
    })),
    custom: orderMusicPlaylists(playlists.filter((playlist) => !isSystemMusicPlaylistId(playlist.id))),
  };
}
