import {
  APP_LOCALES,
  DEFAULT_LANGUAGE_PREFERENCE,
  DEFAULT_LOCALE,
  LOCALE_METADATA,
  PLANNED_LOCALES,
  parseLanguagePreference,
  resolveLanguagePreference,
  type AppLocale,
  type LanguagePreference,
  type LocaleDirection,
} from "./locales";

export const PRE_VAULT_LANGUAGE_STORAGE_KEY = "ganbaru.preVaultLanguagePreference";

/** One choice on the welcome language screen. */
export interface PreVaultLanguageChoice {
  readonly value: LanguagePreference;
  /** The main label: a language's own name, or the localized system option. */
  readonly title: string;
  /** The language `title` is written in, or null when it is in the interface language. */
  readonly titleLang: AppLocale | null;
  /** A second line that names the title in the interface language, or the language the system option resolves to. */
  readonly detail: string;
  /** The language `detail` is written in, or null when it is in the interface language. */
  readonly detailLang: AppLocale | null;
}

/** A planned language on the welcome language screen, shown turned off. */
export interface PlannedLanguageEntry {
  readonly id: string;
  /** The language's own name, written in that language. */
  readonly title: string;
  readonly direction: LocaleDirection;
  /** The language's name in the interface language. */
  readonly detail: string;
}

/**
 * The planned languages, each by its own name with its name in `interfaceLocale` underneath. Names come from the
 * platform's language display names, sentence-capitalized; where the platform cannot name one, the English name
 * stands in.
 */
export function plannedLanguageEntries(interfaceLocale: AppLocale): PlannedLanguageEntry[] {
  let names: Intl.DisplayNames | null = null;
  try {
    names = new Intl.DisplayNames([interfaceLocale], { type: "language", fallback: "none" });
  } catch {
    names = null;
  }
  return PLANNED_LOCALES.map((locale) => {
    const localized = names?.of(locale.id);
    return {
      id: locale.id,
      title: locale.nativeLabel,
      direction: locale.direction,
      detail: localized ? capitalizeFirst(localized, interfaceLocale) : locale.label,
    };
  });
}

function capitalizeFirst(text: string, locale: AppLocale): string {
  const [first = "", ...rest] = Array.from(text);
  return first.toLocaleUpperCase(locale) + rest.join("");
}

/** The locales the browser or webview reports, most preferred first. */
export function browserLocaleCandidates(navigatorLike: Navigator | undefined): string[] {
  if (!navigatorLike) return [];
  const candidates: string[] = [];
  if (Array.isArray(navigatorLike.languages)) candidates.push(...navigatorLike.languages);
  if (typeof navigatorLike.language === "string") candidates.push(navigatorLike.language);
  return candidates;
}

/** The language chosen before a vault exists, or null when none was chosen. */
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

/** Remember the language chosen before a vault exists, until vault setup saves it as a preference. */
export function writePreVaultLanguagePreference(
  storage: Storage | undefined,
  preference: LanguagePreference,
): void {
  try {
    storage?.setItem(PRE_VAULT_LANGUAGE_STORAGE_KEY, preference);
  } catch {
    // Local storage is best effort before a vault exists.
  }
}

/** Forget the pre-vault language once the vault's preferences hold it. */
export function clearPreVaultLanguagePreference(storage: Storage | undefined): void {
  try {
    storage?.removeItem(PRE_VAULT_LANGUAGE_STORAGE_KEY);
  } catch {
    // Local storage is best effort before a vault exists.
  }
}

/** The app locale the system language resolves to, falling back to the default locale. */
export function resolvedSystemLocale(candidates: readonly string[]): AppLocale {
  return resolveLanguagePreference(DEFAULT_LANGUAGE_PREFERENCE, candidates);
}

/** The own name of the app locale the system language resolves to. */
export function resolvedSystemLanguageName(candidates: readonly string[]): string {
  const locale = resolvedSystemLocale(candidates);
  return LOCALE_METADATA[locale]?.nativeLabel ?? LOCALE_METADATA[DEFAULT_LOCALE].nativeLabel;
}

/**
 * The welcome language screen's choices: following the system first, naming the language it resolves to, then every
 * app locale by its own name with its name in the interface language underneath.
 */
export function preVaultLanguageChoices(
  systemCandidates: readonly string[],
  systemLabel: string,
  localizedName: (locale: AppLocale) => string,
): PreVaultLanguageChoice[] {
  const systemLocale = resolvedSystemLocale(systemCandidates);
  return [
    {
      value: DEFAULT_LANGUAGE_PREFERENCE,
      title: systemLabel,
      titleLang: null,
      detail: LOCALE_METADATA[systemLocale].nativeLabel,
      detailLang: systemLocale,
    },
    ...APP_LOCALES.map((locale) => ({
      value: locale,
      title: LOCALE_METADATA[locale].nativeLabel,
      titleLang: locale,
      detail: localizedName(locale),
      detailLang: null,
    })),
  ];
}
