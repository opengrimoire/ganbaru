import type {
  SyncDomainPresentation,
  SyncRecoveryPresentation,
  SyncTablePresentation,
} from "$lib/sync/presentation-registry";
import { notifyNotesSyncChanged } from "./sync-events";

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

const PAGE_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.pageUntitled"),
  note: (t) => t("notes.sync.pageRecoveryNote"),
  restored: (t) => t("notes.sync.pageRestored"),
  restoreFailed: (t) => t("notes.sync.pageRestoreFailed"),
};

const BLOCK_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.blockUntitled"),
  note: (t) => t("notes.sync.blockRecoveryNote"),
  restored: (t) => t("notes.sync.blockRestored"),
  restoreFailed: (t) => t("notes.sync.blockRestoreFailed"),
};

const THREAD_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.threadUntitled"),
  note: (t) => t("notes.sync.threadRecoveryNote"),
  restored: (t) => t("notes.sync.threadRestored"),
  restoreFailed: (t) => t("notes.sync.threadRestoreFailed"),
};

const COMMENT_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.commentUntitled"),
  note: (t) => t("notes.sync.commentRecoveryNote"),
  restored: (t) => t("notes.sync.commentRestored"),
  restoreFailed: (t) => t("notes.sync.commentRestoreFailed"),
};

const TEMPLATE_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.templateUntitled"),
  note: (t) => t("notes.sync.templateRecoveryNote"),
  restored: (t) => t("notes.sync.templateRestored"),
  restoreFailed: (t) => t("notes.sync.templateRestoreFailed"),
};

/** A database is restored with the block that holds it, so an offer rooted in the database alone is only discarded. */
const DATABASE_RECOVERY: SyncRecoveryPresentation = {
  untitled: (t) => t("notes.sync.databaseUntitled"),
  note: (t) => t("notes.sync.databaseRecoveryNote"),
  restorable: false,
};

const TABLE_PRESENTATIONS: Readonly<Record<string, SyncTablePresentation>> = {
  notes_pages: { conflicts: (t, count) => t("notes.sync.pageConflicts", count), recovery: PAGE_RECOVERY },
  notes_blocks: { conflicts: (t, count) => t("notes.sync.blockConflicts", count), recovery: BLOCK_RECOVERY },
  notes_databases: { recovery: DATABASE_RECOVERY },
  notes_data_source_properties: { conflicts: (t, count) => t("notes.sync.propertyConflicts", count) },
  notes_page_property_values: { conflicts: (t, count) => t("notes.sync.valueConflicts", count) },
  notes_page_templates: {
    conflicts: (t, count) => t("notes.sync.templateConflicts", count),
    recovery: TEMPLATE_RECOVERY,
  },
  notes_data_source_templates: {
    conflicts: (t, count) => t("notes.sync.templateConflicts", count),
    recovery: TEMPLATE_RECOVERY,
  },
  notes_comment_threads: {
    conflicts: (t, count) => t("notes.sync.threadConflicts", count),
    recovery: THREAD_RECOVERY,
  },
  notes_comments: { conflicts: (t, count) => t("notes.sync.commentConflicts", count), recovery: COMMENT_RECOVERY },
};

/** How the sync settings present Notes. Tables without their own copy use neutral sync copy. */
export const NOTES_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("titleBar.tab.notes"),
  tables: Object.fromEntries([...NOTES_SYNC_TABLES].map((table) => [table, TABLE_PRESENTATIONS[table] ?? {}])),
  changed: notifyNotesSyncChanged,
};
