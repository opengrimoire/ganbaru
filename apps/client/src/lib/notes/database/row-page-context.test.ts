import { describe, expect, it } from "vitest";
import { basePage } from "$lib/notes/blocks/validation.fixtures";
import { parseNotesDataSourceRowPageContext } from "./row-page-context";

const dataSourceId = "21212121-2121-4121-8121-212121212121";
const databaseId = "22222222-2222-4222-8222-222222222222";
const rowId = "31313131-3131-4131-8131-313131313131";
const parentId = "32323232-3232-4232-8232-323232323232";
const childId = "33333333-3333-4333-8333-333333333333";

const schema = {
  data_source: {
    object: "data_source",
    id: dataSourceId,
    parent: { type: "database_id", database_id: databaseId },
    database_parent: { type: "page_id", page_id: basePage.id },
    title: "Tasks",
    title_rich_text: [],
    description: [],
    icon: null,
    properties: {
      Name: { id: "title", name: "Name", description: "", type: "title", title: {} },
      Due: { id: "due", name: "Due", description: "", type: "date", date: {}, page_visibility: "hide_when_empty" },
    },
    in_trash: false,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T00:00:00.000Z",
  },
  view: {
    object: "view",
    id: "44444444-4444-4444-8444-444444444444",
    parent: { type: "database_id", database_id: databaseId },
    data_source_id: dataSourceId,
    name: "Table",
    type: "table",
    filter: null,
    sorts: [],
    configuration: null,
    url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: "2026-07-01T00:00:00.000Z",
    last_edited_time: "2026-07-01T00:00:00.000Z",
  },
};

function rowPage(id: string) {
  return { ...basePage, id, parent: { type: "data_source_id", data_source_id: dataSourceId } };
}

function context(overrides: Record<string, unknown> = {}) {
  return {
    schema,
    row: rowPage(rowId),
    hierarchy: { parent_row_page_id: parentId, ancestor_row_page_ids: [parentId], depth: 1, child_count: 1 },
    parent: rowPage(parentId),
    children: [rowPage(childId)],
    editing_locked: true,
    ...overrides,
  };
}

describe("row page context validation", () => {
  it("parses a row with its schema, parent, sub-items, and lock state", () => {
    const parsed = parseNotesDataSourceRowPageContext(context());

    expect(parsed.row.id).toBe(rowId);
    expect(parsed.parent?.id).toBe(parentId);
    expect(parsed.children.map((child) => child.id)).toEqual([childId]);
    expect(parsed.hierarchy).toEqual({ parent_row_page_id: parentId, ancestor_row_page_ids: [parentId], depth: 1, child_count: 1 });
    expect(parsed.editing_locked).toBe(true);
    expect(parsed.schema.data_source.properties.Due).toMatchObject({ page_visibility: "hide_when_empty" });
  });

  it("accepts a top-level row without a parent", () => {
    const parsed = parseNotesDataSourceRowPageContext(context({
      hierarchy: { parent_row_page_id: null, ancestor_row_page_ids: [], depth: 0, child_count: 0 },
      parent: null,
      children: [],
    }));

    expect(parsed.parent).toBeNull();
    expect(parsed.children).toEqual([]);
  });

  it("rejects a parent that disagrees with the hierarchy", () => {
    expect(() => parseNotesDataSourceRowPageContext(context({ parent: null })))
      .toThrow("row page context.parent must match its hierarchy");
  });

  it("rejects sub-items that repeat the row or its parent", () => {
    expect(() => parseNotesDataSourceRowPageContext(context({ children: [rowPage(rowId)] })))
      .toThrow("row page context.children must not contain the row or its parent");
    expect(() => parseNotesDataSourceRowPageContext(context({ children: [rowPage(parentId)] })))
      .toThrow("row page context.children must not contain the row or its parent");
  });

  it("rejects inconsistent hierarchy metadata", () => {
    expect(() => parseNotesDataSourceRowPageContext(context({
      hierarchy: { parent_row_page_id: parentId, ancestor_row_page_ids: [], depth: 1, child_count: 1 },
    }))).toThrow("Database row hierarchy is inconsistent");
  });
});
