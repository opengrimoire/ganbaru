import type {
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldType,
} from "$lib/projects/types";
import { projectCustomFieldUsesOptions } from "$lib/projects/custom-fields";

export interface NewCustomFieldOptionDraft {
  id: string;
  name: string;
}

export interface NewCustomFieldDraft {
  id: string;
  name: string;
  fieldType: ProjectCustomFieldType;
  optionRows: NewCustomFieldOptionDraft[];
  optionName: string;
}

export interface CustomFieldUpdateSaveDraft {
  field: ProjectCustomField;
  name: string;
}

export interface CustomFieldCreateSaveDraft {
  draftId: string;
  name: string;
  fieldType: ProjectCustomFieldType;
  optionNames: string[];
}

export interface CustomFieldSaveDraft {
  updates: CustomFieldUpdateSaveDraft[];
  creates: CustomFieldCreateSaveDraft[];
}

export interface CustomFieldOptionUpdateDraft {
  option: ProjectCustomFieldOption;
  name: string;
}

export interface CustomFieldOptionCreateDraft {
  field: ProjectCustomField;
  name: string;
  draftId: string;
}

export interface CustomFieldOptionSaveDraft {
  updates: CustomFieldOptionUpdateDraft[];
  creates: CustomFieldOptionCreateDraft[];
}

export type CustomFieldDraftError =
  | "name_required"
  | "name_exists"
  | "option_name_required"
  | "option_name_exists";

export type CustomFieldDraftResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: CustomFieldDraftError };

export interface CustomFieldSaveInput {
  fields: readonly ProjectCustomField[];
  fieldNameDrafts: Readonly<Record<string, string>>;
  createDraftRows: readonly NewCustomFieldDraft[];
}

export interface CustomFieldOptionSaveInput {
  fields: readonly ProjectCustomField[];
  fieldOptions: (fieldId: string) => readonly ProjectCustomFieldOption[];
  optionNameDrafts: Readonly<Record<string, string>>;
  optionCreateDraftRowsByField: Readonly<Record<string, readonly NewCustomFieldOptionDraft[]>>;
}

function normalizedDraftName(value: string): string {
  return value.trim().toLowerCase();
}

export function draftCustomFieldName(
  field: ProjectCustomField,
  fieldNameDrafts: Readonly<Record<string, string>>,
): string {
  return fieldNameDrafts[field.id] ?? field.name;
}

export function customFieldDraftDirty(
  field: ProjectCustomField,
  fieldNameDrafts: Readonly<Record<string, string>>,
): boolean {
  return draftCustomFieldName(field, fieldNameDrafts) !== field.name;
}

export function draftCustomFieldOptionName(
  option: ProjectCustomFieldOption,
  optionNameDrafts: Readonly<Record<string, string>>,
): string {
  return optionNameDrafts[option.id] ?? option.name;
}

export function customFieldOptionDraftDirty(
  option: ProjectCustomFieldOption,
  optionNameDrafts: Readonly<Record<string, string>>,
): boolean {
  return draftCustomFieldOptionName(option, optionNameDrafts) !== option.name;
}

export function newCustomFieldOptionDraftRows(
  rowsByField: Readonly<Record<string, readonly NewCustomFieldOptionDraft[]>>,
  fieldId: string,
): NewCustomFieldOptionDraft[] {
  return [...(rowsByField[fieldId] ?? [])];
}

export function setNewCustomFieldOptionDraftName(
  rowsByField: Readonly<Record<string, readonly NewCustomFieldOptionDraft[]>>,
  fieldId: string,
  optionId: string,
  name: string,
): Record<string, NewCustomFieldOptionDraft[]> {
  const nextRowsByField: Record<string, NewCustomFieldOptionDraft[]> = Object.fromEntries(
    Object.entries(rowsByField).map(([currentFieldId, rows]) => [currentFieldId, [...rows]]),
  );
  nextRowsByField[fieldId] = newCustomFieldOptionDraftRows(rowsByField, fieldId)
    .map((option) => option.id === optionId ? { ...option, name } : option);
  return nextRowsByField;
}

export function removeNewCustomFieldOptionDraft(
  rowsByField: Readonly<Record<string, readonly NewCustomFieldOptionDraft[]>>,
  fieldId: string,
  optionId: string,
): Record<string, NewCustomFieldOptionDraft[]> {
  const remainingRows = newCustomFieldOptionDraftRows(rowsByField, fieldId)
    .filter((option) => option.id !== optionId);
  const nextRowsByField: Record<string, NewCustomFieldOptionDraft[]> = Object.fromEntries(
    Object.entries(rowsByField).map(([currentFieldId, rows]) => [currentFieldId, [...rows]]),
  );
  if (remainingRows.length > 0) {
    nextRowsByField[fieldId] = remainingRows;
  } else {
    delete nextRowsByField[fieldId];
  }
  return nextRowsByField;
}

export function setNewCustomFieldDraftName(
  rows: readonly NewCustomFieldDraft[],
  fieldId: string,
  name: string,
): NewCustomFieldDraft[] {
  return rows.map((field) => field.id === fieldId ? { ...field, name } : field);
}

export function setNewCustomFieldDraftOptionName(
  rows: readonly NewCustomFieldDraft[],
  fieldId: string,
  optionId: string,
  name: string,
): NewCustomFieldDraft[] {
  return rows.map((field) =>
    field.id === fieldId
      ? {
          ...field,
          optionRows: field.optionRows.map((option) =>
            option.id === optionId ? { ...option, name } : option
          ),
        }
      : field
  );
}

export function setNewCustomFieldDraftPendingOptionName(
  rows: readonly NewCustomFieldDraft[],
  fieldId: string,
  optionName: string,
): NewCustomFieldDraft[] {
  return rows.map((field) => field.id === fieldId ? { ...field, optionName } : field);
}

export function removeNewCustomFieldDraft(
  rows: readonly NewCustomFieldDraft[],
  fieldId: string,
): NewCustomFieldDraft[] {
  return rows.filter((field) => field.id !== fieldId);
}

export function removeNewCustomFieldDraftOption(
  rows: readonly NewCustomFieldDraft[],
  fieldId: string,
  optionId: string,
): NewCustomFieldDraft[] {
  return rows.map((field) =>
    field.id === fieldId
      ? { ...field, optionRows: field.optionRows.filter((option) => option.id !== optionId) }
      : field
  );
}

export function newCustomFieldOptionNames(
  fieldType: ProjectCustomFieldType,
  optionRows: readonly NewCustomFieldOptionDraft[],
  pendingOptionName: string,
): CustomFieldDraftResult<string[]> {
  if (!projectCustomFieldUsesOptions(fieldType)) return { ok: true, value: [] };
  const names: string[] = [];
  const seenNames = new Set<string>();
  for (const option of optionRows) {
    const name = option.name.trim();
    if (!name) return { ok: false, error: "option_name_required" };
    const normalizedName = normalizedDraftName(name);
    if (seenNames.has(normalizedName)) return { ok: false, error: "option_name_exists" };
    seenNames.add(normalizedName);
    names.push(name);
  }
  const pendingName = pendingOptionName.trim();
  if (pendingName) {
    const normalizedName = normalizedDraftName(pendingName);
    if (seenNames.has(normalizedName)) return { ok: false, error: "option_name_exists" };
    names.push(pendingName);
  }
  return { ok: true, value: names };
}

export function customFieldSaveDrafts(
  input: CustomFieldSaveInput,
): CustomFieldDraftResult<CustomFieldSaveDraft> {
  const updates: CustomFieldUpdateSaveDraft[] = [];
  const creates: CustomFieldCreateSaveDraft[] = [];
  const seenNames = new Set<string>();
  for (const field of input.fields) {
    const name = draftCustomFieldName(field, input.fieldNameDrafts).trim();
    if (!name) return { ok: false, error: "name_required" };
    const normalizedName = normalizedDraftName(name);
    if (seenNames.has(normalizedName)) return { ok: false, error: "name_exists" };
    seenNames.add(normalizedName);
    if (!customFieldDraftDirty(field, input.fieldNameDrafts)) continue;
    updates.push({ field, name });
  }
  for (const field of input.createDraftRows) {
    const name = field.name.trim();
    if (!name) return { ok: false, error: "name_required" };
    const normalizedName = normalizedDraftName(name);
    if (seenNames.has(normalizedName)) return { ok: false, error: "name_exists" };
    seenNames.add(normalizedName);
    const optionNames = newCustomFieldOptionNames(
      field.fieldType,
      field.optionRows,
      "",
    );
    if (!optionNames.ok) return optionNames;
    creates.push({
      draftId: field.id,
      name,
      fieldType: field.fieldType,
      optionNames: optionNames.value,
    });
  }
  return { ok: true, value: { updates, creates } };
}

export function customFieldOptionSaveDrafts(
  input: CustomFieldOptionSaveInput,
): CustomFieldDraftResult<CustomFieldOptionSaveDraft> {
  const updates: CustomFieldOptionUpdateDraft[] = [];
  const creates: CustomFieldOptionCreateDraft[] = [];
  for (const field of input.fields) {
    if (!projectCustomFieldUsesOptions(field.fieldType)) continue;
    const seenNames = new Set<string>();
    for (const option of input.fieldOptions(field.id)) {
      const name = draftCustomFieldOptionName(option, input.optionNameDrafts).trim();
      if (!name) return { ok: false, error: "option_name_required" };
      const normalizedName = normalizedDraftName(name);
      if (seenNames.has(normalizedName)) return { ok: false, error: "option_name_exists" };
      seenNames.add(normalizedName);
      if (!customFieldOptionDraftDirty(option, input.optionNameDrafts)) continue;
      updates.push({ option, name });
    }
    for (const option of input.optionCreateDraftRowsByField[field.id] ?? []) {
      const name = option.name.trim();
      if (!name) return { ok: false, error: "option_name_required" };
      const normalizedName = normalizedDraftName(name);
      if (seenNames.has(normalizedName)) return { ok: false, error: "option_name_exists" };
      seenNames.add(normalizedName);
      creates.push({ field, name, draftId: option.id });
    }
  }
  return { ok: true, value: { updates, creates } };
}
