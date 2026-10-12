import {
  PROJECT_TASK_LIST_COLUMNS,
  type ProjectCoreTaskListColumn,
  type ProjectCustomFieldReference,
  type ProjectTaskListColumn,
  type ProjectViewPreference,
} from "$lib/projects/types";

export const TASK_LIST_COLUMNS_PREFERENCE_KEY = "list-visible-columns";
export const CUSTOM_FIELD_REFERENCE_PREFIX = "custom:";

export const DEFAULT_TASK_LIST_COLUMNS: ProjectCoreTaskListColumn[] = [
  "status",
  "start",
  "due",
  "priority",
  "assignee",
  "reviewer",
];
const CORE_TASK_LIST_COLUMNS: ProjectCoreTaskListColumn[] = [...PROJECT_TASK_LIST_COLUMNS];

function isCoreTaskListColumn(value: unknown): value is ProjectCoreTaskListColumn {
  return typeof value === "string"
    && CORE_TASK_LIST_COLUMNS.includes(value as ProjectCoreTaskListColumn);
}

export function customFieldReference(fieldId: string): ProjectCustomFieldReference {
  return `${CUSTOM_FIELD_REFERENCE_PREFIX}${fieldId}`;
}

export function customFieldIdFromCustomFieldReference(value: string): string | undefined {
  if (!value.startsWith(CUSTOM_FIELD_REFERENCE_PREFIX)) return undefined;
  const fieldId = value.slice(CUSTOM_FIELD_REFERENCE_PREFIX.length);
  return fieldId.trim() ? fieldId : undefined;
}

export function customTaskListColumn(fieldId: string): ProjectTaskListColumn {
  return customFieldReference(fieldId);
}

export function customFieldIdFromTaskListColumn(column: ProjectTaskListColumn): string | undefined {
  return customFieldIdFromCustomFieldReference(column);
}

function isCustomTaskListColumn(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
): value is ProjectTaskListColumn {
  if (typeof value !== "string") return false;
  const fieldId = customFieldIdFromCustomFieldReference(value);
  if (!fieldId) return false;
  return customFieldIds ? customFieldIds.has(fieldId) : true;
}

function isTaskListColumn(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
): value is ProjectTaskListColumn {
  return isCoreTaskListColumn(value) || isCustomTaskListColumn(value, customFieldIds);
}

function uniqueTaskListColumns(
  values: readonly unknown[],
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumn[] {
  const columns: ProjectTaskListColumn[] = [];
  for (const value of values) {
    if (isTaskListColumn(value, customFieldIds) && !columns.includes(value)) {
      columns.push(value);
    }
  }
  return columns;
}

export function taskListColumnsMatch(
  first: readonly ProjectTaskListColumn[],
  second: readonly ProjectTaskListColumn[],
): boolean {
  return first.length === second.length && first.every((column, index) => column === second[index]);
}

export function parseTaskListColumns(
  value: string | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumn[] {
  if (value === undefined) return [...DEFAULT_TASK_LIST_COLUMNS];
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return [...DEFAULT_TASK_LIST_COLUMNS];
  }
  const rawColumns = Array.isArray(parsed)
    ? parsed
    : typeof parsed === "object" && parsed !== null && "visibleColumns" in parsed
      ? (parsed as { visibleColumns?: unknown }).visibleColumns
      : undefined;
  if (!Array.isArray(rawColumns)) return [...DEFAULT_TASK_LIST_COLUMNS];
  return uniqueTaskListColumns(rawColumns, customFieldIds);
}

export function taskListColumnsPreferenceValue(columns: readonly ProjectTaskListColumn[]): string {
  return JSON.stringify({ visibleColumns: uniqueTaskListColumns(columns) });
}

export function taskListColumnsForProject(
  preferences: readonly ProjectViewPreference[],
  projectId: string | null | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumn[] {
  if (!projectId) return [...DEFAULT_TASK_LIST_COLUMNS];
  const preference = preferences.find((entry) =>
    entry.projectId === projectId
    && entry.viewId === "list"
    && entry.preferenceKey === TASK_LIST_COLUMNS_PREFERENCE_KEY
  );
  return parseTaskListColumns(preference?.preferenceValue, customFieldIds);
}
