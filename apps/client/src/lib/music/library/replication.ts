/** Replicated tables of the Music library, as sync names them in applied notices and recovery offers. */
export const MUSIC_SYNC_TABLES: ReadonlySet<string> = new Set([
  "music_local_roots",
  "music_library_items",
  "music_item_signals",
  "music_listening_statistics",
  "music_local_locations",
  "music_source_collections",
  "music_source_collection_items",
  "music_playlists",
  "music_playlist_intended_uses",
  "music_playlist_memberships",
  "music_snoozes",
  "music_soundscapes",
  "music_soundscape_presets",
  "music_soundscape_preset_sounds",
  "music_context_assignments",
]);

/** Replicated table whose recovery offers restore a deleted playlist as a new playlist. */
export const MUSIC_PLAYLISTS_TABLE = "music_playlists";
/** Replicated table whose recovery offers can only be discarded. */
export const MUSIC_ITEMS_TABLE = "music_library_items";

/** Whether sync applied rows of any Music table. */
export function touchesMusic(tables: readonly string[]): boolean {
  return tables.some((table) => MUSIC_SYNC_TABLES.has(table));
}

/** Subscribes to the tables sync applied, such as `listenSyncApplied`, and resolves to a function that unsubscribes. */
export type MusicSyncAppliedListen = (onApplied: (tables: string[]) => void) => Promise<() => void>;

/**
 * Runs `onChange` whenever sync applies Music rows from a linked device.
 *
 * @returns A function that stops listening, safe to call before the subscription resolves.
 */
export function listenForMusicReplication(listen: MusicSyncAppliedListen, onChange: () => void): () => void {
  let stopped = false;
  let unsubscribe: (() => void) | null = null;
  void listen((tables) => {
    if (!stopped && touchesMusic(tables)) onChange();
  }).then((stop) => {
    if (stopped) stop();
    else unsubscribe = stop;
  }, (error: unknown) => {
    console.warn("Music could not listen for sync", error);
  });
  return () => {
    stopped = true;
    unsubscribe?.();
    unsubscribe = null;
  };
}
