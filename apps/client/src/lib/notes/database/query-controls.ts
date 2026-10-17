import type { NotesDatabaseTableColumn } from "./table";
import type { NotesDatabaseTableFilter, NotesDatabaseTableFilterCondition, NotesDatabaseTableFilterPredicate, NotesDatabaseTableSort } from "$lib/notes/types";
import { notesDatabaseIsFilterDateValue, notesDatabaseIsFilterPredicate, notesDatabaseSerializeFilters } from "./filters";
export { notesDatabaseRowMatchesFilters } from "./filter-evaluation";
export { NOTES_DATABASE_QUERY_MAX_FILTERS, NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS, NOTES_DATABASE_QUERY_MAX_FILTER_DEPTH,
  notesDatabaseFilterCount, notesDatabaseFilterPredicates, notesDatabaseIsFilterPredicate } from "./filters";

export type NotesDatabaseQueryProperty = Pick<NotesDatabaseTableColumn, "id" | "name" | "type">;

/** Match the bounded native view-query contract. */
export const NOTES_DATABASE_QUERY_MAX_SORTS = 5;

const VALUE_CONDITIONS: readonly NotesDatabaseTableFilterCondition[] = ["contains", "equals", "not_equals", "is_empty", "is_not_empty"];
const CHECKBOX_CONDITIONS: readonly NotesDatabaseTableFilterCondition[] = ["checked", "unchecked", "is_empty", "is_not_empty"];
const NUMBER_CONDITIONS: readonly NotesDatabaseTableFilterCondition[] = ["equals", "not_equals", "greater_than", "greater_than_or_equal", "less_than", "less_than_or_equal", "is_empty", "is_not_empty"];
const DATE_CONDITIONS: readonly NotesDatabaseTableFilterCondition[] = ["equals", "not_equals", "before", "on_or_before", "after", "on_or_after", "is_empty", "is_not_empty"];

/** Return predicates supported by a property's local scalar representation. */
export function notesDatabaseFilterConditions(property: NotesDatabaseQueryProperty): readonly NotesDatabaseTableFilterCondition[] {
  switch (property.type) {
    case "formula": case "rollup": case "button": return [];
    case "checkbox": return CHECKBOX_CONDITIONS;
    case "number": return NUMBER_CONDITIONS;
    case "date": case "created_time": case "last_edited_time": return DATE_CONDITIONS;
    default: return VALUE_CONDITIONS;
  }
}

/** Identify predicates that require a scalar value. */
export function notesDatabaseFilterNeedsValue(condition: NotesDatabaseTableFilterCondition): boolean {
  return !["is_empty", "is_not_empty", "checked", "unchecked"].includes(condition);
}

/** Match date properties and metadata using the same typed operators. */
export function notesDatabaseIsDateFilterProperty(property: NotesDatabaseQueryProperty): boolean {
  return property.type === "date" || property.type === "created_time" || property.type === "last_edited_time";
}

/** Seed a complete typed value when switching from a valueless condition. */
function initialFilterValue(property: NotesDatabaseQueryProperty): string | number {
  if (property.type === "number") return 0;
  if (notesDatabaseIsDateFilterProperty(property)) {
    const now = new Date();
    return `${String(now.getFullYear()).padStart(4, "0")}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  }
  return "";
}

/** Create a supported initial predicate for the selected property. */
export function notesDatabaseNewFilter(property: NotesDatabaseQueryProperty): NotesDatabaseTableFilterPredicate {
  const typed = property.type === "number" || notesDatabaseIsDateFilterProperty(property);
  return {
    property_id: property.id,
    condition: property.type === "checkbox" ? "checked" : typed ? "is_not_empty" : "contains",
    value: property.type === "checkbox" || typed ? null : "",
  };
}

/** Change a predicate while clearing values incompatible with its new property or condition. */
export function notesDatabaseUpdatedFilters(
  filters: readonly NotesDatabaseTableFilter[],
  index: number | readonly number[],
  patch: Partial<NotesDatabaseTableFilterPredicate>,
  properties: readonly NotesDatabaseQueryProperty[],
): NotesDatabaseTableFilter[] {
  return notesDatabaseTransformFilter(filters, typeof index === "number" ? [index] : index, (filter) => {
    if (!notesDatabaseIsFilterPredicate(filter)) return filter;
    const next = { ...filter, ...patch };
    const property = properties.find((candidate) => candidate.id === next.property_id);
    if (!property) return { ...filter };
    if (!notesDatabaseFilterConditions(property).includes(next.condition)) {
      const initial = notesDatabaseNewFilter(property);
      next.condition = initial.condition;
      next.value = initial.value;
    }
    if (!notesDatabaseFilterNeedsValue(next.condition)) next.value = null;
    else if (next.value === null || next.value === undefined
      || (property.type === "number" && typeof next.value !== "number")
      || (notesDatabaseIsDateFilterProperty(property) && (typeof next.value !== "string" || !notesDatabaseIsFilterDateValue(next.value)))) next.value = initialFilterValue(property);
    return next;
  });
}

/** Edit or remove a node by its stable path, preserving sibling groups and predicates. */
export function notesDatabaseTransformFilter(filters: readonly NotesDatabaseTableFilter[], path: readonly number[], transform: (filter: NotesDatabaseTableFilter) => NotesDatabaseTableFilter | null): NotesDatabaseTableFilter[] {
  return filters.flatMap<NotesDatabaseTableFilter>((filter, index) => {
    if (index !== path[0]) return notesDatabaseSerializeFilters([filter]);
    if (path.length === 1) {
      const next = transform(filter);
      return next ? [next] : [];
    }
    if (notesDatabaseIsFilterPredicate(filter)) return [{ ...filter }];
    const children = notesDatabaseTransformFilter(filter.filters, path.slice(1), transform);
    return children.length ? [{ type: filter.type, filters: children }] : [];
  });
}

/** Count group containers for the bounded editor. */
export function notesDatabaseFilterGroupCount(filters: readonly NotesDatabaseTableFilter[]): number {
  return filters.reduce((count, filter) => count + (notesDatabaseIsFilterPredicate(filter) ? 0 : 1 + notesDatabaseFilterGroupCount(filter.filters)), 0);
}

/** Add a node at the root or inside an existing group. */
export function notesDatabaseAppendFilter(filters: readonly NotesDatabaseTableFilter[], path: readonly number[], node: NotesDatabaseTableFilter): NotesDatabaseTableFilter[] {
  if (!path.length) return [...notesDatabaseSerializeFilters(filters), node];
  return notesDatabaseTransformFilter(filters, path, (filter) => notesDatabaseIsFilterPredicate(filter) ? filter
    : { type: filter.type, filters: [...notesDatabaseSerializeFilters(filter.filters), node] });
}

/** Preserve sort priority when changing direction and never append the same property twice. */
export function notesDatabaseSortsWithColumn(
  sorts: readonly NotesDatabaseTableSort[],
  propertyId: string,
  direction: NotesDatabaseTableSort["direction"],
): NotesDatabaseTableSort[] {
  const existingIndex = sorts.findIndex((sort) => sort.property_id === propertyId);
  const next = { property_id: propertyId, direction };
  if (existingIndex < 0 && sorts.length >= NOTES_DATABASE_QUERY_MAX_SORTS) return sorts.map((sort) => ({ ...sort }));
  return existingIndex < 0 ? [...sorts.map((sort) => ({ ...sort })), next]
    : sorts.map((sort, index) => index === existingIndex ? next : { ...sort });
}
