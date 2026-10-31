import type { SyncDomainPresentation, SyncTablePresentation } from "$lib/sync/presentation-registry";
import { publishQuickNotesChanged, QUICK_NOTES_TABLE, QUICK_NOTES_TABLES } from "./window-sync";

/** Notes surface conflicts and root recovery offers; tags and images use neutral sync copy. */
const NOTES_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("quickNotes.sync.conflicts", count),
  recovery: {
    untitled: (t) => t("quickNotes.sync.untitled"),
    restored: (t) => t("quickNotes.sync.restored"),
    restoreFailed: (t) => t("quickNotes.sync.restoreFailed"),
  },
};

/** How the sync settings present Quick notes. */
export const QUICK_NOTES_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("quickNotes.title"),
  tables: Object.fromEntries(
    [...QUICK_NOTES_TABLES].map((table) => [table, table === QUICK_NOTES_TABLE ? NOTES_PRESENTATION : {}]),
  ),
  changed: publishQuickNotesChanged,
};
