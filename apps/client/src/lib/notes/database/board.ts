import { notesDatabaseSerializeFilters } from "./filters";
import {
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  type NotesDatabaseTableColumn,
} from "./table";
import type {
  NotesDatabaseBoardConfiguration,
  NotesDatabaseBoardRowOpenMode,
  NotesDatabaseTableFilter,
  NotesDatabaseTableSort,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourceBoardGroup,
  NotesDataSourceBoardViewUpdate,
  NotesPage,
} from "$lib/notes/types";

type UnknownRecord = Record<string, unknown>;

const BOARD_GROUP_PROPERTY_TYPES = new Set<string>([
  "select",
  "multi_select",
  "status",
  "checkbox",
  "people",
  "relation",
  "date",
]);
const BOARD_MOVABLE_PROPERTY_TYPES = new Set<string>([
  "select",
  "status",
  "multi_select",
  "checkbox",
  "date",
]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function boardConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const board = configuration.board;
  return isRecord(board) ? board : {};
}

/** Read the persisted board configuration with local defaults for missing fields. */
export function notesDatabaseBoardConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseBoardConfiguration {
  const board = boardConfig(view);
  const groupPropertyId = typeof board.group_property_id === "string"
    ? board.group_property_id
    : null;
  const rowOpenMode: NotesDatabaseBoardRowOpenMode =
    board.row_open_mode === "side_panel" ? "side_panel" : "full_page";
  return {
    group_property_id: groupPropertyId,
    group_order: readStringArray(board.group_order),
    hidden_group_ids: readStringArray(board.hidden_group_ids),
    visible_property_ids: readStringArray(board.visible_property_ids),
    row_open_mode: rowOpenMode,
  };
}

/** Build board columns from the current data source schema. */
export function notesDatabaseBoardColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  return notesDatabaseTableColumns(dataSource, view);
}

/** Return properties that can produce board groups. */
export function notesDatabaseBoardGroupableColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => BOARD_GROUP_PROPERTY_TYPES.has(column.type));
}

/** Return the configured card properties, excluding the title and active group property. */
export function notesDatabaseBoardVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseBoardConfiguration,
): NotesDatabaseTableColumn[] {
  const byId = new Map(columns.map((column) => [column.id, column]));
  const groupPropertyId = configuration.group_property_id;
  const configured = configuration.visible_property_ids
    .map((id) => byId.get(id))
    .filter((column): column is NotesDatabaseTableColumn =>
      column !== undefined && column.type !== "title" && column.id !== groupPropertyId
    );
  if (configured.length > 0) return configured;
  return columns
    .filter((column) => column.type !== "title" && column.id !== groupPropertyId)
    .slice(0, 4);
}

/** Build the update payload expected by the local board Tauri command. */
export function notesDatabaseBoardUpdate(
  configuration: NotesDatabaseBoardConfiguration,
  groups: readonly NotesDataSourceBoardGroup[],
  visibleColumns: readonly NotesDatabaseTableColumn[],
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
): NotesDataSourceBoardViewUpdate {
  return {
    filter: notesDatabaseBoardFilters(filters),
    sorts: notesDatabaseBoardSorts(sorts),
    configuration: {
      group_property_id: configuration.group_property_id,
      group_order: groups.map((group) => group.id),
      hidden_group_ids: groups.filter((group) => group.hidden).map((group) => group.id),
      visible_property_ids: visibleColumns.map((column) => column.id),
      row_open_mode: configuration.row_open_mode,
    },
  };
}

/** Return filters stored on a board view. */
export function notesDatabaseBoardFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseTableFiltersFromView(view);
}

/** Return sorts stored on a board view. */
export function notesDatabaseBoardSortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return notesDatabaseTableSortsFromView(view);
}

/** Return user-facing text for a card property. */
export function notesDatabaseBoardCardText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  return notesDatabaseTableCellText(page, column);
}

/** Return the title displayed on a board card. */
export function notesDatabaseBoardCardTitle(
  page: NotesPage,
  columns: readonly NotesDatabaseTableColumn[],
  fallback: string,
): string {
  const titleColumn = columns.find((column) => column.type === "title");
  const title = titleColumn ? notesDatabaseTableCellText(page, titleColumn).trim() : "";
  return title || fallback;
}

/** Return true when dragging into a group can be persisted. */
export function notesDatabaseBoardCanMoveCards(
  groupColumn: NotesDatabaseTableColumn | null,
): boolean {
  return groupColumn !== null && BOARD_MOVABLE_PROPERTY_TYPES.has(groupColumn.type);
}

function notesDatabaseBoardFilters(
  filters: readonly NotesDatabaseTableFilter[],
): NotesDatabaseTableFilter[] {
  return notesDatabaseSerializeFilters(filters);
}

function notesDatabaseBoardSorts(
  sorts: readonly NotesDatabaseTableSort[],
): NotesDatabaseTableSort[] {
  return sorts.map((sort) => ({
    property_id: sort.property_id,
    direction: sort.direction,
  }));
}
