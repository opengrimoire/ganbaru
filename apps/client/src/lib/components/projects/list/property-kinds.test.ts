import { describe, expect, it } from "vitest";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { PROJECT_CUSTOM_FIELD_TYPES } from "$lib/projects/types";
import { projectCustomFieldKind, projectCustomFieldTypeOptions, projectListColumnKind } from "./property-kinds";

const { t } = getLocalization();

describe("Projects property kinds", () => {
  it("offers every custom field type once, in the order the type list defines", () => {
    expect(projectCustomFieldTypeOptions(t).map((option) => option.value)).toEqual([...PROJECT_CUSTOM_FIELD_TYPES]);
    for (const option of projectCustomFieldTypeOptions(t)) expect(option.kind).toBe(projectCustomFieldKind(option.value));
  });

  it("shows a custom column with the icon of its field type", () => {
    expect(projectListColumnKind("custom:points", { fieldType: "number" })).toBe("number");
    expect(projectListColumnKind("custom:owner", { fieldType: "person" })).toBe("person");
  });

  it("shows core columns with the icon of their value type", () => {
    expect(projectListColumnKind("name", undefined)).toBe("title");
    expect(projectListColumnKind("due", undefined)).toBe("date");
    expect(projectListColumnKind("assignee", undefined)).toBe("person");
    expect(projectListColumnKind("dependencies", undefined)).toBe("relation");
  });

  it("falls back to text for a custom column whose field is not loaded", () => {
    expect(projectListColumnKind("custom:missing", undefined)).toBe("text");
  });
});
