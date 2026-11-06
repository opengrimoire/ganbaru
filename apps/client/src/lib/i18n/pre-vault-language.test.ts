import { describe, expect, it } from "vitest";
import { APP_LOCALES, PLANNED_LOCALES } from "./locales";
import {
  PRE_VAULT_LANGUAGE_STORAGE_KEY,
  clearPreVaultLanguagePreference,
  plannedLanguageEntries,
  preferredPlannedLanguage,
  preVaultLanguageChoices,
  readPreVaultLanguagePreference,
  resolvedSystemLanguageName,
  writePreVaultLanguagePreference,
} from "./pre-vault-language";

function storageWith(initial: Record<string, string> = {}): Storage {
  const values = new Map(Object.entries(initial));
  return {
    get length() {
      return values.size;
    },
    clear() {
      values.clear();
    },
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    key(index: number) {
      return [...values.keys()][index] ?? null;
    },
    removeItem(key: string) {
      values.delete(key);
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
  };
}

describe("pre-vault language preference", () => {
  it("reads, writes, and clears the pending setup language", () => {
    const storage = storageWith();

    expect(readPreVaultLanguagePreference(storage)).toBeNull();

    writePreVaultLanguagePreference(storage, "es");
    expect(readPreVaultLanguagePreference(storage)).toBe("es");

    clearPreVaultLanguagePreference(storage);
    expect(readPreVaultLanguagePreference(storage)).toBeNull();
  });

  it("normalizes invalid stored values to the system preference", () => {
    const storage = storageWith({
      [PRE_VAULT_LANGUAGE_STORAGE_KEY]: "fr",
    });

    expect(readPreVaultLanguagePreference(storage)).toBe("system");
  });

  it("uses the system locale with English fallback for the default label", () => {
    expect(resolvedSystemLanguageName(["es-MX"])).toBe("Español");
    expect(resolvedSystemLanguageName(["fr-CA"])).toBe("English");
  });

  it("lists the system first, naming the language it resolves to in that language", () => {
    const [system] = preVaultLanguageChoices(["es-MX"], "System language");

    expect(system).toEqual({
      value: "system",
      title: "System language",
      titleLang: null,
      detail: "Español",
      detailLang: "es",
      preferredPlanned: null,
    });
  });

  it("names every app locale in English, alphabetically, with its own name underneath", () => {
    const choices = preVaultLanguageChoices(["en-US"], "System language");

    expect(choices.map((choice) => choice.value)).toEqual(["system", "en", "es"]);
    expect(new Set(choices.map((choice) => choice.value))).toEqual(new Set(["system", ...APP_LOCALES]));
    expect(choices[1]).toMatchObject({ title: "English", titleLang: "en", detail: "English", detailLang: "en" });
    expect(choices[2]).toMatchObject({ title: "Spanish", titleLang: "en", detail: "Español", detailLang: "es" });
  });

  it("keeps the language names fixed while only the system label follows the interface language", () => {
    const english = preVaultLanguageChoices(["fr-CA"], "System language");
    const spanish = preVaultLanguageChoices(["fr-CA"], "Idioma del sistema");

    expect(spanish[0]).toMatchObject({ title: "Idioma del sistema", detail: "English" });
    expect(spanish.slice(1)).toEqual(english.slice(1));
  });

  it("lists every planned language once, apart from the languages that already ship", () => {
    const entries = plannedLanguageEntries();

    expect(new Set(entries.map((entry) => entry.id))).toEqual(new Set(PLANNED_LOCALES.map((locale) => locale.id)));
    expect(new Set(entries.map((entry) => entry.id)).size).toBe(entries.length);
    for (const entry of entries) {
      expect(APP_LOCALES.some((locale) => entry.id.split("-")[0] === locale)).toBe(false);
    }
  });

  it("names planned languages in English, alphabetically, with their own names underneath", () => {
    const entries = plannedLanguageEntries();

    const titles = entries.map((entry) => entry.title);

    expect(titles).toEqual([...titles].sort(new Intl.Collator("en").compare));
    expect(titles.slice(0, 5)).toEqual(["Arabic", "Bengali", "Catalan", "Chinese (Simplified)", "Chinese (Traditional)"]);
    expect(titles.filter((title) => title.startsWith("Portuguese"))).toEqual(["Portuguese (Brazil)", "Portuguese (Portugal)"]);
    expect(titles.at(-1)).toBe("Vietnamese");
    expect(entries.find((entry) => entry.id === "ja")?.detail).toBe("日本語");
    expect(entries.find((entry) => entry.id === "ar")?.detail).toBe("العربية");
  });

  it("gives each planned language a note in its own language and direction", () => {
    const entries = plannedLanguageEntries();

    for (const entry of entries) expect(entry.notice.trim()).not.toBe("");
    expect(new Set(entries.map((entry) => entry.notice)).size).toBe(entries.length);
    expect(entries.find((entry) => entry.id === "fr")?.notice).toBe("Bientôt disponible. Veuillez m’excuser pour la gêne occasionnée.");
    expect(entries.find((entry) => entry.id === "ar")).toMatchObject({ notice: "قريبًا. أعتذر عن الإزعاج.", direction: "rtl" });
  });

  it("names a planned language the system prefers over the language it resolves to", () => {
    expect(preferredPlannedLanguage(["fr-FR"])).toEqual({ lang: "fr", direction: "ltr", text: "Français (bientôt disponible)" });
    expect(preferredPlannedLanguage(["fr-CA", "es-MX"])?.lang).toBe("fr");
    expect(preferredPlannedLanguage(["ar-EG"])).toMatchObject({ lang: "ar", direction: "rtl" });
    expect(preVaultLanguageChoices(["fr-FR"], "System language")[0]).toMatchObject({
      detail: "English",
      preferredPlanned: { lang: "fr" },
    });
  });

  it("names no planned language when an app language comes first or nothing planned matches", () => {
    expect(preferredPlannedLanguage(["es-MX", "fr-FR"])).toBeNull();
    expect(preferredPlannedLanguage(["en-GB"])).toBeNull();
    expect(preferredPlannedLanguage(["sw-KE"])).toBeNull();
    expect(preferredPlannedLanguage(["not a locale"])).toBeNull();
    expect(preferredPlannedLanguage([])).toBeNull();
    expect(preVaultLanguageChoices(["fr-FR"], "System language").slice(1).every((choice) => choice.preferredPlanned === null)).toBe(true);
  });

  it("matches planned variants by script and region, then by language alone", () => {
    expect(preferredPlannedLanguage(["pt-PT"])?.lang).toBe("pt-PT");
    expect(preferredPlannedLanguage(["pt"])?.lang).toBe("pt-BR");
    expect(preferredPlannedLanguage(["pt-AO"])?.lang).toBe("pt-BR");
    expect(preferredPlannedLanguage(["zh-TW"])?.lang).toBe("zh-Hant");
    expect(preferredPlannedLanguage(["zh-CN"])?.lang).toBe("zh-Hans");
    expect(preferredPlannedLanguage(["iw-IL"])?.lang).toBe("he");
  });

  it("gives every planned language a coming soon fragment", () => {
    for (const locale of PLANNED_LOCALES) expect(locale.comingSoon.trim()).not.toBe("");
  });
});
