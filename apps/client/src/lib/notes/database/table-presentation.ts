import type { NotesDatabaseTableColumn } from "./table";
import { notesDatabaseListGroups, notesDatabaseListGroupableColumns } from "./list";
import type { NotesDatabaseTableCalculation, NotesDatabaseTableColumnPresentation, NotesDatabaseTableConfiguration, NotesDatabaseTablePresentation, NotesDatabaseView, NotesPage } from "$lib/notes/types";
import { NOTES_TABLE_CALCULATIONS, parseNotesTablePresentation } from "$lib/notes/validation/database/table-presentation";

export { NOTES_TABLE_CALCULATIONS, parseNotesTablePresentation } from "$lib/notes/validation/database/table-presentation";

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Read grouping and presentation from a validated saved table view. */
export function notesTableViewSettings(view: NotesDatabaseView): Pick<NotesDatabaseTableConfiguration, "group_property_id" | "group_order" | "collapsed_group_ids" | "hide_empty_groups" | "presentation"> {
  const table = isPlainRecord(view.configuration?.table) ? view.configuration.table : {};
  const strings = (value: unknown): string[] => Array.isArray(value) ? value.filter((id): id is string => typeof id === "string") : [];
  return {
    group_property_id: typeof table.group_property_id === "string" ? table.group_property_id : null,
    group_order: strings(table.group_order),
    collapsed_group_ids: strings(table.collapsed_group_ids),
    hide_empty_groups: table.hide_empty_groups === true,
    presentation: parseNotesTablePresentation(table.presentation),
  };
}

/** Return defaults for a stable property without mutating the saved view. */
export function notesTableColumnPresentation(presentation: NotesDatabaseTablePresentation | undefined, id: string): NotesDatabaseTableColumnPresentation {
  return presentation && Object.hasOwn(presentation.columns, id) ? presentation.columns[id] : { wrap: false, date_format: "locale", time_format: "locale", calculation: null };
}

/** Restrict numeric and checkbox calculations to compatible source properties. */
export function notesTableCompatibleCalculations(column: NotesDatabaseTableColumn): readonly NotesDatabaseTableCalculation[] {
  return NOTES_TABLE_CALCULATIONS.filter((calculation) => {
    if (["sum", "average", "min", "max"].includes(calculation)) return ["number", "rollup", "formula"].includes(column.type);
    if (calculation === "percent_checked") return column.type === "checkbox";
    return true;
  });
}

/** Return a sticky offset through the configured visible property boundary. */
export function notesTableFrozenOffset(columns: readonly NotesDatabaseTableColumn[], frozenId: string | null | undefined, propertyId: string, viewportWidth?: number): number | null {
  const boundary = columns.findIndex((column) => column.id === frozenId);
  const index = columns.findIndex((column) => column.id === propertyId);
  if (boundary < 0 || index < 0 || index > boundary) return null;
  const offset = columns.slice(0, index).reduce((width, column) => width + column.width, 0);
  const scrollingWidths = columns.slice(1).map((column) => column.width);
  const reserve = scrollingWidths.length ? Math.min(...scrollingWidths) : columns[0].width;
  if (viewportWidth !== undefined && offset + columns[index].width > Math.max(0, viewportWidth - reserve)) return null;
  return offset;
}

/** Combine complete-source group counts with the currently hydrated row window. */
export function notesTableGroups(rows: readonly NotesPage[], columns: readonly NotesDatabaseTableColumn[], configuration: NotesDatabaseTableConfiguration, counts: Readonly<Record<string, number>>) {
  const groups = notesDatabaseListGroups(rows, columns, {
    group_property_id: configuration.group_property_id ?? null,
    group_order: configuration.group_order ?? [], hidden_group_ids: [], visible_property_ids: [], row_open_mode: configuration.row_open_mode,
  });
  const groupedColumn = columns.find((column) => column.id === configuration.group_property_id);
  if (groupedColumn?.type === "people" || groupedColumn?.type === "relation") {
    const people = rows.flatMap((row) => Object.values(row.properties).flatMap((value) => isPlainRecord(value) && value.id === groupedColumn.id && Array.isArray(value[groupedColumn.type]) ? value[groupedColumn.type] as unknown[] : []));
    for (const group of groups) {
      if (group.id === "__empty__") continue;
      const person = people.find((value) => isPlainRecord(value) && value.id === group.id);
      group.name = isPlainRecord(person) && typeof person.name === "string" && person.name ? person.name
        : isPlainRecord(person) && typeof person.title === "string" && person.title ? person.title : group.id;
    }
  }
  for (const id of Object.keys(counts)) {
    if (!groups.some((group) => group.id === id)) groups.push({ id, name: id, color: "default", hidden: false, rows: [] });
  }
  const order = configuration.group_order ?? [];
  groups.sort((left, right) => {
    const leftIndex = order.indexOf(left.id); const rightIndex = order.indexOf(right.id);
    return (leftIndex < 0 ? order.length : leftIndex) - (rightIndex < 0 ? order.length : rightIndex);
  });
  return groups.map((group) => ({ ...group, count: Object.hasOwn(counts, group.id) ? counts[group.id] : group.rows.length,
    collapsed: configuration.collapsed_group_ids?.includes(group.id) ?? false,
  })).filter((group) => !configuration.hide_empty_groups || group.count > 0 || group.rows.length > 0);
}

export { notesDatabaseListGroupableColumns as notesTableGroupableColumns };
