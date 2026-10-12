import type { CollectionPropertyKind, CollectionPropertyTypeOption } from "$lib/components/collections/property-icons";
import type { Translate } from "$lib/i18n/translator.svelte";
import { projectCustomFieldTypeLabel } from "$lib/projects/display";
import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
import { PROJECT_CUSTOM_FIELD_TYPES, type ProjectCoreTaskListColumn, type ProjectCustomField, type ProjectCustomFieldType } from "$lib/projects/types";

const CUSTOM_FIELD_KINDS = {
  text: "text",
  number: "number",
  select: "select",
  multi_select: "multi_select",
  status: "status",
  date: "date",
  person: "person",
  files: "files",
  checkbox: "checkbox",
  url: "url",
  phone: "phone",
  email: "email",
} as const satisfies Record<ProjectCustomFieldType, CollectionPropertyKind>;

const CORE_COLUMN_KINDS = {
  name: "title",
  status: "status",
  start: "date",
  due: "date",
  priority: "select",
  assignee: "person",
  reviewer: "person",
  estimate: "number",
  scheduled: "date",
  dependencies: "relation",
} as const satisfies Record<"name" | ProjectCoreTaskListColumn, CollectionPropertyKind>;

/** Return the shared property kind for a custom field type, so every type picker and header uses the same icon. */
export function projectCustomFieldKind(type: ProjectCustomFieldType): CollectionPropertyKind {
  return CUSTOM_FIELD_KINDS[type];
}

/** Return the shared property kind for a task list column, so its header and menu show the icon of its value type. */
export function projectListColumnKind(column: ProjectTaskListResizableColumn, field: Pick<ProjectCustomField, "fieldType"> | undefined): CollectionPropertyKind {
  if (field) return CUSTOM_FIELD_KINDS[field.fieldType];
  return isCoreColumn(column) ? CORE_COLUMN_KINDS[column] : "text";
}

function isCoreColumn(column: string): column is keyof typeof CORE_COLUMN_KINDS {
  return Object.hasOwn(CORE_COLUMN_KINDS, column);
}

/** List every creatable custom field type for the shared property creator. */
export function projectCustomFieldTypeOptions(t: Translate): CollectionPropertyTypeOption<ProjectCustomFieldType>[] {
  return PROJECT_CUSTOM_FIELD_TYPES.map((type) => ({ value: type, label: projectCustomFieldTypeLabel(type, t), kind: CUSTOM_FIELD_KINDS[type] }));
}
