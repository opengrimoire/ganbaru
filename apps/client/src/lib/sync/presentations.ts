import { CALENDAR_SYNC_PRESENTATION } from "$lib/calendar/sync-presentation";
import { MUSIC_SYNC_PRESENTATION } from "$lib/music/sync-presentation";
import { PROJECTS_SYNC_PRESENTATION } from "$lib/projects/sync-presentation";
import { QUICK_NOTES_SYNC_PRESENTATION } from "$lib/quick-notes/sync-presentation";
import { THEMES_SYNC_PRESENTATION } from "$lib/themes/sync-presentation";
import { PREFERENCES_SYNC_PRESENTATION } from "$lib/vault/sync-presentation";
import { SyncPresentationRegistry } from "./presentation-registry";

/** Sync presentation of the domains that replicate, one registration per domain. */
export const syncPresentations = new SyncPresentationRegistry()
  .register(QUICK_NOTES_SYNC_PRESENTATION)
  .register(PREFERENCES_SYNC_PRESENTATION)
  .register(THEMES_SYNC_PRESENTATION)
  .register(MUSIC_SYNC_PRESENTATION)
  .register(PROJECTS_SYNC_PRESENTATION)
  .register(CALENDAR_SYNC_PRESENTATION);
