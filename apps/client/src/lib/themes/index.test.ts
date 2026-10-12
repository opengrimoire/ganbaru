import { describe, it, expect } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  BUILTIN_THEME_REGISTRY,
  DEFAULT_THEME_ID,
  APP_TOKEN_KEYS,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DERIVATION_ENGINE_VERSION,
  darkTheme,
  lightTheme,
  getThemeById,
  isBuiltinThemeId,
  isThemeCalendarDark,
  isThemeDark,
  pickQuickToggleTarget,
  resolveCalendarCanvas,
  resolveCanvas,
  computeThemeTokenOps,
  generateThemeId,
  serializeTheme,
  validateThemeJson,
  type ThemeSources,
  type UserTheme,
} from "$lib/themes";

const PALETTE_INDICES: readonly number[] = Array.from(
  { length: PALETTE_SIZE },
  (_, i) => i,
);

const SAMPLE_SOURCES_DARK: ThemeSources = {
  canvas: "#1B1C1F",
  ink: "#E9EAEE",
  primary: "#90A5FF",
  destructive: "#F06060",
  destructiveText: "#FFFFFF",
  confirm: "#44C48A",
  confirmText: "#000000",
  warning: "#F5B143",
  warningText: "#000000",
};

const SAMPLE_SOURCES_LIGHT: ThemeSources = {
  canvas: "#FAF7F2",
  ink: "#1F1B16",
  primary: "#2563EB",
  destructive: "#B42318",
  destructiveText: "#FFFFFF",
  confirm: "#047857",
  confirmText: "#FFFFFF",
  warning: "#B45309",
  warningText: "#FFFFFF",
};

function paletteOf(hex: string): string[] {
  return Array.from({ length: PALETTE_SIZE }, () => hex);
}

function makeUserTheme(overrides: Partial<UserTheme> = {}): UserTheme {
  const iconLabel = overrides.iconLabel ?? "dark";
  const sources =
    overrides.sources ?? (iconLabel === "dark" ? SAMPLE_SOURCES_DARK : SAMPLE_SOURCES_LIGHT);
  const eventPalette = overrides.eventPalette ?? paletteOf("#abcdef");
  const appTokens =
    overrides.appTokens ?? { ...BASE_APP_TOKENS[iconLabel] };
  const calendarTokens =
    overrides.calendarTokens ?? { ...BASE_CALENDAR_TOKENS[iconLabel] };
  const appIsolated = overrides.appIsolated ?? new Set<string>();
  const calendarIsolated = overrides.calendarIsolated ?? new Set<string>();
  return {
    kind: "user",
    id: overrides.id ?? "test",
    displayName: overrides.displayName ?? "Test",
    iconLabel,
    blendCanvas: overrides.blendCanvas ?? "#202020",
    eventPalette,
    derivationEngineVersion:
      overrides.derivationEngineVersion ?? DERIVATION_ENGINE_VERSION,
    calendarDefaultMode: overrides.calendarDefaultMode ?? "app-canvas",
    calendarDefaultCustom:
      overrides.calendarDefaultCustom ?? sources.canvas,
    sources,
    appTokens,
    calendarTokens,
    appIsolated,
    calendarIsolated,
    seedSources: overrides.seedSources ?? { ...sources },
    seedAppTokens: overrides.seedAppTokens ?? { ...appTokens },
    seedCalendarTokens:
      overrides.seedCalendarTokens ?? { ...calendarTokens },
    seedAppIsolated:
      overrides.seedAppIsolated ?? new Set(appIsolated),
    seedCalendarIsolated:
      overrides.seedCalendarIsolated ?? new Set(calendarIsolated),
    seedEventPalette:
      overrides.seedEventPalette ?? [...eventPalette],
    seedBlendCanvas:
      overrides.seedBlendCanvas ?? overrides.blendCanvas ?? "#202020",
    seedCalendarDefaultMode:
      overrides.seedCalendarDefaultMode ??
      overrides.calendarDefaultMode ??
      "app-canvas",
    seedCalendarDefaultCustom:
      overrides.seedCalendarDefaultCustom ??
      overrides.calendarDefaultCustom ??
      sources.canvas,
    seedIconLabel: overrides.seedIconLabel ?? iconLabel,
  };
}

function buildV1Input(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schemaVersion: 1,
    id: "custom-theme",
    displayName: "Custom Theme",
    iconLabel: "dark",
    blendCanvas: "#202020",
    derivationEngineVersion: DERIVATION_ENGINE_VERSION,
    sources: SAMPLE_SOURCES_DARK,
    appTokens: { ...BASE_APP_TOKENS.dark },
    calendarTokens: { ...BASE_CALENDAR_TOKENS.dark },
    appIsolated: [],
    calendarIsolated: [],
    eventPalette: paletteOf("#abcdef"),
    calendarDefaults: {
      mode: "custom",
      customBasis: "#334455",
    },
    ...overrides,
  };
}

describe("theme registry", () => {
  it("registers light and dark built-in themes", () => {
    expect(BUILTIN_THEME_REGISTRY[lightTheme.id]).toBe(lightTheme);
    expect(BUILTIN_THEME_REGISTRY[darkTheme.id]).toBe(darkTheme);
  });

  it("default theme ID is registered", () => {
    expect(DEFAULT_THEME_ID).toBe(lightTheme.id);
    expect(getThemeById(DEFAULT_THEME_ID)).toBeDefined();
  });

  it("freezes registry to prevent mutation", () => {
    expect(Object.isFrozen(BUILTIN_THEME_REGISTRY)).toBe(true);
  });
});

describe("built-in themes", () => {
  it("every built-in theme has a hex value at every palette index", () => {
    for (const theme of [lightTheme, darkTheme]) {
      expect(theme.eventPalette.length).toBe(PALETTE_SIZE);
      for (const i of PALETTE_INDICES) {
        expect(
          theme.eventPalette[i],
          `${theme.id} is missing index ${i}`,
        ).toMatch(/^#[0-9a-fA-F]{6}$/);
      }
    }
  });

  it("lightTheme has base 'light' and is tagged as builtin", () => {
    expect(lightTheme.base).toBe("light");
    expect(lightTheme.kind).toBe("builtin");
  });

  it("darkTheme has base 'dark' and is tagged as builtin", () => {
    expect(darkTheme.base).toBe("dark");
    expect(darkTheme.kind).toBe("builtin");
  });

  it("every built-in theme provides a hex blendCanvas", () => {
    for (const theme of [lightTheme, darkTheme]) {
      expect(theme.blendCanvas).toMatch(/^#[0-9a-fA-F]{6}$/);
    }
  });

  it("every built-in theme has a user-visible displayName", () => {
    for (const theme of [lightTheme, darkTheme]) {
      expect(theme.displayName.length).toBeGreaterThan(0);
    }
  });

  it("light and dark palettes differ for every slot", () => {
    for (const i of PALETTE_INDICES) {
      expect(
        lightTheme.eventPalette[i].toLowerCase(),
        `index ${i} is identical across light and dark`,
      ).not.toBe(darkTheme.eventPalette[i].toLowerCase());
    }
  });
});

describe("getThemeById", () => {
  it("returns the theme for a valid ID", () => {
    expect(getThemeById("light")).toBe(lightTheme);
    expect(getThemeById("dark")).toBe(darkTheme);
  });

  it("returns undefined for unknown IDs", () => {
    expect(getThemeById("nope")).toBeUndefined();
  });

  it("returns undefined for null, undefined, and empty inputs", () => {
    expect(getThemeById(null)).toBeUndefined();
    expect(getThemeById(undefined)).toBeUndefined();
    expect(getThemeById("")).toBeUndefined();
  });

  it("guards against prototype-chain keys", () => {
    expect(getThemeById("toString")).toBeUndefined();
    expect(getThemeById("__proto__")).toBeUndefined();
    expect(getThemeById("constructor")).toBeUndefined();
  });
});

describe("pickQuickToggleTarget", () => {
  it("switches from the configured light target to the configured dark target", () => {
    expect(
      pickQuickToggleTarget({
        activeId: "light-custom",
        activeIsDark: false,
        lightId: "light-custom",
        darkId: "dark-custom",
      }),
    ).toBe("dark-custom");
  });

  it("switches from the configured dark target to the configured light target", () => {
    expect(
      pickQuickToggleTarget({
        activeId: "dark-custom",
        activeIsDark: true,
        lightId: "light-custom",
        darkId: "dark-custom",
      }),
    ).toBe("light-custom");
  });

  it("uses current luminance when the active theme is neither configured target", () => {
    expect(
      pickQuickToggleTarget({
        activeId: "neutral",
        activeIsDark: true,
        lightId: "light-custom",
        darkId: "dark-custom",
      }),
    ).toBe("light-custom");
    expect(
      pickQuickToggleTarget({
        activeId: "neutral",
        activeIsDark: false,
        lightId: "light-custom",
        darkId: "dark-custom",
      }),
    ).toBe("dark-custom");
  });
});

describe("computeThemeTokenOps", () => {
  it("returns no-op for built-in themes when nothing was applied", () => {
    const result = computeThemeTokenOps(lightTheme, new Set());
    expect(result.toSet.size).toBe(0);
    expect(result.toClear.size).toBe(0);
    expect(result.applied.size).toBe(0);
  });

  it("paints editable snapshot tokens and derived runtime tokens", () => {
    const customApp = { ...BASE_APP_TOKENS.dark, "--primary": "#abc", "--background": "#fff" };
    const customCalendarTokens = { ...BASE_CALENDAR_TOKENS.dark, "--cal-bg": "#eee" };
    const theme = makeUserTheme({
      appTokens: customApp,
      calendarTokens: customCalendarTokens,
    });
    const result = computeThemeTokenOps(theme, new Set());
    expect(result.toSet.get("--primary")).toBe("#abc");
    expect(result.toSet.get("--background")).toBe("#fff");
    expect(result.toSet.get("--cal-bg")).toBe("#eee");
    expect(result.toSet.get("--cal-scrollbar-thumb")).toBeDefined();
    expect(result.toClear.size).toBe(0);
    expect(result.toSet.has("--event-panel-edge")).toBe(true);
    expect(result.toSet.has("--pomodoro-idle-text")).toBe(false);
    // Every editable app and calendar key participates in the snapshot.
    expect(result.applied.has("--primary")).toBe(true);
    expect(result.applied.has("--cal-bg")).toBe(true);
    expect(result.applied.has("--cal-scrollbar-thumb")).toBe(true);
  });

  it("uses curated scrollbar colors for light and dark calendar defaults", () => {
    const lightDefault = makeUserTheme({
      calendarDefaultMode: "light",
      sources: SAMPLE_SOURCES_DARK,
    });
    const darkDefault = makeUserTheme({
      calendarDefaultMode: "dark",
      sources: SAMPLE_SOURCES_LIGHT,
    });

    expect(
      computeThemeTokenOps(lightDefault, new Set()).toSet.get(
        "--cal-scrollbar-thumb",
      ),
    ).toBe(BASE_APP_TOKENS.light["--muted"]);
    expect(
      computeThemeTokenOps(darkDefault, new Set()).toSet.get(
        "--cal-scrollbar-thumb",
      ),
    ).toBe(BASE_APP_TOKENS.dark["--muted"]);
  });

  it("clears stale implementation tokens that user themes no longer paint", () => {
    const theme = makeUserTheme();
    const previous = new Set([
      "--primary",
      "--pomodoro-idle-text",
      "--cal-drag-preview-border",
    ]);
    const result = computeThemeTokenOps(theme, previous);
    expect(result.toClear).toEqual(
      new Set(["--pomodoro-idle-text", "--cal-drag-preview-border"]),
    );
  });

  it("clears previously applied tokens that the new built-in does not paint", () => {
    const previous = new Set(["--primary", "--background", "--cal-bg"]);
    const result = computeThemeTokenOps(lightTheme, previous);
    expect(result.toSet.size).toBe(0);
    expect(result.toClear).toEqual(new Set(["--primary", "--background", "--cal-bg"]));
    expect(result.applied.size).toBe(0);
  });

  it("does not re-clear keys the new theme also paints", () => {
    const theme = makeUserTheme();
    const previous = new Set(["--primary"]);
    const result = computeThemeTokenOps(theme, previous);
    expect(result.toSet.has("--primary")).toBe(true);
    expect(result.toClear.has("--primary")).toBe(false);
  });
});

describe("isBuiltinThemeId", () => {
  it("identifies built-in IDs", () => {
    expect(isBuiltinThemeId("light")).toBe(true);
    expect(isBuiltinThemeId("dark")).toBe(true);
  });

  it("rejects unknown IDs", () => {
    expect(isBuiltinThemeId("custom-abc")).toBe(false);
    expect(isBuiltinThemeId(undefined)).toBe(false);
    expect(isBuiltinThemeId(null)).toBe(false);
    expect(isBuiltinThemeId("")).toBe(false);
  });

  it("guards against prototype-chain keys", () => {
    expect(isBuiltinThemeId("toString")).toBe(false);
    expect(isBuiltinThemeId("__proto__")).toBe(false);
  });
});

describe("token catalogs", () => {
  it("APP_TOKEN_KEYS only contains -- prefixed names", () => {
    for (const key of APP_TOKEN_KEYS) expect(key.startsWith("--")).toBe(true);
  });

  it("CALENDAR_TOKEN_KEYS only contains --cal- prefixed names", () => {
    for (const key of CALENDAR_TOKEN_KEYS) expect(key.startsWith("--cal-")).toBe(true);
  });

  it("are frozen", () => {
    expect(Object.isFrozen(APP_TOKEN_KEYS)).toBe(true);
    expect(Object.isFrozen(CALENDAR_TOKEN_KEYS)).toBe(true);
  });
});

describe("luminance-driven canvas resolution", () => {
  it("resolveCanvas returns the snapshot --background for user themes", () => {
    const theme = makeUserTheme({
      sources: SAMPLE_SOURCES_LIGHT,
      appTokens: { ...BASE_APP_TOKENS.dark, "--background": "#FAF7F2" },
    });
    expect(resolveCanvas(theme).toLowerCase()).toBe("#faf7f2");
  });

  it("isThemeDark returns false when the snapshot canvas is light", () => {
    const theme = makeUserTheme({
      appTokens: { ...BASE_APP_TOKENS.dark, "--background": "#FAF7F2" },
    });
    expect(isThemeDark(theme)).toBe(false);
  });

  it("isThemeDark returns true when the snapshot canvas is dark", () => {
    const theme = makeUserTheme({
      iconLabel: "light",
      appTokens: { ...BASE_APP_TOKENS.light, "--background": "#1B1C1F" },
    });
    expect(isThemeDark(theme)).toBe(true);
  });

  it("isThemeCalendarDark keys off the snapshot --cal-bg", () => {
    const theme = makeUserTheme({
      iconLabel: "light",
      appTokens: { ...BASE_APP_TOKENS.light, "--background": "#FAF7F2" },
      calendarTokens: { ...BASE_CALENDAR_TOKENS.light, "--cal-bg": "#0E0F11" },
    });
    expect(isThemeDark(theme)).toBe(false);
    expect(isThemeCalendarDark(theme)).toBe(true);
  });

  it("falls back to base CSS for built-ins", () => {
    expect(resolveCanvas(lightTheme).toLowerCase()).toBe("#f4f4f7");
    expect(resolveCanvas(darkTheme).toLowerCase()).toBe("#27282a");
    expect(resolveCalendarCanvas(lightTheme).toLowerCase()).toBe("#ffffff");
    expect(resolveCalendarCanvas(darkTheme).toLowerCase()).toBe("#131314");
    expect(isThemeDark(lightTheme)).toBe(false);
    expect(isThemeDark(darkTheme)).toBe(true);
    expect(isThemeCalendarDark(lightTheme)).toBe(false);
    expect(isThemeCalendarDark(darkTheme)).toBe(true);
  });
});

describe("generateThemeId", () => {
  it("returns a unique ID not in the existing registry", () => {
    const existing = { "theme-aaaaaa": darkTheme };
    const id = generateThemeId(existing);
    expect(existing).not.toHaveProperty(id);
    expect(id.startsWith("theme-")).toBe(true);
  });

  it("uses the provided slug-shaped prefix", () => {
    const id = generateThemeId({}, "midnight");
    expect(id.startsWith("midnight-")).toBe(true);
  });

  it("falls back to the default prefix when given a non-slug prefix", () => {
    const id = generateThemeId({}, "not a slug!");
    expect(id.startsWith("theme-")).toBe(true);
  });
});

describe("validateThemeJson current schema", () => {
  it("accepts calendar defaults", () => {
    const result = validateThemeJson(buildV1Input());
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.theme.calendarDefaultMode).toBe("custom");
      expect(result.theme.calendarDefaultCustom).toBe("#334455");
      expect(result.theme.seedCalendarDefaultMode).toBe("custom");
      expect(result.theme.seedCalendarDefaultCustom).toBe("#334455");
    }
  });

  it("rejects invalid calendar defaults", () => {
    const result = validateThemeJson(
      buildV1Input({
        calendarDefaults: { mode: "rainbow", customBasis: "blue" },
      }),
    );
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(
        result.errors.some((e) => e.includes("calendarDefaults.mode")),
      ).toBe(true);
      expect(
        result.errors.some((e) => e.includes("calendarDefaults.customBasis")),
      ).toBe(true);
    }
  });

  it("rejects unsupported schema versions", () => {
    expect(validateThemeJson({ ...buildV1Input(), schemaVersion: 2 }).ok).toBe(false);
    expect(validateThemeJson({ ...buildV1Input(), schemaVersion: undefined }).ok).toBe(false);
  });

  it("rejects missing current-shape fields", () => {
    const input = buildV1Input();
    delete input.sources;
    const result = validateThemeJson(input);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.errors.some((e) => e.includes("sources"))).toBe(true);
    }
  });
});

describe("validateThemeJson rejection cases", () => {
  it("rejects non-object inputs", () => {
    expect(validateThemeJson(null).ok).toBe(false);
    expect(validateThemeJson("not an object").ok).toBe(false);
    expect(validateThemeJson(42).ok).toBe(false);
    expect(validateThemeJson([]).ok).toBe(false);
  });

  it("rejects an id that collides with a built-in", () => {
    const result = validateThemeJson(buildV1Input({ id: "light" }));
    expect(result.ok).toBe(false);
  });
});

describe("serializeTheme round-trip", () => {
  it("survives validate(serialize(t)) for a user theme", () => {
    const original = makeUserTheme({
      id: "round-trip",
      displayName: "Round Trip",
      iconLabel: "light",
      blendCanvas: "#202020",
      calendarDefaultMode: "custom",
      calendarDefaultCustom: "#334455",
      sources: SAMPLE_SOURCES_DARK,
      appIsolated: new Set(["--primary"]),
      calendarIsolated: new Set(["--cal-bg"]),
    });
    const json = serializeTheme(original);
    const reparsed = validateThemeJson(JSON.parse(json));
    expect(reparsed.ok).toBe(true);
    if (reparsed.ok) {
      expect(reparsed.theme.id).toBe(original.id);
      expect(reparsed.theme.displayName).toBe(original.displayName);
      expect(reparsed.theme.iconLabel).toBe(original.iconLabel);
      expect(reparsed.theme.seedIconLabel).toBe(original.iconLabel);
      expect(reparsed.theme.derivationEngineVersion).toBe(
        original.derivationEngineVersion,
      );
      expect(reparsed.theme.sources).toEqual(original.sources);
      expect(reparsed.theme.calendarDefaultMode).toBe("custom");
      expect(reparsed.theme.calendarDefaultCustom).toBe("#334455");
      expect(reparsed.theme.appIsolated.has("--primary")).toBe(true);
      expect(reparsed.theme.calendarIsolated.has("--cal-bg")).toBe(true);
    }
  });

  it("emits user themes with schemaVersion: 1", () => {
    const theme = makeUserTheme();
    const parsed = JSON.parse(serializeTheme(theme)) as Record<string, unknown>;
    expect(parsed.schemaVersion).toBe(1);
  });

  it("emits keys in canonical order for user themes", () => {
    const theme = makeUserTheme({
      appIsolated: new Set(["--primary"]),
    });
    const json = serializeTheme(theme);
    const parsed = JSON.parse(json) as Record<string, unknown>;
    expect(Object.keys(parsed)).toEqual([
      "schemaVersion",
      "id",
      "displayName",
      "iconLabel",
      "derivationEngineVersion",
      "calendarDefaults",
      "sources",
      "appTokens",
      "calendarTokens",
      "eventPalette",
      "blendCanvas",
      "appIsolated",
      "calendarIsolated",
    ]);
    expect(Object.keys(parsed.appTokens as Record<string, unknown>)).toEqual(
      APP_TOKEN_KEYS,
    );
    expect(
      Object.keys(parsed.calendarDefaults as Record<string, unknown>),
    ).toEqual(["mode", "customBasis"]);
    expect(Object.keys(parsed.calendarTokens as Record<string, unknown>)).toEqual(
      CALENDAR_TOKEN_KEYS,
    );
  });

  it("does not emit seeds in the export", () => {
    const theme = makeUserTheme();
    const json = serializeTheme(theme);
    expect(json).not.toContain("seedSources");
    expect(json).not.toContain("seedAppTokens");
    expect(json).not.toContain("seedBlendCanvas");
    expect(json).not.toContain("seedCalendarDefault");
  });

  it("emits a minimal payload for built-ins without schemaVersion", () => {
    const json = serializeTheme(lightTheme);
    const parsed = JSON.parse(json) as Record<string, unknown>;
    expect(parsed).not.toHaveProperty("schemaVersion");
    expect(parsed.id).toBe("light");
  });
});
