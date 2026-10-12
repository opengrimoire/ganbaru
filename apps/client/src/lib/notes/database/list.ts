import { notesDatabaseSerializeFilters } from "./filters";
import {
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  type NotesDatabaseTableColumn,
} from "./table";
import type {
  NotesDatabaseListConfiguration,
  NotesDatabaseListRowOpenMode,
  NotesDatabaseTableFilter,
  NotesDatabaseTableSort,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourceListGroup,
  NotesDataSourceListViewUpdate,
  NotesPage,
} from "$lib/notes/types";

type UnknownRecord = Record<string, unknown>;

const LIST_EMPTY_GROUP_ID = "__empty__";
const LIST_UNGROUPED_ID = "__ungrouped__";
const LIST_GROUP_PROPERTY_TYPES = new Set<string>([
  "select",
  "multi_select",
  "status",
  "checkbox",
  "people",
  "relation",
  "date",
]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function listConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const list = configuration.list;
  return isRecord(list) ? list : {};
}

/** Read the persisted list configuration with local defaults. */
export function notesDatabaseListConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseListConfiguration {
  const list = listConfig(view);
  const rowOpenMode: NotesDatabaseListRowOpenMode =
    list.row_open_mode === "full_page" ? "full_page" : "side_panel";
  return {
    group_property_id: typeof list.group_property_id === "string" ? list.group_property_id : null,
    group_order: readStringArray(list.group_order),
    hidden_group_ids: readStringArray(list.hidden_group_ids),
    visible_property_ids: readStringArray(list.visible_property_ids),
    row_open_mode: rowOpenMode,
  };
}

/** Build list columns from the current data source schema. */
export function notesDatabaseListColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  return notesDatabaseTableColumns(dataSource, view);
}

/** Return properties that can produce compact list groups. */
export function notesDatabaseListGroupableColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => LIST_GROUP_PROPERTY_TYPES.has(column.type));
}

/** Return the configured properties shown to the right of the list title. */
export function notesDatabaseListVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseListConfiguration,
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

/** Build row groups from the current sorted list rows and persisted grouping settings. */
export function notesDatabaseListGroups(
  rows: readonly NotesPage[],
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseListConfiguration,
): NotesDataSourceListGroup[] {
  const groupColumn = configuration.group_property_id
    ? columns.find((column) => column.id === configuration.group_property_id) ?? null
    : null;
  if (!groupColumn) {
    return [{
      id: LIST_UNGROUPED_ID,
      name: "All rows",
      color: "default",
      hidden: false,
      rows: [...rows],
    }];
  }

  const groups = new Map<string, NotesDataSourceListGroup>();
  for (const group of initialGroups(groupColumn, configuration)) {
    groups.set(group.id, group);
  }
  for (const row of rows) {
    for (const groupId of rowGroupIds(row, groupColumn)) {
      const group = groups.get(groupId) ?? dynamicGroup(groupId, groupColumn);
      group.rows.push(row);
      groups.set(groupId, group);
    }
  }

  const ordered: NotesDataSourceListGroup[] = [];
  for (const groupId of configuration.group_order) {
    const group = groups.get(groupId);
    if (!group) continue;
    ordered.push(group);
    groups.delete(groupId);
  }
  ordered.push(...groups.values());
  return ordered;
}

/** Build the update payload expected by the local list Tauri command. */
export function notesDatabaseListUpdate(
  configuration: NotesDatabaseListConfiguration,
  visibleColumns: readonly NotesDatabaseTableColumn[],
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
): NotesDataSourceListViewUpdate {
  return {
    filter: notesDatabaseSerializeFilters(filters),
    sorts: sorts.map((sort) => ({
      property_id: sort.property_id,
      direction: sort.direction,
    })),
    configuration: {
      group_property_id: configuration.group_property_id,
      group_order: configuration.group_order,
      hidden_group_ids: configuration.hidden_group_ids,
      visible_property_ids: visibleColumns.map((column) => column.id),
      row_open_mode: configuration.row_open_mode,
    },
  };
}

/** Return filters stored on a list view. */
export function notesDatabaseListFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseTableFiltersFromView(view);
}

/** Return sorts stored on a list view. */
export function notesDatabaseListSortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return notesDatabaseTableSortsFromView(view);
}

/** Return user-facing text for a list property. */
export function notesDatabaseListRowText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  return notesDatabaseTableCellText(page, column);
}

/** Return the title displayed for a list item. */
export function notesDatabaseListRowTitle(
  page: NotesPage,
  columns: readonly NotesDatabaseTableColumn[],
  fallback: string,
): string {
  const titleColumn = columns.find((column) => column.type === "title");
  const title = titleColumn ? notesDatabaseTableCellText(page, titleColumn).trim() : "";
  return title || fallback;
}

function initialGroups(
  groupColumn: NotesDatabaseTableColumn,
  configuration: NotesDatabaseListConfiguration,
): NotesDataSourceListGroup[] {
  switch (groupColumn.type) {
    case "select":
    case "multi_select":
    case "status":
      return [
        ...groupColumn.options.map((option) => createGroup(option.id, option.name, option.color, configuration)),
        createGroup(LIST_EMPTY_GROUP_ID, "No value", "default", configuration),
      ];
    case "checkbox":
      return [
        createGroup("false", "Unchecked", "gray", configuration),
        createGroup("true", "Checked", "green", configuration),
      ];
    case "date":
    case "people":
      return [createGroup(LIST_EMPTY_GROUP_ID, "No value", "default", configuration)];
    default:
      return [createGroup(LIST_UNGROUPED_ID, "All rows", "default", configuration)];
  }
}

function createGroup(
  id: string,
  name: string,
  color: string,
  configuration: NotesDatabaseListConfiguration,
): NotesDataSourceListGroup {
  return {
    id,
    name,
    color,
    hidden: configuration.hidden_group_ids.includes(id),
    rows: [],
  };
}

function rowGroupIds(row: NotesPage, groupColumn: NotesDatabaseTableColumn): string[] {
  const payload = rowPropertyPayload(row, groupColumn);
  switch (groupColumn.type) {
    case "select":
    case "status":
      return isRecord(payload) && typeof payload.id === "string" && payload.id
        ? [payload.id]
        : [LIST_EMPTY_GROUP_ID];
    case "multi_select":
      if (!Array.isArray(payload)) return [LIST_EMPTY_GROUP_ID];
      return nonEmptyGroupIds(payload
        .filter(isRecord)
        .map((item) => item.id)
        .filter((id): id is string => typeof id === "string" && id.length > 0));
    case "checkbox":
      return typeof payload === "boolean" ? [String(payload)] : ["false"];
    case "date":
      return isRecord(payload) && typeof payload.start === "string" && payload.start
        ? [payload.start]
        : [LIST_EMPTY_GROUP_ID];
    case "people":
    case "relation":
      if (!Array.isArray(payload)) return [LIST_EMPTY_GROUP_ID];
      return nonEmptyGroupIds(payload
        .filter(isRecord)
        .map((item) => item.id)
        .filter((id): id is string => typeof id === "string" && id.length > 0));
    default:
      return [LIST_UNGROUPED_ID];
  }
}

function nonEmptyGroupIds(groupIds: string[]): string[] {
  return groupIds.length > 0 ? [...new Set(groupIds)] : [LIST_EMPTY_GROUP_ID];
}

function dynamicGroup(
  groupId: string,
  groupColumn: NotesDatabaseTableColumn,
): NotesDataSourceListGroup {
  if (groupId === LIST_EMPTY_GROUP_ID) {
    return { id: groupId, name: "No value", color: "default", hidden: false, rows: [] };
  }
  if (groupColumn.type === "date") {
    return { id: groupId, name: groupId, color: "blue", hidden: false, rows: [] };
  }
  if (groupColumn.type === "people") {
    return { id: groupId, name: `Person ${groupId}`, color: "purple", hidden: false, rows: [] };
  }
  return { id: groupId, name: groupId, color: "default", hidden: false, rows: [] };
}

function rowPropertyPayload(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): unknown {
  const property = Object.values(page.properties).find((value) =>
    isRecord(value) && value.id === column.id && value.type === column.type
  );
  return isRecord(property) ? property[column.type] : null;
}
