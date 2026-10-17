import { describe, expect, it } from "vitest";
import { notesDatabaseFilterCount, notesDatabaseParseFilters, notesDatabaseSerializeFilters, NOTES_DATABASE_QUERY_MAX_FILTERS } from "./filters";
import { notesDatabaseAppendFilter, notesDatabaseTransformFilter, notesDatabaseUpdatedFilters } from "./query-controls";
import type { NotesDatabaseTableFilter } from "$lib/notes/types";

const filters: NotesDatabaseTableFilter[] = [{ type: "or", filters: [
  { property_id: "title", condition: "contains", value: "plan" },
  { type: "and", filters: [{ property_id: "estimate", condition: "greater_than", value: 10 }, { property_id: "due", condition: "before", value: "2026-10-03" }] },
] }];

describe("bounded Notes filter trees", () => {
  it("retains the existing flat AND root when its persisted type is omitted", () => {
    const filters = [{ property_id: "title", condition: "contains", value: "plan" }];
    expect(notesDatabaseParseFilters({ filters })).toEqual(filters);
    expect(() => notesDatabaseParseFilters({ type: "xor", filters })).toThrow();
  });
  it("preserves nested groups and numeric values through parsing and serialization", () => {
    expect(notesDatabaseParseFilters({ type: "and", filters })).toEqual(filters);
    expect(notesDatabaseParseFilters({ type: "or", filters: filters[0] && "filters" in filters[0] ? filters[0].filters : [] })).toEqual(filters);
    const copy = notesDatabaseSerializeFilters(filters);
    expect(copy).toEqual(filters);
    expect(copy[0]).not.toBe(filters[0]);
    expect(notesDatabaseFilterCount(filters)).toBe(3);
    expect(notesDatabaseFilterCount(filters, "estimate")).toBe(1);
  });

  it("updates and removes nested leaves without flattening sibling Boolean groups", () => {
    const updated = notesDatabaseUpdatedFilters(filters, [0, 1, 0], { value: 25 }, [{ id: "estimate", name: "Estimate", type: "number" }]);
    expect(updated).toEqual([{ type: "or", filters: [
      { property_id: "title", condition: "contains", value: "plan" },
      { type: "and", filters: [{ property_id: "estimate", condition: "greater_than", value: 25 }, { property_id: "due", condition: "before", value: "2026-10-03" }] },
    ] }]);
    const removed = notesDatabaseTransformFilter(updated, [0, 1], () => null);
    expect(removed).toEqual([{ type: "or", filters: [{ property_id: "title", condition: "contains", value: "plan" }] }]);
    expect(notesDatabaseTransformFilter(removed, [0, 0], () => null)).toEqual([]);
    expect(notesDatabaseAppendFilter(removed, [0], { property_id: "done", condition: "checked", value: null })).toEqual([
      { type: "or", filters: [{ property_id: "title", condition: "contains", value: "plan" }, { property_id: "done", condition: "checked", value: null }] },
    ]);
    expect(notesDatabaseFilterCount(filters)).toBe(3);
  });

  it("rejects malformed predicates, invalid typed values, and empty groups", () => {
    for (const value of [
      { type: "xor", filters: [] }, { type: "or", filters: [] },
      { type: "and", filters: [{ type: "or", filters: [] }] },
      { type: "and", filters: [{ property_id: "estimate", condition: "greater_than", value: "10" }] },
      { type: "and", filters: [{ property_id: "estimate", condition: "equals", value: Number.NaN }] },
      { type: "and", filters: [{ property_id: "due", condition: "before", value: "2026-02-30" }] },
      { type: "and", filters: [{ property_id: "due", condition: "before", value: "2026-02-30T10:00:00Z" }] },
      { type: "and", filters: [{ property_id: "due", condition: "after", value: "tomorrow" }] },
      { type: "and", filters: [{ property_id: "title", condition: "contains", value: "x".repeat(201) }] },
      { type: "and", filters: [{ property_id: "title", condition: "contains", value: "line\nend" }] },
    ]) expect(() => notesDatabaseParseFilters(value)).toThrow();
  });

  it("enforces total predicate and recursive depth bounds across groups", () => {
    const leaf = { property_id: "title", condition: "contains", value: "" };
    expect(() => notesDatabaseParseFilters({ type: "and", filters: Array.from({ length: NOTES_DATABASE_QUERY_MAX_FILTERS + 1 }, () => leaf) })).toThrow("predicate limit");
    const group = { type: "and", filters: [leaf] };
    expect(() => notesDatabaseParseFilters({ type: "and", filters: Array.from({ length: 9 }, () => group) })).toThrow("group limit");
    expect(() => notesDatabaseParseFilters({ type: "and", filters: [{ type: "or", filters: [{ type: "and", filters: [group] }] }] })).not.toThrow();
    expect(() => notesDatabaseParseFilters({ type: "and", filters: [{ type: "or", filters: [{ type: "and", filters: [{ type: "or", filters: [group] }] }] }] })).toThrow("group limit");
  });
});
