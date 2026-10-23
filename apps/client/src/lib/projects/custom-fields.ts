import { formatNumber } from "$lib/i18n/formatters";
import type { AppLocale } from "$lib/i18n/locales";
import type {
  ProjectCustomField,
  ProjectCustomFieldType,
  ProjectCustomFieldValue,
} from "$lib/projects/types";

/** Drag data type that keeps a dragged custom field option from landing in unrelated drop targets. */
export const PROJECT_CUSTOM_FIELD_OPTION_DRAG_DATA_TYPE = "application/x-ganbaru-project-custom-field-option";

/** Decimal places a number custom field keeps; native code stores values as signed 64-bit millionths. */
export const PROJECT_CUSTOM_FIELD_NUMBER_DECIMAL_PLACES = 6;

const NUMBER_SCALE = 10n ** BigInt(PROJECT_CUSTOM_FIELD_NUMBER_DECIMAL_PLACES);
const NUMBER_MIN_UNITS = -(2n ** 63n);
const NUMBER_MAX_UNITS = 2n ** 63n - 1n;
const NUMBER_PATTERN = /^(-?)(\d+)(?:\.(\d+))?$/;

/** Convert a decimal string to signed millionths, or null when it is malformed, too precise, or out of range. */
function projectCustomFieldNumberUnits(text: string): bigint | null {
  const match = NUMBER_PATTERN.exec(text);
  if (!match) return null;
  const [, sign, whole, fraction = ""] = match;
  const significantFraction = fraction.replace(/0+$/, "");
  if (significantFraction.length > PROJECT_CUSTOM_FIELD_NUMBER_DECIMAL_PLACES) return null;
  const magnitude = BigInt(whole) * NUMBER_SCALE
    + BigInt(significantFraction.padEnd(PROJECT_CUSTOM_FIELD_NUMBER_DECIMAL_PLACES, "0"));
  const units = sign ? -magnitude : magnitude;
  return units < NUMBER_MIN_UNITS || units > NUMBER_MAX_UNITS ? null : units;
}

/**
 * Return the canonical decimal string for typed number input, or null when native code would refuse it.
 * Accepts an optional leading minus, digits before the dot, and digits after it when present; rejects plus signs, exponents, commas, and inner spaces.
 * The canonical form has no leading zeros, no trailing fractional zeros, and no negative zero.
 */
export function canonicalProjectCustomFieldNumber(text: string): string | null {
  const units = projectCustomFieldNumberUnits(text.trim());
  if (units === null) return null;
  const negative = units < 0n;
  const magnitude = negative ? -units : units;
  const whole = (magnitude / NUMBER_SCALE).toString();
  const fraction = (magnitude % NUMBER_SCALE).toString()
    .padStart(PROJECT_CUSTOM_FIELD_NUMBER_DECIMAL_PLACES, "0")
    .replace(/0+$/, "");
  return `${negative ? "-" : ""}${whole}${fraction ? `.${fraction}` : ""}`;
}

/** Compare two canonical number custom field values exactly, without floating-point rounding. */
export function compareProjectCustomFieldNumbers(left: string, right: string): number {
  const leftUnits = projectCustomFieldNumberUnits(left) ?? 0n;
  const rightUnits = projectCustomFieldNumberUnits(right) ?? 0n;
  return leftUnits < rightUnits ? -1 : leftUnits > rightUnits ? 1 : 0;
}

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
  if (field.fieldType === "number") return value?.numberValue || undefined;
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
