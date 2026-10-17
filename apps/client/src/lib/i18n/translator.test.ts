// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";

const getConfigKeyMock = vi.fn();
const setConfigKeyMock = vi.fn();

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (...args: unknown[]) => getConfigKeyMock(...args),
  setConfigKey: (...args: unknown[]) => setConfigKeyMock(...args),
}));

function setBrowserLanguages(languages: readonly string[]): void {
  Object.defineProperty(window.navigator, "languages", {
    value: languages,
    configurable: true,
  });
  Object.defineProperty(window.navigator, "language", {
    value: languages[0] ?? "en-US",
    configurable: true,
  });
}

async function loadTranslator() {
  return await import("./translator.svelte");
}

beforeEach(() => {
  vi.resetModules();
  vi.doUnmock("$lib/i18n/messages/es");
  getConfigKeyMock.mockReset();
  setConfigKeyMock.mockReset();
  getConfigKeyMock.mockReturnValue(undefined);
  document.documentElement.lang = "";
  document.documentElement.dir = "";
  setBrowserLanguages(["en-US"]);
});

describe("translator", () => {
  it("starts in English without importing the Spanish runtime catalog", async () => {
    const { getLocalization, initializeLocalizationFromConfig } = await loadTranslator();
    const { localeCatalogHasLoaded } = await import("./catalog-loader");

    await expect(initializeLocalizationFromConfig()).resolves.toBe(true);

    expect(getLocalization().locale).toBe("en");
    expect(getLocalization().t("common.save")).toBe("Save");
    expect(localeCatalogHasLoaded("es")).toBe(false);
  });

  it("loads Spanish before completing Spanish startup", async () => {
    getConfigKeyMock.mockReturnValue("es");
    const { getLocalization, initializeLocalizationFromConfig } = await loadTranslator();

    await expect(initializeLocalizationFromConfig()).resolves.toBe(true);

    const localization = getLocalization();
    expect(localization.languagePreference).toBe("es");
    expect(localization.locale).toBe("es");
    expect(localization.t("common.save")).toBe("Guardar");
    expect(localization.t("format.relativeMinutesFuture", 3)).toBe("en 3 min");
    expect(document.documentElement.lang).toBe("es");
    expect(document.documentElement.dir).toBe("ltr");
  });

  it("switches atomically from English to Spanish and back", async () => {
    const { getLocalization } = await loadTranslator();
    const localization = getLocalization();
    await localization.setLanguagePreference("en", { persist: false });

    await expect(localization.setLanguagePreference("es")).resolves.toBe(true);
    expect(localization.languagePreference).toBe("es");
    expect(localization.locale).toBe("es");
    expect(localization.t("common.save")).toBe("Guardar");
    expect(setConfigKeyMock).toHaveBeenLastCalledWith("preferences.language", "es");

    await expect(localization.setLanguagePreference("en")).resolves.toBe(true);
    expect(localization.languagePreference).toBe("en");
    expect(localization.locale).toBe("en");
    expect(localization.t("common.save")).toBe("Save");
    expect(setConfigKeyMock).toHaveBeenLastCalledWith("preferences.language", "en");
  });

  it("does not let a slow previous catalog replace a newer language choice", async () => {
    const actualSpanish = await vi.importActual<typeof import("$lib/i18n/messages/es")>(
      "$lib/i18n/messages/es",
    );
    let resolveSpanish!: (module: typeof actualSpanish) => void;
    const spanishModule = new Promise<typeof actualSpanish>((resolve) => {
      resolveSpanish = resolve;
    });
    vi.doMock("$lib/i18n/messages/es", () => spanishModule);
    const { getLocalization } = await loadTranslator();
    const localization = getLocalization();
    await localization.setLanguagePreference("en", { persist: false });

    const slowSpanishSwitch = localization.setLanguagePreference("es", { persist: false });
    expect(localization.languagePreference).toBe("en");
    expect(localization.locale).toBe("en");
    expect(localization.t("common.save")).toBe("Save");
    await expect(localization.setLanguagePreference("en", { persist: false })).resolves.toBe(true);
    resolveSpanish(actualSpanish);

    await expect(slowSpanishSwitch).resolves.toBe(false);
    expect(localization.languagePreference).toBe("en");
    expect(localization.locale).toBe("en");
    expect(localization.t("common.save")).toBe("Save");
  });

  it("follows system language changes after the replacement catalog loads", async () => {
    const { getLocalization } = await loadTranslator();
    const localization = getLocalization();
    await localization.setLanguagePreference("system", { persist: false });

    setBrowserLanguages(["es-MX"]);
    window.dispatchEvent(new Event("languagechange"));
    await vi.waitFor(() => expect(localization.locale).toBe("es"));
    expect(localization.t("common.save")).toBe("Guardar");

    setBrowserLanguages(["en-CA"]);
    window.dispatchEvent(new Event("languagechange"));
    await vi.waitFor(() => expect(localization.locale).toBe("en"));
    expect(localization.t("common.save")).toBe("Save");

    await localization.setLanguagePreference("en", { persist: false });
    setBrowserLanguages(["es-MX"]);
    window.dispatchEvent(new Event("languagechange"));
    await Promise.resolve();
    expect(localization.locale).toBe("en");
    expect(localization.languagePreference).toBe("en");
  });

  it("retains the active language when a replacement catalog fails", async () => {
    vi.doMock("$lib/i18n/messages/es", () => {
      throw new Error("Spanish chunk failed");
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { getLocalization } = await loadTranslator();
    const localization = getLocalization();
    await localization.setLanguagePreference("en", { persist: false });

    await expect(localization.setLanguagePreference("es")).resolves.toBe(false);

    expect(localization.languagePreference).toBe("en");
    expect(localization.locale).toBe("en");
    expect(localization.t("common.save")).toBe("Save");
    expect(setConfigKeyMock).not.toHaveBeenCalled();
    expect(consoleError).toHaveBeenCalledOnce();
    consoleError.mockRestore();
  });

  it("falls back to English for missing partial catalog messages", async () => {
    const { translateFromPartialCatalog } = await loadTranslator();

    expect(translateFromPartialCatalog({}, "common.save")).toBe("Save");
    expect(translateFromPartialCatalog({}, "format.relativeMinutesFuture", 2)).toBe(
      "in 2 min",
    );
  });

  it("normalizes invalid stored language preferences", async () => {
    getConfigKeyMock.mockReturnValue("fr");
    const { getLocalization, initializeLocalizationFromConfig } = await loadTranslator();

    await initializeLocalizationFromConfig();

    expect(getLocalization().languagePreference).toBe("system");
    expect(setConfigKeyMock).toHaveBeenCalledWith("preferences.language", "system");
  });
});
