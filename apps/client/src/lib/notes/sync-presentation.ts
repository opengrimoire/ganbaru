import type {
  SyncDomainPresentation,
  SyncRecoveryPresentation,
  SyncTablePresentation,
} from "$lib/sync/presentation-registry";

/** Replicated Notes tables, as sync names them in applied notices, conflicts, and recovery offers. */
export const NOTES_SYNC_TABLES: ReadonlySet<string> = new Set([
  "notes_local_users",
  "notes_folders",
  "notes_pages",
  "notes_page_aliases",
  "notes_blocks",
  "notes_databases",
  "notes_data_sources",
  "notes_data_source_templates",
  "notes_page_templates",
  "notes_data_source_properties",
  "notes_data_source_property_options",
  "notes_page_property_values",
  "notes_relation_edges",
  "notes_database_views",
  "notes_comment_threads",
  "notes_comment_thread_anchors",
  "notes_comments",
  "notes_comment_reactions",
  "notes_suggestions",
  "notes_assets",
]);

/** Whether an applied sync notice names a table the Notes views show. */
export function isNotesSyncTable(table: string): boolean {
  return NOTES_SYNC_TABLES.has(table);
}

/** Notes has no restorers yet, so its recovery offers can only be discarded. */
const DISCARD_ONLY: Pick<SyncRecoveryPresentation, "note" | "restorable"> = {
  note: (t) => t("notes.sync.recoveryNote"),
  restorable: false,
};

const TABLE_PRESENTATIONS: Readonly<Record<string, SyncTablePresentation>> = {
  notes_pages: {
    conflicts: (t, count) => t("notes.sync.pageConflicts", count),
    recovery: { ...DISCARD_ONLY, untitled: (t) => t("notes.sync.pageUntitled") },
  },
  notes_blocks: {
    conflicts: (t, count) => t("notes.sync.blockConflicts", count),
    recovery: { ...DISCARD_ONLY, untitled: (t) => t("notes.sync.blockUntitled") },
  },
  notes_data_source_properties: { conflicts: (t, count) => t("notes.sync.propertyConflicts", count) },
  notes_page_property_values: { conflicts: (t, count) => t("notes.sync.valueConflicts", count) },
  notes_page_templates: { conflicts: (t, count) => t("notes.sync.templateConflicts", count) },
  notes_data_source_templates: { conflicts: (t, count) => t("notes.sync.templateConflicts", count) },
  notes_comment_threads: { conflicts: (t, count) => t("notes.sync.threadConflicts", count) },
  notes_comments: {
    conflicts: (t, count) => t("notes.sync.commentConflicts", count),
    recovery: {
      untitled: (t) => t("notes.sync.commentUntitled"),
      note: (t) => t("notes.sync.commentRecoveryNote"),
      restorable: false,
    },
  },
};

/** How the sync settings present Notes. Tables without their own copy use neutral sync copy. */
export const NOTES_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("titleBar.tab.notes"),
  tables: Object.fromEntries([...NOTES_SYNC_TABLES].map((table) => [table, TABLE_PRESENTATIONS[table] ?? {}])),
  // Open Notes views refresh from the applied notice; discarding a recovery offer changes no row they show.
  changed: () => undefined,
};
