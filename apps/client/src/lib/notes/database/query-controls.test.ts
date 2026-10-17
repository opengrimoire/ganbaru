import { describe, expect, it } from "vitest";
import {
  notesDatabaseFilterConditions,
  notesDatabaseNewFilter,
  notesDatabaseSortsWithColumn,
  notesDatabaseUpdatedFilters,
  NOTES_DATABASE_QUERY_MAX_SORTS,
  type NotesDatabaseQueryProperty,
} from "./query-controls";

const properties: NotesDatabaseQueryProperty[] = [
  { id: "title", name: "Name", type: "title" },
  { id: "done", name: "Done", type: "checkbox" },
  { id: "due", name: "Due", type: "date" },
];

describe("Notes database query predicates", () => {
  it("restricts predicates to each property's scalar type", () => {
    expect(notesDatabaseFilterConditions(properties[1])).toEqual(["checked", "unchecked", "is_empty", "is_not_empty"]);
    expect(notesDatabaseFilterConditions(properties[0])).toEqual(["contains", "equals", "not_equals", "is_empty", "is_not_empty"]);
    expect(notesDatabaseFilterConditions(properties[2])).toEqual(["equals", "not_equals", "before", "on_or_before", "after", "on_or_after", "is_empty", "is_not_empty"]);
    expect(notesDatabaseFilterConditions({ id: "estimate", name: "Estimate", type: "number" })).toEqual(["equals", "not_equals", "greater_than", "greater_than_or_equal", "less_than", "less_than_or_equal", "is_empty", "is_not_empty"]);
    expect(notesDatabaseFilterConditions({ id: "formula", name: "Formula", type: "formula" })).toEqual([]);
  });

  it("clears an incompatible value when changing a text filter to a checkbox", () => {
    const filters = [
      { property_id: "title", condition: "contains" as const, value: "Budget" },
      { property_id: "due", condition: "equals" as const, value: "2026-09-30" },
    ];
    expect(notesDatabaseUpdatedFilters(filters, 0, { property_id: "done" }, properties)).toEqual([
      { property_id: "done", condition: "checked", value: null }, filters[1],
    ]);
    expect(filters[0].value).toBe("Budget");
  });

  it("creates an empty scalar predicate when changing a checkbox filter to text", () => {
    expect(notesDatabaseUpdatedFilters([{ property_id: "done", condition: "unchecked", value: null }], 0,
      { property_id: "title" }, properties)).toEqual([{ property_id: "title", condition: "contains", value: "" }]);
  });

  it("preserves compatible empty predicates and clears unused values", () => {
    expect(notesDatabaseUpdatedFilters([{ property_id: "title", condition: "is_empty", value: "stale" }], 0,
      { property_id: "done" }, properties)).toEqual([{ property_id: "done", condition: "is_empty", value: null }]);
    expect(notesDatabaseNewFilter(properties[1])).toEqual({ property_id: "done", condition: "checked", value: null });
  });

  it("retains a predicate if the selected property is unavailable", () => {
    const filters = [{ property_id: "title", condition: "contains" as const, value: "Budget" }];
    expect(notesDatabaseUpdatedFilters(filters, 0, { property_id: "removed" }, properties)).toEqual(filters);
  });
});

describe("Notes database property sorts", () => {
  it("rejects a sixth property while allowing direction changes at the native bound", () => {
    const sorts = Array.from({ length: NOTES_DATABASE_QUERY_MAX_SORTS }, (_, index) => ({
      property_id: `property-${index}`, direction: "ascending" as const,
    }));
    expect(notesDatabaseSortsWithColumn(sorts, "another", "descending")).toEqual(sorts);
    expect(notesDatabaseSortsWithColumn(sorts, "property-2", "descending")).toEqual(sorts.map((sort, index) =>
      index === 2 ? { ...sort, direction: "descending" } : sort));
  });

  it("changes direction in place and appends each new property only once", () => {
    const sorts = [{ property_id: "title", direction: "ascending" as const }, { property_id: "due", direction: "ascending" as const }];
    const descending = notesDatabaseSortsWithColumn(sorts, "due", "descending");
    expect(descending).toEqual([sorts[0], { property_id: "due", direction: "descending" }]);
    expect(notesDatabaseSortsWithColumn(descending, "done", "ascending")).toEqual([
      ...descending, { property_id: "done", direction: "ascending" },
    ]);
    expect(sorts[1].direction).toBe("ascending");
  });
});
