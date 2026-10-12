export const DEFAULT_LOCALE = "en" as const;
export const DEFAULT_LANGUAGE_PREFERENCE = "system" as const;

export const APP_LOCALES = ["en", "es"] as const;
export type AppLocale = (typeof APP_LOCALES)[number];

export const LANGUAGE_PREFERENCES = [
  DEFAULT_LANGUAGE_PREFERENCE,
  ...APP_LOCALES,
] as const;
export type LanguagePreference = (typeof LANGUAGE_PREFERENCES)[number];

export type LocaleDirection = "ltr" | "rtl";

export interface LocaleMetadata {
  readonly id: AppLocale;
  readonly label: string;
  readonly nativeLabel: string;
  readonly direction: LocaleDirection;
}
export const LOCALE_METADATA: Readonly<Record<AppLocale, LocaleMetadata>> =
  Object.freeze({
    en: Object.freeze({
      id: "en",
      label: "English",
      nativeLabel: "English",
      direction: "ltr",
    }),
    es: Object.freeze({
      id: "es",
      label: "Spanish",
      nativeLabel: "Español",
      direction: "ltr",
    }),
  });

const APP_LOCALE_SET = new Set<string>(APP_LOCALES);
const LANGUAGE_PREFERENCE_SET = new Set<string>(LANGUAGE_PREFERENCES);

export function isAppLocale(value: unknown): value is AppLocale {
  return typeof value === "string" && APP_LOCALE_SET.has(value);
}

export function isLanguagePreference(value: unknown): value is LanguagePreference {
  return typeof value === "string" && LANGUAGE_PREFERENCE_SET.has(value);
}

export function parseLanguagePreference(value: unknown): LanguagePreference {
  return isLanguagePreference(value) ? value : DEFAULT_LANGUAGE_PREFERENCE;
}

function canonicalizeLocale(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed.length === 0) return null;

  try {
    return Intl.getCanonicalLocales(trimmed)[0] ?? null;
  } catch {
    return trimmed;
  }
}

function normalizeLocaleKey(locale: string): string {
  return locale.toLowerCase();
}

function baseLanguage(locale: string): string {
  return normalizeLocaleKey(locale).split("-")[0] ?? "";
}

export function resolveLocaleFromCandidates(
  candidates: readonly string[],
): AppLocale {
  for (const candidate of candidates) {
    const canonical = canonicalizeLocale(candidate);
    if (!canonical) continue;

    const normalized = normalizeLocaleKey(canonical);
    if (isAppLocale(normalized)) return normalized;

    const base = baseLanguage(canonical);
    if (isAppLocale(base)) return base;
  }

  return DEFAULT_LOCALE;
}

export function resolveLanguagePreference(
  preference: LanguagePreference,
  systemLocales: readonly string[],
): AppLocale {
  if (preference !== DEFAULT_LANGUAGE_PREFERENCE) {
    return resolveLocaleFromCandidates([preference]);
  }
  return resolveLocaleFromCandidates(systemLocales);
}

export function localeDirection(locale: AppLocale): LocaleDirection {
  return LOCALE_METADATA[locale].direction;
}
