import { formatNumber } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type {
  ProjectCustomField,
  ProjectCustomFieldType,
  ProjectCustomFieldValue,
} from "$lib/projects/types";

/** Drag data type that keeps a dragged custom field option from landing in unrelated drop targets. */
export const PROJECT_CUSTOM_FIELD_OPTION_DRAG_DATA_TYPE = "application/x-ganbaru-project-custom-field-option";

const TEXT_BACKED_CUSTOM_FIELD_TYPES = new Set<ProjectCustomFieldType>([
  "text",
  "url",
  "phone",
  "email",
  "person",
  "files",
]);

const OPTION_BACKED_CUSTOM_FIELD_TYPES = new Set<ProjectCustomFieldType>([
  "select",
  "multi_select",
  "status",
]);

export function projectCustomFieldUsesTextValue(fieldType: ProjectCustomFieldType): boolean {
  return TEXT_BACKED_CUSTOM_FIELD_TYPES.has(fieldType);
}

export function projectCustomFieldUsesOptions(fieldType: ProjectCustomFieldType): boolean {
  return OPTION_BACKED_CUSTOM_FIELD_TYPES.has(fieldType);
}

export function projectCustomFieldAllowsMultipleOptions(fieldType: ProjectCustomFieldType): boolean {
  return fieldType === "multi_select";
}

export function projectCustomFieldInputType(fieldType: ProjectCustomFieldType): "email" | "tel" | "text" | "url" {
  if (fieldType === "email") return "email";
  if (fieldType === "phone") return "tel";
  if (fieldType === "url") return "url";
  return "text";
}

export function projectCustomFieldTextInputMode(fieldType: ProjectCustomFieldType): "email" | "tel" | "text" | "url" {
  if (fieldType === "email") return "email";
  if (fieldType === "phone") return "tel";
  if (fieldType === "url") return "url";
  return "text";
}

export function projectCustomFieldTextValue(
  value: ProjectCustomFieldValue | undefined,
): string {
  return value?.textValue?.trim() ?? "";
}

export function projectCustomFieldDisplayText(
  value: ProjectCustomFieldValue | undefined,
  field: ProjectCustomField,
): string | undefined {
  if (projectCustomFieldUsesTextValue(field.fieldType)) {
    return projectCustomFieldTextValue(value) || undefined;
  }
  if (field.fieldType === "number") {
    const number = value?.numberValue;
    if (number === undefined) return undefined;
    return Number.isInteger(number) ? number.toFixed(0) : number.toString();
  }
  if (field.fieldType === "date") return value?.dateValue || undefined;
  if (field.fieldType === "checkbox") {
    if (value?.checkboxValue === undefined) return undefined;
    return value.checkboxValue ? "true" : "false";
  }
  return undefined;
}

/**
 * Return `base`, or `base (n)` with the first localized number from 2 that no existing field name uses, ignoring case.
 * Duplicates and properties created without a name use it so the new field never collides with the project's schema.
 */
export function uniqueProjectCustomFieldName(
  base: string,
  fields: readonly Pick<ProjectCustomField, "name">[],
  locale: AppLocale,
): string {
  const taken = new Set(fields.map((field) => field.name.trim().toLocaleLowerCase()));
  let name = base.trim();
  for (let suffix = 2; taken.has(name.toLocaleLowerCase()); suffix += 1) name = `${base.trim()} (${formatNumber(locale, suffix)})`;
  return name;
}
