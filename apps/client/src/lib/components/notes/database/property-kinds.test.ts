import { describe, expect, it } from "vitest";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { NOTES_DATA_SOURCE_PROPERTY_TYPES } from "$lib/notes/types";
import {
  notesPropertyHasSchemaOptions,
  notesPropertyKind,
  notesPropertyTypeLabel,
  notesPropertyTypeOptions,
  notesStoredPropertyTypeLabel,
} from "./property-kinds";

const { t } = getLocalization();

describe("Notes property kinds", () => {
  it("names a stored type in the reader's language and keeps an unknown one as stored", () => {
    expect(notesStoredPropertyTypeLabel("multi_select", t)).toBe("Multi-select");
    expect(notesStoredPropertyTypeLabel("rich_text", t)).toBe("Text");
    expect(notesStoredPropertyTypeLabel("verification", t)).toBe("verification");
  });

  it("offers every type except the unique title exactly once", () => {
    const offered = notesPropertyTypeOptions(t).map((option) => option.value);
    expect(new Set(offered).size).toBe(offered.length);
    expect([...offered].sort()).toEqual(NOTES_DATA_SOURCE_PROPERTY_TYPES.filter((type) => type !== "title").sort());
  });

  it("lists entered values first, then derived values, then automatic values", () => {
    const options = notesPropertyTypeOptions(t);
    const sections = [...new Set(options.map((option) => option.section))];
    expect(sections).toEqual(["basic", "advanced", "automatic"]);
    expect(options[0]).toMatchObject({ value: "rich_text", label: "Text", kind: "text", section: "basic" });
    expect(options.find((option) => option.value === "rollup")?.section).toBe("advanced");
    expect(options.find((option) => option.value === "last_edited_by")?.section).toBe("automatic");
  });

  it("shares icon kinds with Projects for the types both features have", () => {
    expect(notesPropertyKind("rich_text")).toBe("text");
    expect(notesPropertyKind("phone_number")).toBe("phone");
    expect(notesPropertyKind("people")).toBe("person");
    expect(notesPropertyKind("title")).toBe("title");
  });

  it("offers schema editing only for types with configuration", () => {
    const configurable = NOTES_DATA_SOURCE_PROPERTY_TYPES.filter((type) => notesPropertyHasSchemaOptions(type));
    expect(configurable).toEqual(["number", "select", "multi_select", "status", "unique_id", "relation", "rollup", "formula", "button"]);
    expect(notesPropertyHasSchemaOptions("title")).toBe(false);
    expect(notesPropertyHasSchemaOptions("date")).toBe(false);
  });

  it("labels each option with its localized type name", () => {
    for (const option of notesPropertyTypeOptions(t)) expect(option.label).toBe(notesPropertyTypeLabel(option.value, t));
    expect(notesPropertyTypeLabel("phone_number", t)).not.toBe("");
  });
});
