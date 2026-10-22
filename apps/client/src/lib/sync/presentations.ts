import { QUICK_NOTES_SYNC_PRESENTATION } from "$lib/quick-notes/sync-presentation";
import { PREFERENCES_SYNC_PRESENTATION } from "$lib/vault/sync-presentation";
import { SyncPresentationRegistry } from "./presentation-registry";

/** Sync presentation of the domains that replicate, one registration per domain. */
export const syncPresentations = new SyncPresentationRegistry()
  .register(QUICK_NOTES_SYNC_PRESENTATION)
  .register(PREFERENCES_SYNC_PRESENTATION);
