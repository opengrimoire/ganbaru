import type { Theme, ThemeValidationIssue } from "$lib/themes";
import { formatList, formatNumber } from "./formatters";
import type { AppLocale } from "./locales";
import type { Translate } from "./translator.svelte";

export function themeDisplayName(theme: Theme, t: Translate): string {
  if (theme.kind !== "builtin") return theme.displayName;
  if (theme.id === "light") return t("theme.builtInName.light");
  if (theme.id === "dark") return t("theme.builtInName.dark");
  return theme.displayName;
}

/**
 * The catalog message for one theme validation issue, naming the field it concerns.
 *
 * @param issue A problem {@link validateThemeJson} reported.
 * @param t Translator for the active locale.
 * @param locale Locale whose number and list formats apply.
 */
export function themeValidationMessage(
  issue: ThemeValidationIssue,
  t: Translate,
  locale: AppLocale,
): string {
  const field = issue.path;
  const number = (value: number) => formatNumber(locale, value);
  switch (issue.code) {
    case "not_object":
      return t("settings.theme.validation.notObject");
    case "schema_version":
      return t("settings.theme.validation.schemaVersion", field, number(issue.expected));
    case "required":
      return t("settings.theme.validation.required", field);
    case "non_empty_string":
      return t("settings.theme.validation.nonEmptyString", field);
    case "slug":
      return t("settings.theme.validation.slug", field);
    case "built_in_id":
      return t("settings.theme.validation.builtInId", field);
    case "max_length":
      return t("settings.theme.validation.maxLength", field, number(issue.limit));
    case "hex_color":
      return t("settings.theme.validation.hexColor", field);
    case "hex_color_list":
      return t("settings.theme.validation.hexColorList", field, number(issue.count));
    case "entry_count":
      return t(
        "settings.theme.validation.entryCount",
        field,
        number(issue.expected),
        number(issue.actual),
      );
    case "object":
      return t("settings.theme.validation.object", field);
    case "token_color_map":
      return t("settings.theme.validation.tokenColorMap", field);
    case "token_name_list":
      return t("settings.theme.validation.tokenNameList", field);
    case "string":
      return t("settings.theme.validation.string", field);
    case "number":
      return t("settings.theme.validation.number", field);
    case "non_negative_integer":
      return t("settings.theme.validation.nonNegativeInteger", field);
    case "one_of":
      return t(
        "settings.theme.validation.oneOf",
        field,
        formatList(locale, issue.allowed.map((value) => `"${value}"`), { type: "disjunction" }),
      );
  }
}

/**
 * The catalog messages for every issue of a failed theme validation, in order.
 *
 * @param issues Problems {@link validateThemeJson} reported.
 * @param t Translator for the active locale.
 * @param locale Locale whose number and list formats apply.
 */
export function themeValidationMessages(
  issues: readonly ThemeValidationIssue[],
  t: Translate,
  locale: AppLocale,
): string[] {
  return issues.map((issue) => themeValidationMessage(issue, t, locale));
}
