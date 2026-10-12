import type { NotesDatabaseTableFilter, NotesDatabaseTableFilterCondition, NotesDatabaseTableFilterPredicate } from "$lib/notes/types";

export const NOTES_DATABASE_QUERY_MAX_FILTERS = 10;
export const NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS = 8;
export const NOTES_DATABASE_QUERY_MAX_FILTER_DEPTH = 3;
const MAX_FILTER_TEXT_CHARS = 200;

const CONDITIONS: readonly NotesDatabaseTableFilterCondition[] = [
  "contains", "equals", "not_equals", "greater_than", "greater_than_or_equal", "less_than", "less_than_or_equal",
  "before", "on_or_before", "after", "on_or_after", "is_empty", "is_not_empty", "checked", "unchecked",
];

/** Distinguish a scalar predicate from a Boolean group. */
export function notesDatabaseIsFilterPredicate(filter: NotesDatabaseTableFilter): filter is NotesDatabaseTableFilterPredicate {
  return "property_id" in filter;
}

/** Return recursive predicates in presentation order without changing Boolean structure. */
export function notesDatabaseFilterPredicates(filters: readonly NotesDatabaseTableFilter[], propertyId?: string): NotesDatabaseTableFilterPredicate[] {
  return filters.flatMap((filter) => notesDatabaseIsFilterPredicate(filter)
    ? propertyId === undefined || filter.property_id === propertyId ? [filter] : []
    : notesDatabaseFilterPredicates(filter.filters, propertyId));
}

/** Count predicates rather than group containers. */
export function notesDatabaseFilterCount(filters: readonly NotesDatabaseTableFilter[], propertyId?: string): number {
  return notesDatabaseFilterPredicates(filters, propertyId).length;
}

/** Copy a filter tree into the native write contract without sharing mutable children. */
export function notesDatabaseSerializeFilters(filters: readonly NotesDatabaseTableFilter[]): NotesDatabaseTableFilter[] {
  return filters.map((filter) => notesDatabaseIsFilterPredicate(filter)
    ? { property_id: filter.property_id, condition: filter.condition, value: filter.value ?? null }
    : { type: filter.type, filters: notesDatabaseSerializeFilters(filter.filters) });
}

/** Validate an ISO calendar date or an explicit RFC 3339 instant. */
export function notesDatabaseIsFilterDateValue(value: string): boolean {
  if (/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    const date = new Date(`${value}T00:00:00Z`);
    return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value;
  }
  return /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value)
    && notesDatabaseIsFilterDateValue(value.slice(0, 10)) && Number.isFinite(Date.parse(value));
}

/** Parse the bounded persisted tree. The external root is an AND or OR group. */
export function notesDatabaseParseFilters(value: unknown): NotesDatabaseTableFilter[] {
  if (value === null || value === undefined) return [];
  const counts = { predicates: 0, groups: 0 };
  function readFilterRecord(raw: unknown): Record<string, unknown> {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) throw new Error("database filter must be an object");
    return raw as Record<string, unknown>;
  }
  function parseFilterNodes(raw: unknown, depth: number): NotesDatabaseTableFilter[] {
    if (!Array.isArray(raw)) throw new Error("database filter.filters must be an array");
    return raw.map<NotesDatabaseTableFilter>((entry: unknown) => {
      const node = readFilterRecord(entry);
      if (node.type !== undefined) {
        if ((node.type !== "and" && node.type !== "or") || node.property_id !== undefined || node.condition !== undefined || node.value !== undefined) {
          throw new Error("database filter group must use and or or");
        }
        counts.groups += 1;
        if (depth >= NOTES_DATABASE_QUERY_MAX_FILTER_DEPTH || counts.groups > NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS) throw new Error("database filter group limit exceeded");
        const filters = parseFilterNodes(node.filters, depth + 1);
        if (!filters.length) throw new Error("database filter group must be nonempty");
        return { type: node.type, filters };
      }
      counts.predicates += 1;
      if (counts.predicates > NOTES_DATABASE_QUERY_MAX_FILTERS) throw new Error("database filter predicate limit exceeded");
      if (typeof node.property_id !== "string" || !node.property_id.trim()) throw new Error("database filter.property_id must be a nonempty string");
      const condition = CONDITIONS.find((candidate) => candidate === node.condition);
      if (!condition || node.filters !== undefined) throw new Error("database filter.condition must be supported");
      const value = node.value ?? null;
      if (typeof value !== "string" && typeof value !== "boolean" && typeof value !== "number" && value !== null) throw new Error("database filter.value must be scalar");
      if (typeof value === "number" && !Number.isFinite(value)) throw new Error("database filter.value must be finite");
      if (typeof value === "string" && (Array.from(value).length > MAX_FILTER_TEXT_CHARS || /\p{Cc}/u.test(value))) throw new Error("database filter.value contains invalid text");
      if (["greater_than", "greater_than_or_equal", "less_than", "less_than_or_equal"].includes(condition) && typeof value !== "number") throw new Error("database numeric filter.value must be a number");
      if (["before", "on_or_before", "after", "on_or_after"].includes(condition) && (typeof value !== "string" || !notesDatabaseIsFilterDateValue(value))) throw new Error("database date filter.value must be an ISO date or RFC 3339 timestamp");
      return { property_id: node.property_id, condition, value };
    });
  }
  const root = readFilterRecord(value);
  if (root.type !== undefined && root.type !== "and" && root.type !== "or") throw new Error("database filter.type must be and or or");
  const filters = parseFilterNodes(root.filters, root.type === "or" ? 1 : 0);
  if (root.type === "or") {
    if (!filters.length) throw new Error("database filter group must be nonempty");
    counts.groups += 1;
    if (counts.groups > NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS) throw new Error("database filter group limit exceeded");
    return [{ type: "or", filters }];
  }
  return filters;
}
