import { musicBuilderLoader } from "$lib/music/builder/loader";
import { onMusicLibraryChanged } from "$lib/music/library/events";
import { getMusicPlaylistSummaryCache } from "$lib/music/playlists/summary-cache.svelte";
import { getMusicSourcesController } from "$lib/music/sources/controller.svelte";
import {
  onActiveVaultIdentityChange,
  requireActiveVaultIdentity,
} from "$lib/vault/active-vault";

let activeLoad: Promise<void> | null = null;
let queuedVaultId: string | null = null;

async function preloadVault(vaultId: string): Promise<void> {
  const sources = getMusicSourcesController();
  const playlists = getMusicPlaylistSummaryCache();
  sources.setVault(vaultId);
  playlists.setVault(vaultId);
  await Promise.all([sources.load(), playlists.load()]);
  if (sources.firstUseSession) await musicBuilderLoader.load();
}

/** Preloads core Music data at app startup and follows active-vault changes. */
export function startMusicFirstUsePreload(): () => void {
  const start = (vaultId: string): void => {
    queuedVaultId = vaultId;
    if (activeLoad) return;
    activeLoad = (async () => {
      while (queuedVaultId) {
        const nextVaultId = queuedVaultId;
        queuedVaultId = null;
        try {
          await preloadVault(nextVaultId);
        } catch (error) {
          console.warn("Music first-use preload failed", error);
        }
      }
    })().finally(() => {
      activeLoad = null;
    });
  };
  try {
    start(requireActiveVaultIdentity());
  } catch {
    // Startup can mount before vault validation publishes the active identity.
  }
  const stopVaultListener = onActiveVaultIdentityChange((_previous, next) => {
    if (next) start(next);
  });
  const stopLibraryListener = onMusicLibraryChanged(() => {
    void getMusicPlaylistSummaryCache().refresh();
  });
  return () => {
    stopVaultListener();
    stopLibraryListener();
  };
}
