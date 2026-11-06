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

/** A language planned for a later release: shown as coming, never selectable, and not an app locale yet. */
export interface PlannedLocale {
  /** The BCP 47 tag the translation will use. */
  readonly id: string;
  /** The English name, with any variant in parentheses so it sorts under the base language. */
  readonly label: string;
  readonly nativeLabel: string;
  readonly direction: LocaleDirection;
  /** A short note, written in the language itself, that it is coming soon, with an apology. */
  readonly comingNotice: string;
}

/** The languages planned after English and Spanish, in their intended order. */
export const PLANNED_LOCALES: readonly PlannedLocale[] = Object.freeze([
  { id: "pt-BR", label: "Portuguese (Brazil)", nativeLabel: "Português (Brasil)", direction: "ltr", comingNotice: "Em breve. Peço desculpas pelo inconveniente." },
  { id: "fr", label: "French", nativeLabel: "Français", direction: "ltr", comingNotice: "Bientôt disponible. Veuillez m’excuser pour la gêne occasionnée." },
  { id: "de", label: "German", nativeLabel: "Deutsch", direction: "ltr", comingNotice: "Demnächst verfügbar. Ich bitte die Unannehmlichkeiten zu entschuldigen." },
  { id: "it", label: "Italian", nativeLabel: "Italiano", direction: "ltr", comingNotice: "Presto disponibile. Mi scuso per il disagio." },
  { id: "ja", label: "Japanese", nativeLabel: "日本語", direction: "ltr", comingNotice: "近日対応予定です。ご不便をおかけして申し訳ありません。" },
  { id: "ko", label: "Korean", nativeLabel: "한국어", direction: "ltr", comingNotice: "곧 지원될 예정입니다. 불편을 드려 죄송합니다." },
  { id: "zh-Hans", label: "Chinese (Simplified)", nativeLabel: "简体中文", direction: "ltr", comingNotice: "即将推出。给您带来不便，敬请谅解。" },
  { id: "ru", label: "Russian", nativeLabel: "Русский", direction: "ltr", comingNotice: "Скоро появится. Приношу извинения за неудобства." },
  { id: "hi", label: "Hindi", nativeLabel: "हिन्दी", direction: "ltr", comingNotice: "जल्द ही आ रही है। असुविधा के लिए मुझे खेद है।" },
  { id: "ar", label: "Arabic", nativeLabel: "العربية", direction: "rtl", comingNotice: "قريبًا. أعتذر عن الإزعاج." },
  { id: "tr", label: "Turkish", nativeLabel: "Türkçe", direction: "ltr", comingNotice: "Çok yakında. Verdiğim rahatsızlık için özür dilerim." },
  { id: "id", label: "Indonesian", nativeLabel: "Bahasa Indonesia", direction: "ltr", comingNotice: "Segera hadir. Mohon maaf atas ketidaknyamanannya." },
  { id: "zh-Hant", label: "Chinese (Traditional)", nativeLabel: "繁體中文", direction: "ltr", comingNotice: "即將推出。造成不便，敬請見諒。" },
  { id: "vi", label: "Vietnamese", nativeLabel: "Tiếng Việt", direction: "ltr", comingNotice: "Sắp ra mắt. Xin lỗi vì sự bất tiện này." },
  { id: "pl", label: "Polish", nativeLabel: "Polski", direction: "ltr", comingNotice: "Już wkrótce. Przepraszam za niedogodności." },
  { id: "uk", label: "Ukrainian", nativeLabel: "Українська", direction: "ltr", comingNotice: "Незабаром. Перепрошую за незручності." },
  { id: "nl", label: "Dutch", nativeLabel: "Nederlands", direction: "ltr", comingNotice: "Binnenkort beschikbaar. Mijn excuses voor het ongemak." },
  { id: "fa", label: "Persian", nativeLabel: "فارسی", direction: "rtl", comingNotice: "به‌زودی. بابت این ناراحتی پوزش می‌خواهم." },
  { id: "th", label: "Thai", nativeLabel: "ไทย", direction: "ltr", comingNotice: "เร็ว ๆ นี้ ขออภัยในความไม่สะดวก" },
  { id: "bn", label: "Bengali", nativeLabel: "বাংলা", direction: "ltr", comingNotice: "শীঘ্রই আসছে। অসুবিধার জন্য আমি দুঃখিত।" },
  { id: "sv", label: "Swedish", nativeLabel: "Svenska", direction: "ltr", comingNotice: "Kommer snart. Jag ber om ursäkt för besväret." },
  { id: "cs", label: "Czech", nativeLabel: "Čeština", direction: "ltr", comingNotice: "Již brzy. Omlouvám se za nepříjemnosti." },
  { id: "fil", label: "Filipino", nativeLabel: "Filipino", direction: "ltr", comingNotice: "Malapit na. Paumanhin sa abala." },
  { id: "ur", label: "Urdu", nativeLabel: "اردو", direction: "rtl", comingNotice: "جلد آ رہی ہے۔ زحمت کے لیے معذرت خواہ ہوں۔" },
  { id: "pt-PT", label: "Portuguese (Portugal)", nativeLabel: "Português (Portugal)", direction: "ltr", comingNotice: "Brevemente. Peço desculpa pelo incómodo." },
  { id: "ca", label: "Catalan", nativeLabel: "Català", direction: "ltr", comingNotice: "Aviat disponible. Disculpeu les molèsties." },
  { id: "ro", label: "Romanian", nativeLabel: "Română", direction: "ltr", comingNotice: "În curând. Îmi cer scuze pentru inconvenient." },
  { id: "el", label: "Greek", nativeLabel: "Ελληνικά", direction: "ltr", comingNotice: "Έρχεται σύντομα. Ζητώ συγγνώμη για την ταλαιπωρία." },
  { id: "hu", label: "Hungarian", nativeLabel: "Magyar", direction: "ltr", comingNotice: "Hamarosan. Elnézést kérek a kellemetlenségért." },
  { id: "ms", label: "Malay", nativeLabel: "Bahasa Melayu", direction: "ltr", comingNotice: "Akan datang. Saya mohon maaf atas kesulitan ini." },
  { id: "ta", label: "Tamil", nativeLabel: "தமிழ்", direction: "ltr", comingNotice: "விரைவில் வருகிறது. சிரமத்திற்கு வருந்துகிறேன்." },
]);

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
