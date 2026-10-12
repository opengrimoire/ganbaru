import type {
  NotesDatabaseTableConfiguration,
  NotesDatabaseTableFilter,
  NotesDatabaseTableRowOpenMode,
  NotesDatabaseTableSort,
  NotesDatabaseTableSortDirection,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourcePropertyType,
  NotesPage,
  NotesDataSourceNumberFormat,
} from "$lib/notes/types";
import { notesDatabaseParseFilters, notesDatabaseSerializeFilters } from "./filters";
import { NOTES_DATA_SOURCE_NUMBER_FORMATS } from "$lib/notes/types";
import { notesDatabaseDateValue, type NotesDatabaseDateValue } from "./date";
import type { NotesDatabasePropertyDisplayFormat } from "./property-display";
import { notesDataSourcePropertyIcon } from "./data-source-schema";

type UnknownRecord = Record<string, unknown>;

export interface NotesDatabaseTableColumn {
  id: string;
  name: string;
  type: NotesDataSourcePropertyType;
  hidden: boolean;
  width: number;
  options: NotesDatabaseTableOption[];
  relationDataSourceId: string | null;
  buttonLabel: string;
  buttonRequiresConfirmation: boolean;
  numberFormat?: NotesDataSourceNumberFormat;
  displayFormat?: NotesDatabasePropertyDisplayFormat;
  /** A `lucide:` picker value; absent or null shows the type icon. */
  icon?: string | null;
}

export interface NotesDatabaseTableOption {
  id: string;
  name: string;
  color: string;
}

export interface NotesDatabaseTableRelationItem {
  id: string;
  title: string;
}

export type NotesDatabaseTableEditValue = string | boolean | string[] | NotesDatabaseDateValue | null;

const DEFAULT_COLUMN_WIDTH = 180;
const TITLE_COLUMN_WIDTH = 220;
const MIN_COLUMN_WIDTH = 96;
const MAX_COLUMN_WIDTH = 480;
const TABLE_SORT_DIRECTIONS = new Set<NotesDatabaseTableSortDirection>([
  "ascending",
  "descending",
]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function tableConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const table = configuration.table;
  return isRecord(table) ? table : {};
}

export function notesDatabaseTableConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseTableConfiguration {
  const table = tableConfig(view);
  const rawWidths = isRecord(table.column_widths) ? table.column_widths : {};
  const column_widths: Record<string, number> = {};
  for (const [propertyId, width] of Object.entries(rawWidths)) {
    if (typeof width !== "number" || !Number.isInteger(width)) continue;
    column_widths[propertyId] = clampColumnWidth(width);
  }
  const rowOpenMode = table.row_open_mode === "side_panel" ? "side_panel" : "full_page";
  return {
    property_order: readStringArray(table.property_order),
    hidden_property_ids: readStringArray(table.hidden_property_ids).filter((id) => id !== "title"),
    column_widths,
    row_open_mode: rowOpenMode,
    collapsed_row_ids: readStringArray(table.collapsed_row_ids),
  };
}

export function notesDatabaseTableColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  const configuration = notesDatabaseTableConfigurationFromView(view);
  const hidden = new Set(configuration.hidden_property_ids);
  const byId = new Map<string, NotesDatabaseTableColumn>();
  for (const [key, rawProperty] of Object.entries(dataSource.properties)) {
    if (!isRecord(rawProperty)) continue;
    const type = readString(rawProperty.type, "rich_text") as NotesDataSourcePropertyType;
    const id = type === "title" ? "title" : readString(rawProperty.id, key);
    byId.set(id, {
      id,
      name: readString(rawProperty.name, key),
      type,
      hidden: type === "title" ? false : hidden.has(id),
      width: configuration.column_widths[id] ?? (type === "title" ? TITLE_COLUMN_WIDTH : DEFAULT_COLUMN_WIDTH),
      options: propertyOptions(rawProperty, type),
      relationDataSourceId: relationDataSourceId(rawProperty, type),
      buttonLabel: buttonLabel(rawProperty, type),
      buttonRequiresConfirmation: buttonRequiresConfirmation(rawProperty, type),
      numberFormat: numberFormat(rawProperty, type),
      icon: notesDataSourcePropertyIcon(rawProperty.icon),
    });
  }
  const columns: NotesDatabaseTableColumn[] = [];
  for (const propertyId of ["title", ...configuration.property_order.filter((id) => id !== "title")]) {
    const column = byId.get(propertyId);
    if (!column) continue;
    columns.push(column);
    byId.delete(propertyId);
  }
  columns.push(...Array.from(byId.values()).sort((left, right) => left.name.localeCompare(right.name)));
  return columns;
}

export function notesDatabaseTableVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => !column.hidden);
}

export function notesDatabaseTableFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseParseFilters(view.filter);
}

export function notesDatabaseTableSortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return view.sorts.flatMap((sort) => {
    const propertyId = readString(sort.property_id);
    const direction = sort.direction;
    if (!propertyId || !TABLE_SORT_DIRECTIONS.has(direction as NotesDatabaseTableSortDirection)) {
      return [];
    }
    return [{
      property_id: propertyId,
      direction: direction as NotesDatabaseTableSortDirection,
    }];
  });
}

export function notesDatabaseTableUpdate(
  columns: readonly NotesDatabaseTableColumn[],
  rowOpenMode: NotesDatabaseTableRowOpenMode,
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
  settings?: Partial<NotesDatabaseTableConfiguration>,
): {
  filter: NotesDatabaseTableFilter[];
  sorts: NotesDatabaseTableSort[];
  configuration: NotesDatabaseTableConfiguration;
} {
  const propertyOrder = columns.map((column) => column.id);
  const columnWidths: Record<string, number> = {};
  for (const column of columns) {
    columnWidths[column.id] = clampColumnWidth(column.width);
  }
  return {
    filter: notesDatabaseSerializeFilters(filters),
    sorts: sorts.map((sort) => ({
      property_id: sort.property_id,
      direction: sort.direction,
    })),
    configuration: {
      ...settings,
      property_order: propertyOrder,
      hidden_property_ids: columns
        .filter((column) => column.hidden && column.type !== "title")
        .map((column) => column.id),
      column_widths: columnWidths,
      row_open_mode: rowOpenMode,
    },
  };
}

export function notesDatabaseTableCellText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  const property = Object.values(page.properties).find((value) => propertyMatchesColumn(value, column));
  if (!isRecord(property)) return "";
  const payload = property[column.type];
  switch (column.type) {
    case "title":
    case "rich_text":
      return Array.isArray(payload) ? richTextPlainText(payload) : "";
    case "number":
      return typeof payload === "number" ? String(payload) : "";
    case "checkbox":
      return payload === true ? "true" : "false";
    case "select":
    case "status":
      return isRecord(payload) ? readString(payload.name) : "";
    case "multi_select":
      return Array.isArray(payload)
        ? payload.filter(isRecord).map((item) => readString(item.name)).filter(Boolean).join(", ")
        : "";
    case "relation":
      return notesDatabaseTableRelationItems(page, column)
        .map((item) => item.title || item.id)
        .join(", ");
    case "rollup":
      return rollupPlainText(payload);
    case "formula":
      return formulaPlainText(payload);
    case "button":
      return isRecord(payload) ? readString(payload.label, column.buttonLabel || column.name) : column.buttonLabel;
    case "date":
      return isRecord(payload) ? readString(payload.start) : "";
    case "url":
    case "email":
    case "phone_number":
      return typeof payload === "string" ? payload : "";
    case "unique_id":
      if (!isRecord(payload)) return "";
      return `${readString(payload.prefix)}${typeof payload.number === "number" ? payload.number : ""}`;
    case "place":
      return isRecord(payload) ? readString(payload.name) : "";
    case "created_time":
    case "last_edited_time":
      return readString(payload, column.type === "created_time" ? page.created_time : page.last_edited_time);
    case "created_by":
    case "last_edited_by":
      return isRecord(payload) ? readString(payload.name) || readString(payload.id) : "";
    case "files":
    case "people":
      return Array.isArray(payload) ? payload.filter(isRecord).map((item) => readString(item.name) || readString(item.id)).filter(Boolean).join(", ") : "";
  }
}

/**
 * Read a row's title from its title property, whatever that property is named, without needing the source schema.
 *
 * @param page Row page with hydrated property values.
 * @param fallback Text shown when the title is blank, such as a localized "Untitled".
 */
export function notesDatabaseRowTitle(page: NotesPage, fallback: string): string {
  for (const value of Object.values(page.properties)) {
    if (!isRecord(value) || value.type !== "title" || !Array.isArray(value.title)) continue;
    const title = richTextPlainText(value.title).trim();
    if (title) return title;
  }
  return fallback;
}

/** Read exact option labels without splitting names that contain punctuation. */
export function notesDatabaseTableOptionNames(page: NotesPage, column: NotesDatabaseTableColumn): string[] {
  const property = Object.values(page.properties).find((value) => propertyMatchesColumn(value, column));
  if (!isRecord(property)) return [];
  const payload = property[column.type];
  if (column.type === "multi_select") {
    return Array.isArray(payload) ? payload.filter(isRecord).map((item) => readString(item.name)).filter(Boolean) : [];
  }
  return (column.type === "select" || column.type === "status") && isRecord(payload) && readString(payload.name)
    ? [readString(payload.name)] : [];
}

export function notesDatabaseTableCellEditValue(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): NotesDatabaseTableEditValue {
  if (column.type === "checkbox") {
    return notesDatabaseTableCellText(page, column) === "true";
  }
  if (column.type === "relation") {
    return notesDatabaseTableRelationItems(page, column).map((item) => item.id);
  }
  if (column.type === "date") return notesDatabaseTableDateValue(page, column);
  return notesDatabaseTableCellText(page, column);
}

/** Read a row's complete date range for editing without losing its timezone. */
export function notesDatabaseTableDateValue(page: NotesPage, column: NotesDatabaseTableColumn): NotesDatabaseDateValue | null {
  const property = Object.values(page.properties).find((value) => propertyMatchesColumn(value, column));
  return isRecord(property) ? notesDatabaseDateValue(property.date) : null;
}

export function notesDatabaseTableColumnCanEdit(column: NotesDatabaseTableColumn): boolean {
  return ![
    "files",
    "people",
    "created_time",
    "created_by",
    "last_edited_time",
    "last_edited_by",
    "unique_id",
    "rollup",
    "formula",
    "button",
  ].includes(column.type);
}

export function notesDatabaseTableColumnWidth(
  column: NotesDatabaseTableColumn,
  delta: number,
): number {
  return clampColumnWidth(column.width + delta);
}

export function notesDatabaseTableRelationItems(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): NotesDatabaseTableRelationItem[] {
  const property = Object.values(page.properties).find((value) =>
    propertyMatchesColumn(value, column)
  );
  if (!isRecord(property) || !Array.isArray(property.relation)) return [];
  return property.relation.filter(isRecord).flatMap((item) => {
    const id = readString(item.id);
    if (!id) return [];
    return [{
      id,
      title: readString(item.title, id),
    }];
  });
}

export function notesDatabaseTableEditValuesEqual(
  left: NotesDatabaseTableEditValue,
  right: NotesDatabaseTableEditValue,
): boolean {
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right)) return false;
    if (left.length !== right.length) return false;
    return left.every((item, index) => item === right[index]);
  }
  if (typeof left === "object" && left !== null || typeof right === "object" && right !== null) {
    if (typeof left !== "object" || left === null || typeof right !== "object" || right === null) return false;
    return left.start === right.start && left.end === right.end && left.time_zone === right.time_zone;
  }
  return left === right;
}

function numberFormat(property: UnknownRecord, type: NotesDataSourcePropertyType): NotesDataSourceNumberFormat {
  const config = type === "number" && isRecord(property.number) ? property.number : {};
  return NOTES_DATA_SOURCE_NUMBER_FORMATS.find((format) => format === config.format) ?? "number";
}

function clampColumnWidth(width: number): number {
  return Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, Math.round(width)));
}

function propertyOptions(
  property: UnknownRecord,
  type: NotesDataSourcePropertyType,
): NotesDatabaseTableOption[] {
  if (type !== "select" && type !== "multi_select" && type !== "status") return [];
  const config = property[type];
  if (!isRecord(config) || !Array.isArray(config.options)) return [];
  return config.options.filter(isRecord).map((option) => ({
    id: readString(option.id),
    name: readString(option.name),
    color: readString(option.color, "default"),
  })).filter((option) => option.id && option.name);
}

function relationDataSourceId(
  property: UnknownRecord,
  type: NotesDataSourcePropertyType,
): string | null {
  if (type !== "relation") return null;
  const config = property.relation;
  if (!isRecord(config)) return null;
  const dataSourceId = readString(config.data_source_id);
  return dataSourceId || null;
}

function buttonLabel(property: UnknownRecord, type: NotesDataSourcePropertyType): string {
  if (type !== "button") return "";
  const config = property.button;
  if (!isRecord(config)) return "Run";
  return readString(config.label, "Run");
}

function buttonRequiresConfirmation(
  property: UnknownRecord,
  type: NotesDataSourcePropertyType,
): boolean {
  if (type !== "button") return false;
  const config = property.button;
  return isRecord(config) && config.requires_confirmation === true;
}

function propertyMatchesColumn(value: unknown, column: NotesDatabaseTableColumn): boolean {
  return (
    isRecord(value)
    && value.id === column.id
    && value.type === column.type
  );
}

function richTextPlainText(items: unknown[]): string {
  return items.filter(isRecord).map((item) => {
    if (typeof item.plain_text === "string") return item.plain_text;
    const text = item.text;
    return isRecord(text) ? readString(text.content) : "";
  }).join("");
}

function rollupPlainText(value: unknown): string {
  if (!isRecord(value)) return "";
  const type = readString(value.type);
  switch (type) {
    case "number":
      return typeof value.number === "number" ? String(value.number) : "";
    case "date":
      return datePlainText(value.date);
    case "array":
      return Array.isArray(value.array)
        ? value.array.map(propertyPlainText).filter(Boolean).join(", ")
        : "";
    default:
      return "";
  }
}

function formulaPlainText(value: unknown): string {
  if (!isRecord(value)) return "";
  const type = readString(value.type);
  switch (type) {
    case "number":
      return typeof value.number === "number" ? String(value.number) : "";
    case "boolean":
      return typeof value.boolean === "boolean" ? String(value.boolean) : "";
    case "date":
      return datePlainText(value.date);
    case "string":
      return readString(value.string);
    default:
      return "";
  }
}

function propertyPlainText(value: unknown): string {
  if (!isRecord(value)) return "";
  const type = readString(value.type);
  const payload = value[type];
  switch (type) {
    case "title":
    case "rich_text":
      return Array.isArray(payload) ? richTextPlainText(payload) : "";
    case "number":
      return typeof payload === "number" ? String(payload) : "";
    case "checkbox":
      return payload === true ? "true" : payload === false ? "false" : "";
    case "select":
    case "status":
    case "place":
      return isRecord(payload) ? readString(payload.name) : "";
    case "multi_select":
    case "files":
    case "people":
    case "relation":
      return Array.isArray(payload)
        ? payload
            .filter(isRecord)
            .map((item) => readString(item.title) || readString(item.name) || readString(item.id))
            .filter(Boolean)
            .join(", ")
        : "";
    case "date":
      return datePlainText(payload);
    case "url":
    case "email":
    case "phone_number":
    case "created_time":
    case "last_edited_time":
      return typeof payload === "string" ? payload : "";
    case "unique_id":
      return isRecord(payload)
        ? `${readString(payload.prefix)}${typeof payload.number === "number" ? payload.number : ""}`
        : "";
    case "button":
      return isRecord(payload) ? readString(payload.label) : "";
    default:
      return "";
  }
}

function datePlainText(value: unknown): string {
  if (!isRecord(value)) return "";
  const start = readString(value.start);
  const end = readString(value.end);
  return end && end !== start ? `${start} to ${end}` : start;
}
