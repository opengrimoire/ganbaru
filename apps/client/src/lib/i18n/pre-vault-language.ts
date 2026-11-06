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
  /** The main label: a language's English name, or the localized system option. */
  readonly title: string;
  /** The language `title` is written in, or null when it is in the interface language. */
  readonly titleLang: AppLocale | null;
  /** The second line: the language's own name, or the own name of the language the system option resolves to. */
  readonly detail: string;
  /** The language `detail` is written in. */
  readonly detailLang: AppLocale;
}

/** A planned language on the welcome language screen, shown turned off. */
export interface PlannedLanguageEntry {
  /** The BCP 47 tag the translation will use, which is also the language of `detail`. */
  readonly id: string;
  /** The English name. */
  readonly title: string;
  /** The language's own name, written in that language. */
  readonly detail: string;
  /** A note, written in that language, that it is coming in a future version. */
  readonly notice: string;
  /** The language's text direction, which `notice` keeps while staying left-aligned. */
  readonly direction: LocaleDirection;
}

const ENGLISH_NAME_ORDER = new Intl.Collator(DEFAULT_LOCALE);

/**
 * The planned languages, each by its English name with its own name underneath, in the alphabetical order of the
 * English names. English names lead so the order reads the same whatever the interface language.
 */
export function plannedLanguageEntries(): PlannedLanguageEntry[] {
  return PLANNED_LOCALES.map((locale) => ({
    id: locale.id,
    title: locale.label,
    detail: locale.nativeLabel,
    notice: locale.comingNotice,
    direction: locale.direction,
  })).sort((a, b) => ENGLISH_NAME_ORDER.compare(a.title, b.title));
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
 * app locale by its English name with its own name underneath, in the alphabetical order of the English names.
 */
export function preVaultLanguageChoices(
  systemCandidates: readonly string[],
  systemLabel: string,
): PreVaultLanguageChoice[] {
  const systemLocale = resolvedSystemLocale(systemCandidates);
  const locales = [...APP_LOCALES].sort((a, b) =>
    ENGLISH_NAME_ORDER.compare(LOCALE_METADATA[a].label, LOCALE_METADATA[b].label),
  );
  return [
    {
      value: DEFAULT_LANGUAGE_PREFERENCE,
      title: systemLabel,
      titleLang: null,
      detail: LOCALE_METADATA[systemLocale].nativeLabel,
      detailLang: systemLocale,
    },
    ...locales.map((locale) => ({
      value: locale,
      title: LOCALE_METADATA[locale].label,
      titleLang: DEFAULT_LOCALE,
      detail: LOCALE_METADATA[locale].nativeLabel,
      detailLang: locale,
    })),
  ];
}
