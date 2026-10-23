import type { SyncDomainPresentation, SyncTablePresentation } from "$lib/sync/presentation-registry";
import { notifyMusicLibraryChanged } from "$lib/music/library/events";
import { MUSIC_ITEMS_TABLE, MUSIC_PLAYLISTS_TABLE, MUSIC_SYNC_TABLES } from "$lib/music/library/replication";

/** A deleted playlist that another device kept editing returns as a new playlist with those songs. */
const PLAYLISTS_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("music.sync.playlistUntitled"),
    note: (t) => t("music.sync.playlistRecoveryNote"),
    restored: (t) => t("music.sync.playlistRestored"),
    restoreFailed: (t) => t("music.sync.playlistRestoreFailed"),
  },
};

/** A deleted library track can only be discarded: its edits never bring the track back. */
const ITEMS_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("music.sync.itemUntitled"),
    note: (t) => t("music.sync.itemRecoveryNote"),
    restorable: false,
    restored: (t) => t("music.sync.itemRestored"),
    restoreFailed: (t) => t("music.sync.itemRestoreFailed"),
  },
};

/** How the sync settings present the Music library. Other Music tables use neutral sync copy. */
export const MUSIC_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("music.title"),
  tables: Object.fromEntries(
    [...MUSIC_SYNC_TABLES].map((table) => [
      table,
      table === MUSIC_PLAYLISTS_TABLE ? PLAYLISTS_PRESENTATION : table === MUSIC_ITEMS_TABLE ? ITEMS_PRESENTATION : {},
    ]),
  ),
  changed: notifyMusicLibraryChanged,
};
