import { describe, expect, it } from "vitest";
import { notesTableColumnPresentation, notesTableFrozenOffset, notesTableGroups, parseNotesTablePresentation } from "./table-presentation";
import { createProvisionalNotesPage } from "$lib/notes/pages/creation";
import type { NotesDatabaseTableColumn } from "./table";

function column(id: string, width: number): NotesDatabaseTableColumn {
  return { id, width, name: id, type: "select", hidden: false, options: [{ id: "later", name: "Later", color: "blue" }], relationDataSourceId: null, buttonLabel: "", buttonRequiresConfirmation: false };
}

describe("saved table presentation", () => {
  it("validates presentation values and unknown settings before rendering", () => {
    expect(parseNotesTablePresentation({ columns: { title: { wrap: true, calculation: "unique" } }, frozen_property_id: "title" }).columns.title.wrap).toBe(true);
    for (const value of [{ columns: { title: { wrap: "yes" } } }, { columns: { title: { calculation: "median" } } }, { frozen_property_id: 7 }, { columns: { title: { date_format: "tomorrow" } } }, { unrelated: true }]) {
      expect(() => parseNotesTablePresentation(value)).toThrow();
    }
  });
  it("calculates frozen offsets from visible column widths and the selected boundary", () => {
    const columns = [column("title", 220), column("status", 180), column("estimate", 120)];
    expect(notesTableFrozenOffset(columns, "status", "title")).toBe(0);
    expect(notesTableFrozenOffset(columns, "status", "status")).toBe(220);
    expect(notesTableFrozenOffset(columns, "status", "estimate")).toBeNull();
    expect(notesTableFrozenOffset(columns, "missing", "title")).toBeNull();
  });
  it("shows complete-source groups even when their rows have not been hydrated", () => {
    const groups = notesTableGroups([], [column("status", 180)], { property_order: [], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page", group_property_id: "status", group_order: ["remote", "later"], collapsed_group_ids: ["later"], hide_empty_groups: true }, { later: 12, remote: 4 });
    expect(groups.map((group) => [group.id, group.count, group.collapsed, group.rows.length])).toEqual([["remote", 4, false, 0], ["later", 12, true, 0]]);
  });
  it("leaves a scrollable data column visible when a saved frozen prefix exceeds the viewport", () => {
    const columns = [column("title", 380), column("status", 180), column("estimate", 120)];
    expect(notesTableFrozenOffset(columns, "status", "title", 340)).toBeNull();
    expect(notesTableFrozenOffset(columns, "status", "title", 500)).toBe(0);
    expect(notesTableFrozenOffset(columns, "status", "status", 500)).toBeNull();
    expect(notesTableFrozenOffset(columns, "status", "status", 680)).toBe(380);
    expect(notesTableFrozenOffset(columns, "status", "estimate", 900)).toBeNull();
  });
  it("treats inherited object names as ordinary property and group identities", () => {
    expect(notesTableColumnPresentation({ frozen_property_id: null, columns: {} }, "constructor").wrap).toBe(false);
    const status = { ...column("status", 180), options: [{ id: "constructor", name: "Constructor", color: "default" }] };
    const groups = notesTableGroups([], [status], { property_order: [], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page", group_property_id: "status", hide_empty_groups: true }, {});
    expect(groups).toEqual([]);
  });
  it("groups relation identities and repeated multi-value memberships exactly once", () => {
    const row = createProvisionalNotesPage({ id: "row", first_block_id: "block", title: "Task", parent: { type: "data_source_id", data_source_id: "source" }, folder_id: null }).page;
    row.properties.Related = { id: "related", type: "relation", relation: [{ id: "target", title: "Target" }, { id: "target", title: "Target" }] };
    const related: NotesDatabaseTableColumn = { ...column("related", 180), name: "Related", type: "relation" };
    const groups = notesTableGroups([row], [related], { property_order: [], hidden_property_ids: [], column_widths: {}, row_open_mode: "full_page", group_property_id: "related", hide_empty_groups: true }, { target: 1 });
    expect(groups).toMatchObject([{ id: "target", name: "Target", count: 1, rows: [{ id: "row" }] }]);
  });
});
