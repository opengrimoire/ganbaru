// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesDataSourceCsvExportSaveResult } from "$lib/notes/types";
import NotesDatabaseCsvExportPanel from "./NotesDatabaseCsvExportPanel.svelte";

const notesApi = vi.hoisted(() => ({
  saveNotesDataSourceCsv: vi.fn<(dataSourceId: string) => Promise<NotesDataSourceCsvExportSaveResult>>(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/notes")>(),
  saveNotesDataSourceCsv: notesApi.saveNotesDataSourceCsv,
}));

function saved(rows: number): NotesDataSourceCsvExportSaveResult {
  return {
    saved: true,
    export: {
      object: "notes_data_source_csv_export",
      data_source_id: "source-a",
      database_id: "database-a",
      view_id: "view-a",
      scope: "view",
      file_name: "tasks.csv",
      csv: "",
      exported_row_count: rows,
      exported_property_count: 4,
      diagnostics: [],
    },
  };
}

describe("NotesDatabaseCsvExportPanel", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    notesApi.saveNotesDataSourceCsv.mockReset();
  });

  function exportButton(): HTMLButtonElement {
    const button = target?.querySelector<HTMLButtonElement>("button.bg-primary");
    if (!button) throw new Error("The export button did not render");
    return button;
  }

  it("shows an export outcome only for the view it exported", async () => {
    const props = $state({ dataSourceId: "source-a", databaseId: "database-a", viewId: "view-a" });
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesDatabaseCsvExportPanel, { target, props });
    notesApi.saveNotesDataSourceCsv.mockResolvedValueOnce(saved(7));

    exportButton().click();
    await vi.waitFor(() => expect(target?.textContent).toContain("7"));

    props.viewId = "view-b";
    flushSync();
    expect(target.textContent).not.toContain("7");

    let finish = (_result: NotesDataSourceCsvExportSaveResult) => {};
    notesApi.saveNotesDataSourceCsv.mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    exportButton().click();
    flushSync();
    props.viewId = "view-c";
    flushSync();
    finish(saved(9));
    await vi.waitFor(() => expect(exportButton().disabled).toBe(false));
    expect(target.textContent).not.toContain("9");
  });
});
