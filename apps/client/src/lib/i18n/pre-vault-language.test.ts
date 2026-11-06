import { describe, expect, it, vi } from "vitest";
import { APP_LOCALES, PLANNED_LOCALES, type AppLocale } from "./locales";
import {
  PRE_VAULT_LANGUAGE_STORAGE_KEY,
  clearPreVaultLanguagePreference,
  plannedLanguageEntries,
  preVaultLanguageChoices,
  readPreVaultLanguagePreference,
  resolvedSystemLanguageName,
  writePreVaultLanguagePreference,
} from "./pre-vault-language";

function englishNames(locale: AppLocale): string {
  return locale === "en" ? "English" : "Spanish";
}

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
    const [system] = preVaultLanguageChoices(["es-MX"], "System language", englishNames);

    expect(system).toEqual({
      value: "system",
      title: "System language",
      titleLang: null,
      detail: "Español",
      detailLang: "es",
    });
  });

  it("names every app locale by its own name, with its name in the interface language underneath", () => {
    const choices = preVaultLanguageChoices(["en-US"], "System language", englishNames);

    expect(choices.map((choice) => choice.value)).toEqual(["system", ...APP_LOCALES]);
    expect(choices.find((choice) => choice.value === "en")).toMatchObject({ title: "English", titleLang: "en", detail: "English" });
    expect(choices.find((choice) => choice.value === "es")).toMatchObject({ title: "Español", titleLang: "es", detail: "Spanish" });
  });

  it("lists the planned languages by their own names, apart from the languages that already ship", () => {
    const entries = plannedLanguageEntries("en");

    expect(entries.map((entry) => entry.id)).toEqual(PLANNED_LOCALES.map((locale) => locale.id));
    expect(new Set(entries.map((entry) => entry.id)).size).toBe(entries.length);
    for (const entry of entries) {
      expect(APP_LOCALES.some((locale) => entry.id.split("-")[0] === locale)).toBe(false);
    }
    expect(entries.find((entry) => entry.id === "ja")?.title).toBe("日本語");
    expect(entries.find((entry) => entry.id === "ar")?.direction).toBe("rtl");
  });

  it("names planned languages in the interface language, starting with a capital", () => {
    expect(plannedLanguageEntries("en").find((entry) => entry.id === "fr")?.detail).toBe("French");
    expect(plannedLanguageEntries("es").find((entry) => entry.id === "fr")?.detail).toBe("Francés");
    expect(plannedLanguageEntries("es").find((entry) => entry.id === "de")?.detail).toBe("Alemán");
  });

  it("falls back to the English name when the platform cannot name a planned language", () => {
    vi.stubGlobal("Intl", { ...Intl, DisplayNames: undefined });
    try {
      expect(plannedLanguageEntries("es").find((entry) => entry.id === "fr")?.detail).toBe("French");
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("keeps each language's own name while the second line follows the interface language", () => {
    const spanishNames = (locale: AppLocale): string => (locale === "en" ? "Inglés" : "Español");
    const choices = preVaultLanguageChoices(["fr-CA"], "Idioma del sistema", spanishNames);

    expect(choices[0].detail).toBe("English");
    expect(choices.find((choice) => choice.value === "en")).toMatchObject({ title: "English", detail: "Inglés" });
    expect(choices.find((choice) => choice.value === "es")).toMatchObject({ title: "Español", detail: "Español" });
  });
});
