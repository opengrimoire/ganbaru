import { describe, expect, it } from "vitest";
import {
  moveNotesDatabaseRowPageProperty,
  notesDatabaseParentExclusions,
  notesDatabaseRowCanAddSubitems,
  notesDatabaseRowPageEntries,
  notesDatabaseRowPageValueIsEmpty,
  notesDatabaseRowSearch,
  notesDatabaseSubitemExclusions,
  orderNotesDatabaseRowPageProperties,
  stepNotesDatabaseRowPageProperty,
  withNotesDatabaseRowPagePositions,
  type NotesDatabaseRowPagePlacement,
} from "./row-page-properties";
import { NOTES_DATABASE_ROW_MAX_DEPTH, type NotesDatabaseRowHierarchyMetadata } from "./row-hierarchy";
import type { NotesDatabaseTableColumn } from "./table";
import type { NotesPage } from "$lib/notes/types";

function placement(id: string, pagePosition: number | null = null, pageVisibility: NotesDatabaseRowPagePlacement["pageVisibility"] = "always_show"): NotesDatabaseRowPagePlacement {
  return { id, type: id === "title" ? "title" : "rich_text", pageVisibility, pagePosition };
}

function ids(properties: readonly NotesDatabaseRowPagePlacement[]): string[] {
  return properties.map((property) => property.id);
}

describe("orderNotesDatabaseRowPageProperties", () => {
  it("follows the fallback order when nothing is positioned and leaves out the title", () => {
    expect(ids(orderNotesDatabaseRowPageProperties([placement("title"), placement("b"), placement("a")]))).toEqual(["b", "a"]);
  });

  it("lists positioned properties first by position, then the rest in fallback order", () => {
    const ordered = orderNotesDatabaseRowPageProperties([
      placement("title", 0),
      placement("a"),
      placement("b", 3),
      placement("c"),
      placement("d", 1),
    ]);
    expect(ids(ordered)).toEqual(["d", "b", "a", "c"]);
  });

  it("keeps fallback order between equal positions", () => {
    expect(ids(orderNotesDatabaseRowPageProperties([placement("b", 0), placement("a", 0)]))).toEqual(["b", "a"]);
  });
});

describe("notesDatabaseRowPageEntries", () => {
  const ordered = [placement("shown"), placement("hidden", null, "always_hide"), placement("empty", null, "hide_when_empty"), placement("filled", null, "hide_when_empty")];

  it("folds always hidden properties and empty hide when empty properties", () => {
    const entries = notesDatabaseRowPageEntries(ordered, (property) => property.id !== "filled");
    expect(entries.map(({ property, collapsed }) => [property.id, collapsed])).toEqual([
      ["shown", false],
      ["hidden", true],
      ["empty", true],
      ["filled", false],
    ]);
  });

  it("never folds always shown properties even when empty", () => {
    expect(notesDatabaseRowPageEntries([placement("a")], () => true)[0]?.collapsed).toBe(false);
  });
});

describe("withNotesDatabaseRowPagePositions", () => {
  it("gives each listed property its index and leaves the title alone", () => {
    const drafts = [placement("title"), placement("a", 4), placement("b"), placement("c")];
    const positioned = withNotesDatabaseRowPagePositions(drafts, ["c", "a", "b"]);
    expect(positioned.map((property) => [property.id, property.pagePosition])).toEqual([
      ["title", null],
      ["a", 1],
      ["b", 2],
      ["c", 0],
    ]);
    expect(drafts[1]?.pagePosition).toBe(4);
  });

  it("keeps the same object when a position does not change", () => {
    const drafts = [placement("a", 0)];
    expect(withNotesDatabaseRowPagePositions(drafts, ["a"])[0]).toBe(drafts[0]);
  });

  it("produces an order that reads back the same way", () => {
    const drafts = [placement("title"), placement("a"), placement("b", 0), placement("c")];
    const order = ids(orderNotesDatabaseRowPageProperties(drafts));
    const moved = moveNotesDatabaseRowPageProperty(order, "c", "b", "before");
    expect(moved).toEqual(["c", "b", "a"]);
    expect(ids(orderNotesDatabaseRowPageProperties(withNotesDatabaseRowPagePositions(drafts, moved ?? [])))).toEqual(["c", "b", "a"]);
  });
});

describe("stepNotesDatabaseRowPageProperty", () => {
  it("steps over folded properties without moving them", () => {
    expect(stepNotesDatabaseRowPageProperty(["a", "hidden", "b"], ["a", "b"], "b", -1)).toEqual(["b", "a", "hidden"]);
  });

  it("reports no move at either end", () => {
    expect(stepNotesDatabaseRowPageProperty(["a", "b"], ["a", "b"], "a", -1)).toBeNull();
    expect(stepNotesDatabaseRowPageProperty(["a", "b"], ["a", "b"], "b", 1)).toBeNull();
  });
});

const SOURCE_ID = "11111111-1111-4111-8111-111111111111";

function rowPage(properties: NotesPage["properties"]): NotesPage {
  return {
    object: "page",
    id: "55555555-5555-4555-8555-555555555555",
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T01:00:00.000Z",
    parent: { type: "data_source_id", data_source_id: SOURCE_ID },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties,
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function column(id: string, type: NotesDatabaseTableColumn["type"]): NotesDatabaseTableColumn {
  return { id, name: id, type, hidden: false, width: 180, options: [], relationDataSourceId: null, buttonLabel: "Run", buttonRequiresConfirmation: false };
}

describe("notesDatabaseRowPageValueIsEmpty", () => {
  it("treats unchecked and missing checkboxes as empty", () => {
    expect(notesDatabaseRowPageValueIsEmpty(rowPage({ Done: { id: "done", type: "checkbox", checkbox: false } }), column("done", "checkbox"))).toBe(true);
    expect(notesDatabaseRowPageValueIsEmpty(rowPage({}), column("done", "checkbox"))).toBe(true);
    expect(notesDatabaseRowPageValueIsEmpty(rowPage({ Done: { id: "done", type: "checkbox", checkbox: true } }), column("done", "checkbox"))).toBe(false);
  });

  it("treats blank text, no options, no relations, and no date as empty", () => {
    const page = rowPage({
      Notes: { id: "notes", type: "rich_text", rich_text: [{ type: "text", plain_text: "  " }] },
      Tags: { id: "tags", type: "multi_select", multi_select: [] },
      Links: { id: "links", type: "relation", relation: [] },
      Due: { id: "due", type: "date", date: null },
    });
    for (const [id, type] of [["notes", "rich_text"], ["tags", "multi_select"], ["links", "relation"], ["due", "date"]] as const) {
      expect(notesDatabaseRowPageValueIsEmpty(page, column(id, type))).toBe(true);
    }
  });

  it("treats values as filled", () => {
    const page = rowPage({
      Estimate: { id: "estimate", type: "number", number: 0 },
      Links: { id: "links", type: "relation", relation: [{ id: "row" }] },
    });
    expect(notesDatabaseRowPageValueIsEmpty(page, column("estimate", "number"))).toBe(false);
    expect(notesDatabaseRowPageValueIsEmpty(page, column("links", "relation"))).toBe(false);
  });

  it("never treats buttons or timestamps as empty", () => {
    for (const type of ["button", "created_time", "last_edited_time"] as const) {
      expect(notesDatabaseRowPageValueIsEmpty(rowPage({}), column(type, type))).toBe(false);
    }
  });
});

function hierarchy(depth: number, ancestors: string[] = []): NotesDatabaseRowHierarchyMetadata {
  return { parent_row_page_id: ancestors[0] ?? null, ancestor_row_page_ids: ancestors, depth, child_count: 0 };
}

describe("notesDatabaseSubitemExclusions", () => {
  it("leaves out the row, every ancestor, and its direct sub-items, but not deeper descendants", () => {
    const excluded = notesDatabaseSubitemExclusions({ rowId: "self", hierarchy: hierarchy(2, ["parent", "root"]), childIds: ["child"] });
    expect(excluded).toEqual(["self", "parent", "root", "child"]);
    expect(excluded).not.toContain("grandchild");
  });

  it("allows another level until the row reaches the depth limit", () => {
    expect(notesDatabaseRowCanAddSubitems(hierarchy(NOTES_DATABASE_ROW_MAX_DEPTH))).toBe(false);
    expect(notesDatabaseRowCanAddSubitems(hierarchy(NOTES_DATABASE_ROW_MAX_DEPTH - 1))).toBe(true);
  });
});

describe("notesDatabaseParentExclusions", () => {
  it("leaves out the row, its current parent, and its direct sub-items, but offers higher ancestors", () => {
    const excluded = notesDatabaseParentExclusions({ rowId: "self", hierarchy: hierarchy(2, ["parent", "root"]), childIds: ["child"] });
    expect(excluded).toEqual(["self", "parent", "child"]);
    expect(excluded).not.toContain("root");
  });

  it("leaves out only the row and its sub-items for a top-level row", () => {
    expect(notesDatabaseParentExclusions({ rowId: "self", hierarchy: hierarchy(0), childIds: ["a", "b"] })).toEqual(["self", "a", "b"]);
  });
});

describe("notesDatabaseRowSearch", () => {
  const rows = ["Write docs", "Revisión de diseño", "Ship release"];

  it("matches every word in any order, ignoring case and accents", () => {
    expect(notesDatabaseRowSearch(rows, "DISENO revision", (row) => row)).toEqual(["Revisión de diseño"]);
    expect(notesDatabaseRowSearch(rows, "docs write", (row) => row)).toEqual(["Write docs"]);
  });

  it("returns every row for a blank query", () => {
    expect(notesDatabaseRowSearch(rows, "  ", (row) => row)).toEqual(rows);
  });
});
