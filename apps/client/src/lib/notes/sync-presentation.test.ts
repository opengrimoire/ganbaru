import { describe, expect, it, vi } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { syncPresentations } from "$lib/sync/presentations";
import { onNotesSyncChanged } from "./sync-events";
import { isNotesSyncTable, NOTES_SYNC_PRESENTATION, NOTES_SYNC_TABLES } from "./sync-presentation";

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

/** Tables with conflicts the Notes views resolve or the sync settings explain. */
const CONFLICT_TABLES = [
  "notes_pages",
  "notes_blocks",
  "notes_data_source_properties",
  "notes_page_property_values",
  "notes_page_templates",
  "notes_data_source_templates",
  "notes_comment_threads",
  "notes_comments",
];
/** Tables whose recovery offers restore through Notes, with their own titles, notes, and outcomes. */
const RESTORABLE_TABLES = [
  "notes_pages",
  "notes_blocks",
  "notes_comment_threads",
  "notes_comments",
  "notes_page_templates",
  "notes_data_source_templates",
];

describe("notes sync presentation", () => {
  it("registers every replicated Notes table once", () => {
    expect(Object.keys(NOTES_SYNC_PRESENTATION.tables).sort()).toEqual([...NOTES_SYNC_TABLES].sort());
    expect(syncPresentations.labels(tEn)).toContain("Notes");
  });

  it("recognizes Notes tables in applied notices and ignores other domains", () => {
    expect(isNotesSyncTable("notes_blocks")).toBe(true);
    expect(isNotesSyncTable("notes_comments")).toBe(true);
    expect(isNotesSyncTable("project_tasks")).toBe(false);
    expect(isNotesSyncTable("notes_page_history_snapshots")).toBe(false);
  });

  it("points each conflict table to where it is resolved, with plural forms", () => {
    for (const t of [tEn, tEs]) {
      for (const table of CONFLICT_TABLES) {
        const one = syncPresentations.conflictLine(t, table, 1);
        const many = syncPresentations.conflictLine(t, table, 3);
        expect(one).not.toBe(t("sync.conflicts", 1));
        expect(many).not.toBe(one);
        expect(many).toContain("3");
      }
    }
  });

  it("offers restorable Notes rows with their own title, note, and outcome in both languages", () => {
    for (const t of [tEn, tEs]) {
      for (const table of RESTORABLE_TABLES) {
        expect(syncPresentations.recoveryRestorable(table)).toBe(true);
        expect(syncPresentations.recoveryUntitled(t, table)).not.toBe(t("sync.recovery.untitled"));
        expect(syncPresentations.recoveryNote(t, table)).toBeTruthy();
        expect(syncPresentations.recoveryRestored(t, table)).not.toBe(t("sync.recovery.restored"));
        expect(syncPresentations.recoveryRestoreFailed(t, table)).not.toBe(t("sync.recovery.failed"));
      }
    }
  });

  it("offers a database on its own only for discarding, since it returns with its block", () => {
    expect(syncPresentations.recoveryRestorable("notes_databases")).toBe(false);
    for (const t of [tEn, tEs]) {
      expect(syncPresentations.recoveryUntitled(t, "notes_databases")).not.toBe(t("sync.recovery.untitled"));
      expect(syncPresentations.recoveryNote(t, "notes_databases")).toBeTruthy();
    }
  });

  it("tells this window's Notes views that rows changed", () => {
    vi.stubGlobal("window", new EventTarget());
    const listener = vi.fn();
    const stop = onNotesSyncChanged(listener);
    try {
      NOTES_SYNC_PRESENTATION.changed();
    } finally {
      stop();
      vi.unstubAllGlobals();
    }
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("uses neutral copy for tables without their own", () => {
    expect(syncPresentations.conflictLine(tEn, "notes_folders", 2)).toBe(tEn("sync.conflicts", 2));
    expect(syncPresentations.recoveryNote(tEn, "notes_folders")).toBeNull();
  });
});
