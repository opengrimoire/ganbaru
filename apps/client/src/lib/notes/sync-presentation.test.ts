import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { syncPresentations } from "$lib/sync/presentations";
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
/** Tables whose recovery offers have their own titles and notes. */
const RECOVERY_TABLES = ["notes_pages", "notes_blocks", "notes_comments"];

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

  it("offers Notes recovery as discard only, with a title and a note in both languages", () => {
    for (const t of [tEn, tEs]) {
      for (const table of RECOVERY_TABLES) {
        expect(syncPresentations.recoveryUntitled(t, table)).not.toBe(t("sync.recovery.untitled"));
        expect(syncPresentations.recoveryNote(t, table)).toBeTruthy();
        expect(syncPresentations.recoveryRestorable(table)).toBe(false);
      }
    }
  });

  it("explains that a comment removed with its block is listed under that block", () => {
    expect(syncPresentations.recoveryNote(tEn, "notes_comments")).toBe(tEn("notes.sync.commentRecoveryNote"));
  });

  it("uses neutral copy for tables without their own", () => {
    expect(syncPresentations.conflictLine(tEn, "notes_folders", 2)).toBe(tEn("sync.conflicts", 2));
    expect(syncPresentations.recoveryNote(tEn, "notes_folders")).toBeNull();
  });
});
