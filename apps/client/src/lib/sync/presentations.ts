import { QUICK_NOTES_SYNC_PRESENTATION } from "$lib/quick-notes/sync-presentation";
import { SyncPresentationRegistry } from "./presentation-registry";

/** Sync presentation of the domains that replicate, one registration per domain. */
export const syncPresentations = new SyncPresentationRegistry()
  .register(QUICK_NOTES_SYNC_PRESENTATION);
