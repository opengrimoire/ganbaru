import { getConfigKey, setConfigKey } from "$lib/vault/config";
import {
  DEFAULT_LANGUAGE_PREFERENCE,
  DEFAULT_LOCALE,
  LOCALE_METADATA,
  type AppLocale,
  type LanguagePreference,
  type LocaleDirection,
  parseLanguagePreference,
  resolveLanguagePreference,
} from "./locales";
import { en, type MessageCatalog } from "./messages/en";
import {
  loadLocaleCatalog,
  type RuntimeMessageCatalog,
} from "./catalog-loader";

const LANGUAGE_CONFIG_KEY = "preferences.language";

type CatalogBranch = Readonly<Record<string, unknown>>;

type MessageKey<T> = {
  [Key in keyof T & string]: T[Key] extends string
    ? Key
    : T[Key] extends (...args: infer _Args) => string
      ? Key
      : T[Key] extends CatalogBranch
        ? `${Key}.${MessageKey<T[Key]>}`
        : never;
}[keyof T & string];

type MessageAtPath<T, Path extends string> =
  Path extends `${infer Head}.${infer Tail}`
    ? Head extends keyof T
      ? MessageAtPath<T[Head], Tail>
      : never
    : Path extends keyof T
      ? T[Path]
      : never;

type MessageArgs<Value> = Value extends (...args: infer Args) => string ? Args : [];

export type TranslationKey = MessageKey<MessageCatalog>;

export type Translate = <Key extends TranslationKey>(
  key: Key,
  ...args: MessageArgs<MessageAtPath<MessageCatalog, Key>>
) => string;

interface LanguagePreferenceOptions {
  readonly persist?: boolean;
}

let languagePreference = $state<LanguagePreference>(DEFAULT_LANGUAGE_PREFERENCE);
let resolvedLocale = $state<AppLocale>(DEFAULT_LOCALE);
let resolvedDirection = $state<LocaleDirection>(LOCALE_METADATA[DEFAULT_LOCALE].direction);
let activeCatalog = $state.raw<RuntimeMessageCatalog>(en);
let languageRequestId = 0;

function browserLocaleCandidates(): string[] {
  if (typeof navigator === "undefined") return [];
  const candidates: string[] = [];
  if (Array.isArray(navigator.languages)) candidates.push(...navigator.languages);
  if (typeof navigator.language === "string") candidates.push(navigator.language);
  return candidates;
}

function readCatalogPath(catalog: unknown, path: string): unknown {
  let current: unknown = catalog;
  for (const part of path.split(".")) {
    if (typeof current !== "object" || current === null || Array.isArray(current)) {
      return undefined;
    }
    if (!Object.hasOwn(current, part)) return undefined;
    current = (current as CatalogBranch)[part];
  }
  return current;
}

function applyDocumentLocale(locale: AppLocale, direction: LocaleDirection): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  root.lang = locale;
  root.dir = direction;
}

/**
 * Applies the saved language preference. An unsupported saved value reads as
 * its normalized preference without being written back, so linked devices
 * never race to correct the same portable value.
 */
export async function initializeLocalizationFromConfig(): Promise<boolean> {
  const parsed = parseLanguagePreference(getConfigKey<unknown>(LANGUAGE_CONFIG_KEY, undefined));
  return await setLanguagePreference(parsed, { persist: false });
}

/**
 * Loads and atomically applies a language preference. A failed or stale
 * catalog request leaves the active preference and catalog unchanged.
 */
export async function setLanguagePreference(
  preference: LanguagePreference,
  options: LanguagePreferenceOptions = {},
): Promise<boolean> {
  const nextLocale = resolveLanguagePreference(preference, browserLocaleCandidates());
  const requestId = ++languageRequestId;
  let nextCatalog: RuntimeMessageCatalog;
  try {
    nextCatalog = await loadLocaleCatalog(nextLocale);
  } catch (error) {
    if (requestId === languageRequestId) {
      console.error(`Failed to load ${nextLocale} locale catalog:`, error);
    }
    return false;
  }
  if (requestId !== languageRequestId) return false;

  const nextDirection = LOCALE_METADATA[nextLocale].direction;
  activeCatalog = nextCatalog;
  languagePreference = preference;
  resolvedLocale = nextLocale;
  resolvedDirection = nextDirection;
  applyDocumentLocale(nextLocale, nextDirection);
  if (options.persist ?? true) {
    setConfigKey(LANGUAGE_CONFIG_KEY, preference);
  }
  return true;
}

export const translateFromCatalog: Translate = ((
  key: TranslationKey,
  ...args: readonly unknown[]
) => {
  const preferred = readCatalogPath(activeCatalog, key);
  return translateWithFallback(preferred, key, args);
}) as Translate;

function translateWithFallback(
  preferred: unknown,
  key: TranslationKey,
  args: readonly unknown[],
): string {
  const fallback = readCatalogPath(en, key);
  const value = preferred ?? fallback;

  if (typeof value === "function") {
    const message = value as (...messageArgs: readonly unknown[]) => string;
    return message(...args);
  }
  if (typeof value === "string") return value;
  return key;
}

export function translateFromPartialCatalog<Key extends TranslationKey>(
  catalog: unknown,
  key: Key,
  ...args: MessageArgs<MessageAtPath<MessageCatalog, Key>>
): string {
  const preferred = readCatalogPath(catalog, key);
  return translateWithFallback(preferred, key, args);
}

export const translate: Translate = translateFromCatalog;

export function getLocalization() {
  return {
    get languagePreference(): LanguagePreference {
      return languagePreference;
    },
    get locale(): AppLocale {
      return resolvedLocale;
    },
    get direction(): LocaleDirection {
      return resolvedDirection;
    },
    t: translate,
    setLanguagePreference,
  };
}

if (typeof window !== "undefined") {
  applyDocumentLocale(DEFAULT_LOCALE, LOCALE_METADATA[DEFAULT_LOCALE].direction);
  window.addEventListener("languagechange", () => {
    if (languagePreference !== DEFAULT_LANGUAGE_PREFERENCE) return;
    void setLanguagePreference(DEFAULT_LANGUAGE_PREFERENCE, { persist: false });
  });
}
