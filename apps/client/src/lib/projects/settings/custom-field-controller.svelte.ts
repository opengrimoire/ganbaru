import type { getLocalization } from "$lib/i18n/translator.svelte";
import { projectCustomFieldUsesOptions } from "$lib/projects/custom-fields";
import {
  draftCustomFieldName,
  newCustomFieldOptionDraftRows,
  draftCustomFieldOptionName,
  newCustomFieldOptionNames,
  removeNewCustomFieldDraft,
  removeNewCustomFieldDraftOption,
  removeNewCustomFieldOptionDraft,
  setNewCustomFieldDraftName,
  setNewCustomFieldDraftOptionName,
  setNewCustomFieldDraftPendingOptionName,
  setNewCustomFieldOptionDraftName,
  type NewCustomFieldDraft,
  type NewCustomFieldOptionDraft,
} from "./custom-field-drafts";
import type { ProjectSettingsSessionState } from "./session.svelte";
import type { ProjectCustomField, ProjectCustomFieldOption } from "$lib/projects/types";

export interface ProjectSettingsCustomFieldControllerOptions {
  state: ProjectSettingsSessionState;
  fields: () => readonly ProjectCustomField[];
  collections: {
    optionsForField: (fieldId: string) => ProjectCustomFieldOption[];
    removeCustomField: (fieldId: string) => void;
    removeCustomFieldOption: (optionId: string) => void;
  };
  translate: ReturnType<typeof getLocalization>["t"];
  setDraftError: (error: "name_required" | "name_exists" | "option_name_required" | "option_name_exists") => void;
  afterCreateDraft: () => void;
  createId?: () => string;
}

/** Owns custom-field and nested option draft workflows within one settings session. */
export function createProjectSettingsCustomFieldController(
  options: ProjectSettingsCustomFieldControllerOptions,
) {
  const { state, collections } = options;
  const createId = options.createId ?? (() => crypto.randomUUID());
  let pendingDeleteFieldId = $state<string | null>(null);
  let pendingDeleteOptionId = $state<string | null>(null);

  const fields = () => options.fields();
  const fieldOptions = (field: ProjectCustomField): ProjectCustomFieldOption[] =>
    collections.optionsForField(field.id);
  const fieldName = (field: ProjectCustomField): string =>
    draftCustomFieldName(field, state.customFieldNameDrafts);
  const optionName = (option: ProjectCustomFieldOption): string =>
    draftCustomFieldOptionName(option, state.customFieldOptionNameDrafts);
  const normalized = (value: string): string => value.trim().toLowerCase();

  function fieldNameExists(name: string, ignoredId?: string): boolean {
    const value = normalized(name);
    return value.length > 0 && fields().some((field) => field.id !== ignoredId && normalized(fieldName(field)) === value);
  }

  function createFieldNameExists(name: string, ignoredId?: string): boolean {
    const value = normalized(name);
    return value.length > 0 && (fieldNameExists(name) || state.customFieldCreateDraftRows.some(
      (field) => field.id !== ignoredId && normalized(field.name) === value,
    ));
  }

  function optionCreateRows(fieldId: string): NewCustomFieldOptionDraft[] {
    return newCustomFieldOptionDraftRows(state.customFieldOptionDraftRowsByField, fieldId);
  }

  function optionNameExists(fieldId: string, name: string, ignoredId?: string): boolean {
    const value = normalized(name);
    return value.length > 0 && collections.optionsForField(fieldId).some(
      (option) => option.id !== ignoredId && normalized(optionName(option)) === value,
    );
  }

  function optionCreateNameExists(fieldId: string, name: string, ignoredId?: string): boolean {
    const value = normalized(name);
    return value.length > 0 && (optionNameExists(fieldId, name) || optionCreateRows(fieldId).some(
      (option) => option.id !== ignoredId && normalized(option.name) === value,
    ));
  }

  function stagedOptionNameExists(rows: readonly NewCustomFieldOptionDraft[], name: string, ignoredId?: string): boolean {
    const value = normalized(name);
    return value.length > 0 && rows.some((option) => option.id !== ignoredId && normalized(option.name) === value);
  }

  function setOptionCreateName(fieldId: string, optionId: string, name: string): void {
    state.customFieldOptionDraftRowsByField = setNewCustomFieldOptionDraftName(
      state.customFieldOptionDraftRowsByField, fieldId, optionId, name,
    );
  }

  function removeOptionCreate(fieldId: string, optionId: string): void {
    state.customFieldOptionDraftRowsByField = removeNewCustomFieldOptionDraft(
      state.customFieldOptionDraftRowsByField, fieldId, optionId,
    );
  }

  function setCreateFieldName(fieldId: string, name: string): void {
    state.customFieldCreateDraftRows = setNewCustomFieldDraftName(state.customFieldCreateDraftRows, fieldId, name);
  }

  function setCreateOptionName(fieldId: string, optionId: string, name: string): void {
    state.customFieldCreateDraftRows = setNewCustomFieldDraftOptionName(state.customFieldCreateDraftRows, fieldId, optionId, name);
  }

  function setCreatePendingOptionName(fieldId: string, name: string): void {
    state.customFieldCreateDraftRows = setNewCustomFieldDraftPendingOptionName(state.customFieldCreateDraftRows, fieldId, name);
  }

  function removeCreateField(fieldId: string): void {
    state.customFieldCreateDraftRows = removeNewCustomFieldDraft(state.customFieldCreateDraftRows, fieldId);
  }

  function removeCreateOption(fieldId: string, optionId: string): void {
    state.customFieldCreateDraftRows = removeNewCustomFieldDraftOption(state.customFieldCreateDraftRows, fieldId, optionId);
  }

  function addOption(rows: NewCustomFieldOptionDraft[], pendingName: string, onAdd: (row: NewCustomFieldOptionDraft) => void): boolean {
    const name = pendingName.trim();
    if (!name) {
      state.projectSettingsError = options.translate("projects.customFields.optionNameRequired");
      return false;
    }
    if (stagedOptionNameExists(rows, name)) {
      state.projectSettingsError = options.translate("projects.customFields.optionNameExists");
      return false;
    }
    onAdd({ id: createId(), name });
    state.projectSettingsError = null;
    return true;
  }

  function submitNewOption(): void {
    if (!projectCustomFieldUsesOptions(state.newCustomFieldType)) return;
    if (addOption(state.newCustomFieldOptionRows, state.newCustomFieldOptionName, (row) => {
      state.newCustomFieldOptionRows = [...state.newCustomFieldOptionRows, row];
    })) state.newCustomFieldOptionName = "";
  }

  function submitCreateDraftOption(field: NewCustomFieldDraft): void {
    if (!projectCustomFieldUsesOptions(field.fieldType)) return;
    if (addOption(field.optionRows, field.optionName, (row) => {
      state.customFieldCreateDraftRows = state.customFieldCreateDraftRows.map((entry) =>
        entry.id === field.id ? { ...entry, optionRows: [...entry.optionRows, row], optionName: "" } : entry,
      );
    })) return;
  }

  function submitField(): void {
    const name = state.newCustomFieldName.trim();
    if (!name) {
      state.projectSettingsError = options.translate("projects.customFields.nameRequired");
      return;
    }
    if (createFieldNameExists(name)) {
      state.projectSettingsError = options.translate("projects.customFields.nameExists");
      return;
    }
    const optionNames = newCustomFieldOptionNames(
      state.newCustomFieldType, state.newCustomFieldOptionRows, state.newCustomFieldOptionName,
    );
    if (!optionNames.ok) {
      options.setDraftError(optionNames.error);
      return;
    }
    state.customFieldCreateDraftRows = [...state.customFieldCreateDraftRows, {
      id: createId(), name, fieldType: state.newCustomFieldType,
      optionRows: optionNames.value.map((value) => ({ id: createId(), name: value })), optionName: "",
    }];
    state.newCustomFieldName = "";
    state.newCustomFieldType = "text";
    state.newCustomFieldOptionRows = [];
    state.newCustomFieldOptionName = "";
    state.projectSettingsError = null;
    options.afterCreateDraft();
  }

  function submitOption(field: ProjectCustomField): void {
    const name = (state.newCustomFieldOptionDrafts[field.id] ?? "").trim();
    if (!name) {
      state.projectSettingsError = options.translate("projects.customFields.optionNameRequired");
      return;
    }
    if (optionCreateNameExists(field.id, name)) {
      state.projectSettingsError = options.translate("projects.customFields.optionNameExists");
      return;
    }
    state.customFieldOptionDraftRowsByField = { ...state.customFieldOptionDraftRowsByField,
      [field.id]: [...optionCreateRows(field.id), { id: createId(), name }] };
    state.newCustomFieldOptionDrafts = { ...state.newCustomFieldOptionDrafts, [field.id]: "" };
    state.projectSettingsError = null;
  }

  function cleanupField(fieldId: string): void {
    const names = { ...state.customFieldNameDrafts }; delete names[fieldId]; state.customFieldNameDrafts = names;
    const inputs = { ...state.newCustomFieldOptionDrafts }; delete inputs[fieldId]; state.newCustomFieldOptionDrafts = inputs;
    const rows = { ...state.customFieldOptionDraftRowsByField }; delete rows[fieldId]; state.customFieldOptionDraftRowsByField = rows;
  }

  function cleanupOption(optionId: string): void {
    const names = { ...state.customFieldOptionNameDrafts }; delete names[optionId]; state.customFieldOptionNameDrafts = names;
  }

  async function removeField(): Promise<void> {
    const field = pendingDeleteField();
    if (!field) return;
    pendingDeleteFieldId = null;
    state.projectSettingsError = null;
    try { collections.removeCustomField(field.id); cleanupField(field.id); }
    catch (error) { state.projectSettingsError = options.translate("projects.customFields.deleteFailed", error instanceof Error ? error.message : String(error)); }
  }

  async function removeOption(): Promise<void> {
    const option = pendingDeleteOption();
    if (!option) return;
    pendingDeleteOptionId = null;
    state.projectSettingsError = null;
    try { collections.removeCustomFieldOption(option.id); cleanupOption(option.id); }
    catch (error) { state.projectSettingsError = options.translate("projects.customFields.optionDeleteFailed", error instanceof Error ? error.message : String(error)); }
  }

  function pendingDeleteField(): ProjectCustomField | undefined {
    return fields().find((field) => field.id === pendingDeleteFieldId);
  }
  function pendingDeleteOption(): ProjectCustomFieldOption | undefined {
    return fields().flatMap(fieldOptions).find((option) => option.id === pendingDeleteOptionId);
  }
  function resetTransientState(): void { pendingDeleteFieldId = null; pendingDeleteOptionId = null; }

  return {
    fieldOptions, fieldName, optionName, optionCreateRows,
    fieldNameExists, createFieldNameExists, optionNameExists, optionCreateNameExists, stagedOptionNameExists,
    setOptionCreateName, removeOptionCreate, setCreateFieldName, setCreateOptionName,
    setCreatePendingOptionName, removeCreateField, removeCreateOption,
    setNewOptionName: (id: string, name: string) => { state.newCustomFieldOptionRows = state.newCustomFieldOptionRows.map((row) => row.id === id ? { ...row, name } : row); },
    removeNewOption: (id: string) => { state.newCustomFieldOptionRows = state.newCustomFieldOptionRows.filter((row) => row.id !== id); },
    submitNewOption, submitCreateDraftOption, submitField, submitOption,
    requestDeleteField: (field: ProjectCustomField) => { pendingDeleteFieldId = field.id; },
    cancelDeleteField: () => { pendingDeleteFieldId = null; }, removeField,
    requestDeleteOption: (option: ProjectCustomFieldOption) => { pendingDeleteOptionId = option.id; },
    cancelDeleteOption: () => { pendingDeleteOptionId = null; }, removeOption,
    pendingDeleteField, pendingDeleteOption, resetTransientState,
  };
}
