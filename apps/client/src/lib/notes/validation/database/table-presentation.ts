import type { NotesDatabaseTableCalculation, NotesDatabaseTableColorRule, NotesDatabaseTableColumnPresentation, NotesDatabaseTablePresentation } from "../../contracts/database/table";
import { notesDatabaseParseFilters } from "$lib/notes/database/filters";

const DATE_FORMATS = new Set(["locale", "iso", "relative"]);
const TIME_FORMATS = new Set(["locale", "12_hour", "24_hour", "hidden"]);
export const NOTES_TABLE_CALCULATIONS = ["count_all", "count_values", "empty", "unique", "sum", "average", "min", "max", "percent_checked"] as const satisfies readonly NotesDatabaseTableCalculation[];

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Validate unknown saved presentation without dropping invalid configuration. */
export function parseNotesTablePresentation(value: unknown): NotesDatabaseTablePresentation {
  if (value === undefined) return { frozen_property_id: null, columns: {} };
  if (!isPlainRecord(value)) throw new Error("Table presentation must be an object");
  if (Object.keys(value).some((key) => !["columns", "frozen_property_id", "color_rules"].includes(key))) throw new Error("Unknown table presentation setting");
  const frozen = value.frozen_property_id ?? null;
  if (frozen !== null && typeof frozen !== "string") throw new Error("Frozen property identity must be a string");
  const rawColumns = value.columns ?? {};
  if (!isPlainRecord(rawColumns)) throw new Error("Column presentation must be an object");
  const columns: Record<string, NotesDatabaseTableColumnPresentation> = {};
  for (const [id, settings] of Object.entries(rawColumns)) {
    if (!id || !isPlainRecord(settings)) throw new Error("Invalid column presentation");
    if (Object.keys(settings).some((key) => !["wrap", "date_format", "time_format", "calculation"].includes(key))) throw new Error("Unknown column presentation setting");
    const wrap = settings.wrap ?? false;
    const dateFormat = settings.date_format ?? "locale";
    const timeFormat = settings.time_format ?? "locale";
    const calculation = settings.calculation ?? null;
    if (typeof wrap !== "boolean" || typeof dateFormat !== "string" || !DATE_FORMATS.has(dateFormat)
      || typeof timeFormat !== "string" || !TIME_FORMATS.has(timeFormat)
      || (calculation !== null && !NOTES_TABLE_CALCULATIONS.includes(calculation as NotesDatabaseTableCalculation))) {
      throw new Error("Unsupported column presentation");
    }
    const parsed = {
      wrap,
      date_format: dateFormat as NotesDatabaseTableColumnPresentation["date_format"],
      time_format: timeFormat as NotesDatabaseTableColumnPresentation["time_format"],
      calculation: calculation as NotesDatabaseTableCalculation | null,
    };
    Object.defineProperty(columns, id, { value: parsed, enumerable: true, configurable: true, writable: true });
  }
  const rawRules = value.color_rules ?? [];
  if (!Array.isArray(rawRules) || rawRules.length > 16) throw new Error("Conditional colors require at most 16 rules");
  const colors: readonly NotesDatabaseTableColorRule["color"][] = ["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"];
  const identities = new Set<string>();
  const rules = rawRules.map((rule: unknown): NotesDatabaseTableColorRule => {
    if (!isPlainRecord(rule) || typeof rule.id !== "string" || !rule.id || rule.id.length > 64 || identities.has(rule.id)
      || (rule.property_id !== null && typeof rule.property_id !== "string")
      || !colors.includes(rule.color as NotesDatabaseTableColorRule["color"])) throw new Error("Invalid conditional color rule");
    if (Object.keys(rule).some((key) => !["id", "property_id", "color", "filters"].includes(key))) throw new Error("Unknown conditional color setting");
    identities.add(rule.id);
    const filters = notesDatabaseParseFilters({ type: "and", filters: rule.filters });
    if (!filters.length) throw new Error("Conditional color requires a predicate");
    return { id: rule.id, property_id: rule.property_id, color: rule.color as NotesDatabaseTableColorRule["color"], filters };
  });
  return { frozen_property_id: frozen, columns, color_rules: rules };
}
