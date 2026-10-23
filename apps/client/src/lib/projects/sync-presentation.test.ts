import { describe, expect, it, vi } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { syncPresentations } from "$lib/sync/presentations";
import { PROJECTS_SYNC_PRESENTATION, PROJECTS_SYNC_TABLES } from "./sync-presentation";
import { onProjectsSyncChanged } from "./sync-events";
import { publishProjectsSyncChange } from "./window-sync";

vi.mock("./window-sync", () => ({ publishProjectsSyncChange: vi.fn() }));

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

/** Tables whose recovery offers the Projects restorers handle. */
const RECOVERABLE_TABLES = ["project_groups", "projects", "project_sections", "project_statuses", "project_tasks"];
/** Tables with conflicts the Projects views resolve. */
const CONFLICT_TABLES = ["projects", "project_tasks", "project_checklist_items", "project_task_dependencies"];

describe("projects sync presentation", () => {
  it("registers every replicated Projects table once", () => {
    expect(Object.keys(PROJECTS_SYNC_PRESENTATION.tables).sort()).toEqual([...PROJECTS_SYNC_TABLES].sort());
    expect(syncPresentations.labels(tEn)).toContain("Projects");
  });

  it("titles, explains, and confirms each recoverable table in both languages", () => {
    for (const t of [tEn, tEs]) {
      for (const table of RECOVERABLE_TABLES) {
        expect(syncPresentations.recoveryUntitled(t, table)).not.toBe(t("sync.recovery.untitled"));
        expect(syncPresentations.recoveryNote(t, table)).toBeTruthy();
        expect(syncPresentations.recoveryRestorable(table)).toBe(true);
        expect(syncPresentations.recoveryRestored(t, table)).not.toBe(t("sync.recovery.restored"));
        expect(syncPresentations.recoveryRestoreFailed(t, table)).not.toBe(t("sync.recovery.failed"));
      }
    }
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

  it("uses neutral copy for tables without their own", () => {
    expect(syncPresentations.conflictLine(tEn, "project_tags", 2)).toBe(tEn("sync.conflicts", 2));
    expect(syncPresentations.recoveryNote(tEn, "project_tags")).toBeNull();
  });

  it("tells this window's store and the other windows that rows changed", () => {
    vi.stubGlobal("window", new EventTarget());
    const listener = vi.fn();
    const stop = onProjectsSyncChanged(listener);
    try {
      PROJECTS_SYNC_PRESENTATION.changed();
    } finally {
      stop();
      vi.unstubAllGlobals();
    }
    expect(listener).toHaveBeenCalledTimes(1);
    expect(publishProjectsSyncChange).toHaveBeenCalledTimes(1);
  });
});
