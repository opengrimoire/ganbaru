import {
  APP_LOCALES,
  DEFAULT_LANGUAGE_PREFERENCE,
  DEFAULT_LOCALE,
  LOCALE_METADATA,
  parseLanguagePreference,
  resolveLanguagePreference,
  type AppLocale,
  type LanguagePreference,
} from "./locales";

export const PRE_VAULT_LANGUAGE_STORAGE_KEY = "ganbaru.preVaultLanguagePreference";

export interface LanguageOption {
  readonly value: LanguagePreference;
  readonly label: string;
  readonly searchText: string;
}

export function browserLocaleCandidates(navigatorLike: Navigator | undefined): string[] {
  if (!navigatorLike) return [];
  const candidates: string[] = [];
  if (Array.isArray(navigatorLike.languages)) candidates.push(...navigatorLike.languages);
  if (typeof navigatorLike.language === "string") candidates.push(navigatorLike.language);
  return candidates;
}

export function readPreVaultLanguagePreference(
  storage: Storage | undefined,
): LanguagePreference | null {
  if (!storage) return null;
  try {
    const stored = storage.getItem(PRE_VAULT_LANGUAGE_STORAGE_KEY);
    if (stored === null) return null;
    return parseLanguagePreference(stored);
  } catch {
    return null;
  }
}

export function writePreVaultLanguagePreference(
  storage: Storage | undefined,
  preference: LanguagePreference,
): void {
  try {
    storage?.setItem(PRE_VAULT_LANGUAGE_STORAGE_KEY, preference);
  } catch {
    // Local storage is best effort on the pre-vault screen.
  }
}

export function clearPreVaultLanguagePreference(storage: Storage | undefined): void {
  try {
    storage?.removeItem(PRE_VAULT_LANGUAGE_STORAGE_KEY);
  } catch {
    // Local storage is best effort on the pre-vault screen.
  }
}

export function resolvedSystemLanguageName(
  candidates: readonly string[],
): string {
  const locale = resolveLanguagePreference(DEFAULT_LANGUAGE_PREFERENCE, candidates);
  return LOCALE_METADATA[locale]?.nativeLabel ?? LOCALE_METADATA[DEFAULT_LOCALE].nativeLabel;
}

export function selectedLanguageName(
  preference: LanguagePreference,
  resolvedLocale: AppLocale,
): string {
  if (preference === DEFAULT_LANGUAGE_PREFERENCE) {
    return LOCALE_METADATA[resolvedLocale]?.nativeLabel ?? LOCALE_METADATA[DEFAULT_LOCALE].nativeLabel;
  }
  return LOCALE_METADATA[preference]?.nativeLabel ?? LOCALE_METADATA[DEFAULT_LOCALE].nativeLabel;
}

export function languageMatchesQuery(option: LanguageOption, query: string): boolean {
  const normalized = query.trim().toLocaleLowerCase();
  if (normalized.length === 0) return true;
  return option.searchText.toLocaleLowerCase().includes(normalized);
}

export function languageOptionSearchText(
  value: LanguagePreference,
  label: string,
  searchAlias: string | undefined,
): string {
  const localeNames = value === DEFAULT_LANGUAGE_PREFERENCE
    ? Object.values(LOCALE_METADATA).flatMap((metadata) => [
      metadata.id,
      metadata.label,
      metadata.nativeLabel,
    ])
    : [
      value,
      LOCALE_METADATA[value]?.label ?? "",
      LOCALE_METADATA[value]?.nativeLabel ?? "",
    ];
  return [label, searchAlias, ...localeNames].filter(Boolean).join(" ");
}

export function supportedExplicitLocaleOptions(): readonly AppLocale[] {
  return APP_LOCALES;
}
