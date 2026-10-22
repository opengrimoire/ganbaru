import type { SyncDomainPresentation } from "$lib/sync/presentation-registry";
import { reloadUserThemes, THEMES_SYNC_TABLE } from "$lib/stores/theme.svelte";

/** Reload user themes after rows changed outside the theme store, such as a restored recovery offer. */
function refreshThemes(): void {
  void reloadUserThemes().catch((error: unknown) => {
    console.error("Theme reload after a sync change failed", error);
  });
}

/** How the sync settings present user themes. Theme conflicts are resolved in the theme editor. */
export const THEMES_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("settings.theme.themesHeading"),
  tables: {
    [THEMES_SYNC_TABLE]: {
      conflicts: (t, count) => t("theme.sync.conflicts", count),
      recovery: {
        untitled: (t) => t("theme.sync.untitled"),
        restored: (t) => t("theme.sync.restored"),
        restoreFailed: (t) => t("theme.sync.restoreFailed"),
      },
    },
  },
  changed: refreshThemes,
};
