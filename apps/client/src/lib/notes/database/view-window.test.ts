import { describe, expect, it } from "vitest";
import {
  MAX_RETAINED_DATABASE_ROWS,
  mergeNotesDatabaseBoardWindow,
  mergeNotesDatabaseRows,
} from "./view-window";
import type { NotesDataSourceBoardView, NotesPage } from "$lib/notes/types";

function row(id: string, title: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    parent: { type: "data_source_id", data_source_id: "source" },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: { title },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("Notes database view windows", () => {
  it("deduplicates a repeated keyset boundary and applies the authoritative row", () => {
    expect(mergeNotesDatabaseRows(
      [row("a", "Old"), row("b", "B")],
      [row("a", "New"), row("c", "C")],
    ).map((page) => [page.id, page.properties.title])).toEqual([
      ["a", "New"],
      ["b", "B"],
      ["c", "C"],
    ]);
  });

  it("keeps the newest keyset page within the mounted row cap", () => {
    const rows = Array.from({ length: 10_000 }, (_, index) => row(String(index), String(index)));
    expect(mergeNotesDatabaseRows(rows.slice(0, 9_920), rows.slice(9_920))).toEqual(
      rows.slice(-MAX_RETAINED_DATABASE_ROWS),
    );
  });

  it("applies one mounted row cap across board groups", () => {
    const view = (groups: Array<{ id: string; rows: NotesPage[] }>): NotesDataSourceBoardView => ({
      data_source: {
        object: "data_source",
        id: "source",
        parent: { type: "database_id", database_id: "database" },
        database_parent: { type: "page_id", page_id: "database-page" },
        title: "Tasks",
        title_rich_text: [],
        description: [],
        icon: null,
        properties: {},
        in_trash: false,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: "2026-01-01T00:00:00Z",
        last_edited_time: "2026-01-01T00:00:00Z",
      },
      view: {
        object: "view",
        id: "view",
        parent: { type: "database_id", database_id: "database" },
        data_source_id: "source",
        name: "Board",
        type: "board",
        filter: null,
        sorts: [],
        configuration: {},
        url: null,
        source_provider: null,
        source_object_id: null,
        source_workspace_id: null,
        source_last_edited_time: null,
        created_time: "2026-01-01T00:00:00Z",
        last_edited_time: "2026-01-01T00:00:00Z",
      },
      groups: groups.map((group) => ({ ...group, name: group.id, color: "default", hidden: false })),
      total_row_count: 10_000,
      group_counts: {},
      next_cursor: "next",
      has_more: true,
    });
    const first = view([
      { id: "a", rows: Array.from({ length: 160 }, (_, index) => row(`a${index}`, "A")) },
      { id: "b", rows: Array.from({ length: 80 }, (_, index) => row(`b${index}`, "B")) },
    ]);
    const next = view([
      { id: "a", rows: Array.from({ length: 40 }, (_, index) => row(`c${index}`, "C")) },
      { id: "b", rows: Array.from({ length: 40 }, (_, index) => row(`d${index}`, "D")) },
    ]);
    const merged = mergeNotesDatabaseBoardWindow(first, next);
    expect(merged.groups.flatMap((group) => group.rows)).toHaveLength(MAX_RETAINED_DATABASE_ROWS);
    expect(merged.groups.flatMap((group) => group.rows).at(-1)?.id).toBe("d39");
  });
});
