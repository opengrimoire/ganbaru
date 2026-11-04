import type { SyncDomainPresentation } from "$lib/sync/presentation-registry";
import { PREFERENCES_SYNC_TABLE, refreshConfigFromSync } from "./config";

/** Reread the preferences after rows changed outside the config bridge, such as a restored recovery offer. */
function refreshPreferences(): void {
  void refreshConfigFromSync().catch((error: unknown) => {
    console.error("Config refresh after a sync change failed", error);
  });
}

/** How the sync settings present portable preferences. Preference conflicts are never surfaced, so the table uses neutral sync copy. */
export const PREFERENCES_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("settings.title"),
  tables: { [PREFERENCES_SYNC_TABLE]: {} },
  changed: refreshPreferences,
};
