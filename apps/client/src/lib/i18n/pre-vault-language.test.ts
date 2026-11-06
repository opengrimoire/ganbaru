import { describe, expect, it } from "vitest";
import { APP_LOCALES, type AppLocale } from "./locales";
import {
  PRE_VAULT_LANGUAGE_STORAGE_KEY,
  clearPreVaultLanguagePreference,
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

  it("names every app locale by its own name, qualified only where the interface name differs", () => {
    const choices = preVaultLanguageChoices(["en-US"], "System language", englishNames);

    expect(choices.map((choice) => choice.value)).toEqual(["system", ...APP_LOCALES]);
    expect(choices.find((choice) => choice.value === "en")).toMatchObject({ title: "English", titleLang: "en", detail: null });
    expect(choices.find((choice) => choice.value === "es")).toMatchObject({ title: "Español", titleLang: "es", detail: "Spanish" });
  });

  it("drops the qualifier for the interface's own language when it is shown in another", () => {
    const spanishNames = (locale: AppLocale): string => (locale === "en" ? "Inglés" : "Español");
    const choices = preVaultLanguageChoices(["fr-CA"], "Idioma del sistema", spanishNames);

    expect(choices[0].detail).toBe("English");
    expect(choices.find((choice) => choice.value === "en")?.detail).toBe("Inglés");
    expect(choices.find((choice) => choice.value === "es")?.detail).toBeNull();
  });
});
