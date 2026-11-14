import { describe, expect, it } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  validateThemeJson,
  type ThemeValidationIssue,
  type ThemeValidationIssueCode,
} from "$lib/themes";
import { es } from "./messages/es";
import { themeValidationMessage, themeValidationMessages } from "./theme-labels";
import { translateFromCatalog, translateFromPartialCatalog, type Translate } from "./translator.svelte";

const translateEs = ((key: Parameters<Translate>[0], ...args: unknown[]) =>
  (translateFromPartialCatalog as (catalog: unknown, key: string, ...rest: unknown[]) => string)(es, key, ...args)
) as Translate;

const SAMPLE_ISSUES: Record<ThemeValidationIssueCode, ThemeValidationIssue> = {
  not_object: { code: "not_object", path: "" },
  schema_version: { code: "schema_version", path: "schemaVersion", expected: 1 },
  required: { code: "required", path: "appTokens.--primary" },
  non_empty_string: { code: "non_empty_string", path: "displayName" },
  slug: { code: "slug", path: "id" },
  built_in_id: { code: "built_in_id", path: "id" },
  max_length: { code: "max_length", path: "displayName", limit: 60 },
  hex_color: { code: "hex_color", path: "eventPalette[3]" },
  hex_color_list: { code: "hex_color_list", path: "eventPalette", count: 12 },
  entry_count: { code: "entry_count", path: "eventPalette", expected: 12, actual: 4 },
  object: { code: "object", path: "calendarDefaults" },
  token_color_map: { code: "token_color_map", path: "calendarTokens" },
  token_name_list: { code: "token_name_list", path: "appIsolated" },
  string: { code: "string", path: "appIsolated[2]" },
  number: { code: "number", path: "derivationEngineVersion" },
  non_negative_integer: { code: "non_negative_integer", path: "derivationEngineVersion" },
  one_of: { code: "one_of", path: "iconLabel", allowed: ["light", "dark"] },
};

describe("themeValidationMessage", () => {
  it("renders every issue code in English and Spanish and names the field", () => {
    for (const issue of Object.values(SAMPLE_ISSUES)) {
      const english = themeValidationMessage(issue, translateFromCatalog, "en");
      const spanish = themeValidationMessage(issue, translateEs, "es");
      expect(english, issue.code).not.toMatch(/^settings\./);
      expect(spanish, issue.code).not.toMatch(/^settings\./);
      expect(spanish, issue.code).not.toBe(english);
      if (issue.path) {
        expect(english, issue.code).toContain(issue.path);
        expect(spanish, issue.code).toContain(issue.path);
      }
    }
  });

  it("includes limits, counts, and allowed values with locale formatting", () => {
    expect(themeValidationMessage(SAMPLE_ISSUES.max_length, translateFromCatalog, "en")).toContain("60");
    const count = themeValidationMessage(SAMPLE_ISSUES.entry_count, translateFromCatalog, "en");
    expect(count).toContain("12");
    expect(count).toContain("4");
    expect(themeValidationMessage(SAMPLE_ISSUES.one_of, translateFromCatalog, "en")).toContain('"light" or "dark"');
    expect(themeValidationMessage(SAMPLE_ISSUES.one_of, translateEs, "es")).toContain('"light" o "dark"');
  });

  it("localizes the issues a rejected theme reports without echoing its values", () => {
    const result = validateThemeJson({
      schemaVersion: 1,
      id: "<script>",
      displayName: "",
      blendCanvas: "red",
      eventPalette: new Array(PALETTE_SIZE - 1).fill("#000000"),
    });
    expect(result.ok).toBe(false);
    if (result.ok) return;
    const messages = themeValidationMessages(result.issues, translateEs, "es");
    expect(messages).toHaveLength(result.issues.length);
    expect(messages.some((message) => message.startsWith("id solo puede usar"))).toBe(true);
    expect(messages.some((message) => message.includes("blendCanvas"))).toBe(true);
    expect(messages.join("\n")).not.toContain("<script>");
  });
});
