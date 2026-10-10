import { describe, expect, it } from "vitest";
import {
  newCustomFieldOptionDraftRows,
  newCustomFieldOptionNames,
  customFieldOptionSaveDrafts,
  customFieldSaveDrafts,
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
import type {
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldType,
} from "$lib/projects/types";

function field(overrides: Partial<ProjectCustomField> = {}): ProjectCustomField {
  return {
    id: "field-1",
    projectId: "project-1",
    name: "Phase",
    fieldType: "select",
    sortOrder: 1000,
    icon: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function option(overrides: Partial<ProjectCustomFieldOption> = {}): ProjectCustomFieldOption {
  return {
    id: "option-1",
    fieldId: "field-1",
    name: "Draft",
    sortOrder: 1000,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function createField(
  overrides: Partial<NewCustomFieldDraft> = {},
): NewCustomFieldDraft {
  return {
    id: "draft-field-1",
    name: "Area",
    fieldType: "select",
    optionRows: [createOption({ id: "draft-option-1", name: "Home" })],
    optionName: "",
    ...overrides,
  };
}

function createOption(
  overrides: Partial<NewCustomFieldOptionDraft> = {},
): NewCustomFieldOptionDraft {
  return {
    id: "draft-option-1",
    name: "Home",
    ...overrides,
  };
}

describe("customFieldSaveDrafts", () => {
  it("returns custom field updates and creates with trimmed option names", () => {
    const existingField = field();
    const createDraft = createField({
      optionRows: [createOption({ name: " Home " })],
    });

    const result = customFieldSaveDrafts({
      fields: [existingField],
      fieldNameDrafts: { "field-1": "  Stage  " },
      createDraftRows: [createDraft],
    });

    expect(result).toEqual({
      ok: true,
      value: {
        updates: [{ field: existingField, name: "Stage" }],
        creates: [{
          draftId: "draft-field-1",
          name: "Area",
          fieldType: "select",
          optionNames: ["Home"],
        }],
      },
    });
  });

  it("rejects blank and duplicate custom field names", () => {
    expect(customFieldSaveDrafts({
      fields: [field()],
      fieldNameDrafts: { "field-1": " " },
      createDraftRows: [],
    })).toEqual({ ok: false, error: "name_required" });
    expect(customFieldSaveDrafts({
      fields: [field(), field({ id: "field-2", name: "Area" })],
      fieldNameDrafts: { "field-2": " phase " },
      createDraftRows: [],
    })).toEqual({ ok: false, error: "name_exists" });
  });
});

describe("customFieldOptionSaveDrafts", () => {
  it("returns option updates and creates for fields that accept options", () => {
    const existingField = field();
    const existingOption = option();
    const result = customFieldOptionSaveDrafts({
      fields: [
        existingField,
        field({ id: "field-2", name: "Notes", fieldType: "text" }),
      ],
      fieldOptions: (fieldId) => fieldId === "field-1" ? [existingOption] : [],
      optionNameDrafts: { "option-1": "  Ready  " },
      optionCreateDraftRowsByField: {
        "field-1": [createOption({ id: "draft-option-2", name: " Blocked " })],
        "field-2": [createOption({ id: "draft-option-3", name: "Skipped" })],
      },
    });

    expect(result).toEqual({
      ok: true,
      value: {
        updates: [{ option: existingOption, name: "Ready" }],
        creates: [{ field: existingField, name: "Blocked", draftId: "draft-option-2" }],
      },
    });
  });

  it("rejects blank and duplicate option names within a field", () => {
    expect(customFieldOptionSaveDrafts({
      fields: [field()],
      fieldOptions: () => [option()],
      optionNameDrafts: { "option-1": " " },
      optionCreateDraftRowsByField: {},
    })).toEqual({ ok: false, error: "option_name_required" });
    expect(customFieldOptionSaveDrafts({
      fields: [field()],
      fieldOptions: () => [option()],
      optionNameDrafts: {},
      optionCreateDraftRowsByField: {
        "field-1": [createOption({ name: " draft " })],
      },
    })).toEqual({ ok: false, error: "option_name_exists" });
  });
});

describe("newCustomFieldOptionNames", () => {
  it("returns no options for field types that do not accept options", () => {
    const fieldType: ProjectCustomFieldType = "text";

    expect(newCustomFieldOptionNames(
      fieldType,
      [createOption({ name: "Ignored" })],
      "Also ignored",
    )).toEqual({ ok: true, value: [] });
  });

  it("includes pending option names and rejects duplicates", () => {
    expect(newCustomFieldOptionNames(
      "select",
      [createOption({ name: "One" })],
      "Two",
    )).toEqual({ ok: true, value: ["One", "Two"] });
    expect(newCustomFieldOptionNames(
      "select",
      [createOption({ name: "One" })],
      " one ",
    )).toEqual({ ok: false, error: "option_name_exists" });
  });
});

describe("custom field draft row transformations", () => {
  it("updates and removes option create draft rows by field", () => {
    const rowsByField = {
      "field-1": [
        createOption({ id: "draft-option-1", name: "One" }),
        createOption({ id: "draft-option-2", name: "Two" }),
      ],
    };

    expect(newCustomFieldOptionDraftRows(rowsByField, "field-1"))
      .toEqual(rowsByField["field-1"]);
    expect(setNewCustomFieldOptionDraftName(
      rowsByField,
      "field-1",
      "draft-option-2",
      "Renamed",
    )["field-1"]).toEqual([
      createOption({ id: "draft-option-1", name: "One" }),
      createOption({ id: "draft-option-2", name: "Renamed" }),
    ]);
    expect(removeNewCustomFieldOptionDraft(
      { "field-1": [createOption({ id: "draft-option-1" })] },
      "field-1",
      "draft-option-1",
    )).toEqual({});
  });

  it("updates and removes staged custom field create drafts", () => {
    const rows = [
      createField({
        id: "field-draft-1",
        name: "One",
        optionRows: [createOption({ id: "option-draft-1", name: "Alpha" })],
      }),
      createField({ id: "field-draft-2", name: "Two" }),
    ];

    expect(setNewCustomFieldDraftName(rows, "field-draft-1", "Renamed")[0])
      .toMatchObject({ id: "field-draft-1", name: "Renamed" });
    expect(setNewCustomFieldDraftOptionName(
      rows,
      "field-draft-1",
      "option-draft-1",
      "Beta",
    )[0]?.optionRows[0]).toMatchObject({ id: "option-draft-1", name: "Beta" });
    expect(setNewCustomFieldDraftPendingOptionName(
      rows,
      "field-draft-1",
      "Pending",
    )[0]).toMatchObject({ id: "field-draft-1", optionName: "Pending" });
    expect(removeNewCustomFieldDraft(rows, "field-draft-2"))
      .toHaveLength(1);
    expect(removeNewCustomFieldDraftOption(
      rows,
      "field-draft-1",
      "option-draft-1",
    )[0]?.optionRows).toEqual([]);
  });
});
