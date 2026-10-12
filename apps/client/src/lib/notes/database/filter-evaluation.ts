import type { NotesDatabaseTableColumn } from "./table";
import { notesDatabaseTableCellText } from "./table";
import { notesDatabaseIsFilterPredicate } from "./filters";
import type { NotesDatabaseTableFilter, NotesDatabaseTableFilterCondition, NotesPage } from "$lib/notes/types";

const DAY_MILLISECONDS = 24 * 60 * 60 * 1000;
const WHITE_SPACE_ONLY = /^[\u0009-\u000D\u0020\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]*$/u;

/** Match SQLite's default case folding without changing non-ASCII letters. */
function sqliteAsciiCaseFold(value: string): string {
  return value.replace(/[A-Z]/g, (character) => character.toLowerCase());
}

function propertyPayload(row: NotesPage, column: NotesDatabaseTableColumn): unknown {
  const property = Object.values(row.properties).find((value) => typeof value === "object" && value !== null
    && !Array.isArray(value) && (("id" in value && value.id === column.id)
      || (column.type === "title" && "type" in value && value.type === "title")));
  return typeof property === "object" && property !== null && !Array.isArray(property)
    ? (property as Record<string, unknown>)[column.type] : undefined;
}

function dateNumber(value: unknown, dayOnly: boolean): number | null {
  if (typeof value !== "string" || !value) return null;
  // Stored dates can use a separate timezone with a local timestamp. SQL treats
  // such timestamps as wall-clock values; explicit offsets denote UTC instants.
  const hasExplicitOffset = /(?:Z|[+-]\d{2}:\d{2})$/.test(value);
  const timestamp = Date.parse(value.length === 10 ? `${value}T00:00:00Z` : hasExplicitOffset ? value : `${value}Z`);
  return Number.isFinite(timestamp) ? dayOnly ? Math.floor(timestamp / DAY_MILLISECONDS) : timestamp : null;
}

function satisfiesCondition(condition: NotesDatabaseTableFilterCondition, left: number | string, right: number | string): boolean {
  switch (condition) {
    case "equals": return left === right;
    case "not_equals": return left !== right;
    case "greater_than": case "after": return left > right;
    case "greater_than_or_equal": case "on_or_after": return left >= right;
    case "less_than": case "before": return left < right;
    case "less_than_or_equal": case "on_or_before": return left <= right;
    default: return false;
  }
}

/** Evaluate bounded view predicates against exact row values for presentation rules. */
export function notesDatabaseRowMatchesFilters(row: NotesPage, columns: readonly NotesDatabaseTableColumn[], filters: readonly NotesDatabaseTableFilter[]): boolean {
  function matches(filter: NotesDatabaseTableFilter): boolean {
    if (!notesDatabaseIsFilterPredicate(filter)) return filter.type === "and" ? filter.filters.every(matches) : filter.filters.some(matches);
    const column = columns.find((candidate) => candidate.id === filter.property_id);
    if (!column || ["formula", "rollup", "button"].includes(column.type)) return false;
    const payload = propertyPayload(row, column);
    const text = column.type === "created_time" ? row.created_time : column.type === "last_edited_time" ? row.last_edited_time
      : column.type === "checkbox" ? typeof payload === "boolean" ? String(payload) : ""
      : notesDatabaseTableCellText(row, column);
    if (filter.condition === "is_empty") return WHITE_SPACE_ONLY.test(text);
    if (filter.condition === "is_not_empty") return !WHITE_SPACE_ONLY.test(text);
    if (filter.condition === "checked") return column.type === "checkbox" && payload === true;
    if (filter.condition === "unchecked") return column.type === "checkbox" && payload === false;
    if (column.type === "number") return typeof payload === "number" && Number.isFinite(payload)
      && typeof filter.value === "number" && Number.isFinite(filter.value) && satisfiesCondition(filter.condition, payload, filter.value);
    if (column.type === "date" || column.type === "created_time" || column.type === "last_edited_time") {
      const dayOnly = typeof filter.value === "string" && filter.value.length === 10;
      const left = dateNumber(text, dayOnly), right = dateNumber(filter.value, dayOnly);
      return left !== null && right !== null && satisfiesCondition(filter.condition, left, right);
    }
    const left = sqliteAsciiCaseFold(text);
    const right = sqliteAsciiCaseFold(String(filter.value ?? ""));
    return filter.condition === "contains" ? left.includes(right) : satisfiesCondition(filter.condition, left, right);
  }
  return filters.every(matches);
}
