import { describe, it, expect } from "vitest";
import { PALETTE_SIZE } from "$lib/calendar/types";
import {
  canResetTokenToSeed,
  cloneTheme,
  mergeThemePatch,
  nextUniqueDisplayName,
  normalizeDisplayName,
  themeIdCollisionError,
  toUserThemeSnapshot,
} from "./operations";
import {
  APP_TOKEN_KEYS,
  BUILTIN_THEME_REGISTRY,
  BASE_APP_TOKENS,
  BASE_CALENDAR_TOKENS,
  CALENDAR_TOKEN_KEYS,
  DERIVATION_ENGINE_VERSION,
  darkTheme,
  isSemanticSignalAppToken,
  lightTheme,
  serializeTheme,
  type ThemeSources,
  type UserTheme,
} from "$lib/themes";

const DEFAULT_SOURCES_DARK: ThemeSources = {
  canvas: "#101010",
  ink: "#eaeaea",
  primary: "#5a8cff",
  destructive: "#f06060",
  destructiveText: "#ffffff",
  confirm: "#44c48a",
  confirmText: "#000000",
  warning: "#f5b143",
  warningText: "#000000",
};

function paletteOf(hex: string): string[] {
  return Array.from({ length: PALETTE_SIZE }, () => hex);
}

function makeUserTheme(overrides: Partial<UserTheme> = {}): UserTheme {
  const eventPalette = paletteOf("#abcdef");
  const sources = overrides.sources ?? { ...DEFAULT_SOURCES_DARK };
  const iconLabel = overrides.iconLabel ?? "dark";
  const appTokens =
    overrides.appTokens ?? { ...BASE_APP_TOKENS[iconLabel] };
  const calendarTokens =
    overrides.calendarTokens ?? { ...BASE_CALENDAR_TOKENS[iconLabel] };
  const appIsolated = overrides.appIsolated ?? new Set<string>();
  const calendarIsolated = overrides.calendarIsolated ?? new Set<string>();
  return {
    kind: "user",
    id: overrides.id ?? "seed",
    displayName: overrides.displayName ?? "Seed",
    iconLabel,
    blendCanvas: overrides.blendCanvas ?? "#101010",
    eventPalette: overrides.eventPalette ?? eventPalette,
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
      overrides.seedEventPalette ?? [...(overrides.eventPalette ?? eventPalette)],
    seedBlendCanvas: overrides.seedBlendCanvas ?? overrides.blendCanvas ?? "#101010",
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

describe("cloneTheme", () => {
  it("assigns the supplied id and displayName", () => {
    const copy = cloneTheme(makeUserTheme(), "fork", "Fork");
    expect(copy.id).toBe("fork");
    expect(copy.displayName).toBe("Fork");
  });

  it("tags the clone as a user theme", () => {
    const copy = cloneTheme(makeUserTheme(), "fork", "Fork");
    expect(copy.kind).toBe("user");
  });

  it("copies blendCanvas verbatim", () => {
    const source = makeUserTheme({ iconLabel: "light", blendCanvas: "#fafafa" });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.blendCanvas).toBe("#fafafa");
  });

  it("clones the eventPalette so source mutations do not leak", () => {
    const source = makeUserTheme();
    const copy = cloneTheme(source, "fork", "Fork");
    (copy.eventPalette as string[])[2] = "#ff0000";
    expect(source.eventPalette[2]).toBe("#abcdef");
  });

  it("carries forward isolated app-token flags from a user source", () => {
    const source = makeUserTheme({
      appIsolated: new Set(["--primary", "--card"]),
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.appIsolated.has("--primary")).toBe(true);
    expect(copy.appIsolated.has("--card")).toBe(true);
  });

  it("carries forward isolated calendar-token flags from a user source", () => {
    const source = makeUserTheme({
      calendarIsolated: new Set(["--cal-bg"]),
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.calendarIsolated.has("--cal-bg")).toBe(true);
  });

  it("does not invent isolated flags on a built-in clone", () => {
    // Cloning a built-in should produce an unpinned theme so the first
    // canvas edit cascades through every surface.
    const copy = cloneTheme(darkTheme, "fork", "Fork");
    expect(copy.appIsolated.size).toBe(0);
    expect(copy.calendarIsolated.size).toBe(0);
  });

  it("synthesizes sources from the resolved palette when source is a built-in", () => {
    const copy = cloneTheme(lightTheme, "fork", "Fork");
    expect(copy.sources).toEqual({
      canvas: BASE_APP_TOKENS.light["--background"],
      ink: BASE_APP_TOKENS.light["--foreground"],
      primary: BASE_APP_TOKENS.light["--primary"],
      destructive: BASE_APP_TOKENS.light["--destructive"],
      destructiveText: BASE_APP_TOKENS.light["--destructive-foreground"],
      confirm: BASE_APP_TOKENS.light["--action-confirm"],
      confirmText: BASE_APP_TOKENS.light["--action-confirm-foreground"],
      warning: BASE_APP_TOKENS.light["--status-tentative"],
      warningText: BASE_APP_TOKENS.light["--status-tentative-foreground"],
    });
  });

  it("preserves sources verbatim when the source is a user theme", () => {
    const theSources: ThemeSources = {
      canvas: "#111111",
      ink: "#eeeeee",
      primary: "#00aaff",
      destructive: "#ff0033",
      destructiveText: "#ffffff",
      confirm: "#00cc88",
      confirmText: "#000000",
      warning: "#f5a524",
      warningText: "#000000",
    };
    const source = makeUserTheme({ sources: theSources });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.sources).toEqual(theSources);
    expect(copy.sources).not.toBe(source.sources);
  });

  it("snapshots appTokens from the resolved source palette", () => {
    const customApp = { ...BASE_APP_TOKENS.dark, "--primary": "#abc123" };
    const source = makeUserTheme({
      appTokens: customApp,
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.appTokens["--primary"]).toBe("#abc123");
    for (const key of APP_TOKEN_KEYS) {
      if (key === "--primary" || isSemanticSignalAppToken(key)) continue;
      expect(copy.appTokens[key]).toBe(BASE_APP_TOKENS.dark[key]);
    }
    expect(copy.appTokens["--destructive"]).toBe(source.sources.destructive);
    expect(copy.appTokens["--destructive-foreground"]).toBe(
      source.sources.destructiveText,
    );
  });

  it("snapshots calendarTokens from the resolved source palette", () => {
    const customCalendarTokens = { ...BASE_CALENDAR_TOKENS.dark, "--cal-bg": "#202020" };
    const source = makeUserTheme({
      calendarTokens: customCalendarTokens,
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.calendarTokens["--cal-bg"]).toBe("#202020");
    for (const key of CALENDAR_TOKEN_KEYS) {
      if (key === "--cal-bg") continue;
      expect(copy.calendarTokens[key]).toBe(BASE_CALENDAR_TOKENS.dark[key]);
    }
  });

  it("clones seedEventPalette so later mutations do not leak", () => {
    const source = makeUserTheme();
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedEventPalette).toEqual(copy.eventPalette);
    expect(copy.seedEventPalette).not.toBe(copy.eventPalette);
  });

  it("snapshots seedSources matching the clone's sources", () => {
    const source = makeUserTheme({ iconLabel: "light" });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedSources).toEqual(copy.sources);
    expect(copy.seedSources).not.toBe(copy.sources);
  });

  it("snapshots seedBlendCanvas from the source blendCanvas", () => {
    const source = makeUserTheme({ blendCanvas: "#fafafa" });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedBlendCanvas).toBe("#fafafa");
  });

  it("snapshots seedAppTokens equal to live appTokens at clone time", () => {
    const customApp = { ...BASE_APP_TOKENS.dark, "--primary": "#abc123" };
    const source = makeUserTheme({ appTokens: customApp });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedAppTokens).toEqual(copy.appTokens);
    expect(copy.seedAppTokens).not.toBe(copy.appTokens);
  });

  it("snapshots seedCalendarTokens equal to live calendarTokens at clone time", () => {
    const customCalendarTokens = { ...BASE_CALENDAR_TOKENS.dark, "--cal-bg": "#202020" };
    const source = makeUserTheme({ calendarTokens: customCalendarTokens });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedCalendarTokens).toEqual(copy.calendarTokens);
    expect(copy.seedCalendarTokens).not.toBe(copy.calendarTokens);
  });

  it("seeds isolated flag sets that mirror the clone's live flags", () => {
    const source = makeUserTheme({
      appIsolated: new Set(["--primary"]),
      calendarIsolated: new Set(["--cal-bg"]),
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.seedAppIsolated.has("--primary")).toBe(true);
    expect(copy.seedCalendarIsolated.has("--cal-bg")).toBe(true);
    // Independent set instances so mutations on one do not leak.
    expect(copy.seedAppIsolated).not.toBe(copy.appIsolated);
    expect(copy.seedCalendarIsolated).not.toBe(copy.calendarIsolated);
  });

  it("stamps engine version at the current code constant for built-in clones", () => {
    const copy = cloneTheme(darkTheme, "fork", "Fork");
    expect(copy.derivationEngineVersion).toBe(DERIVATION_ENGINE_VERSION);
  });

  it("carries the source's engine version stamp on user clones", () => {
    const source = makeUserTheme({ derivationEngineVersion: 0 });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.derivationEngineVersion).toBe(0);
  });

  it("carries the source's iconLabel verbatim on user clones", () => {
    const source = makeUserTheme({ iconLabel: "light" });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.iconLabel).toBe("light");
    expect(copy.seedIconLabel).toBe("light");
  });

  it("carries calendar defaults on user clones", () => {
    const source = makeUserTheme({
      calendarDefaultMode: "custom",
      calendarDefaultCustom: "#334455",
    });
    const copy = cloneTheme(source, "fork", "Fork");
    expect(copy.calendarDefaultMode).toBe("custom");
    expect(copy.calendarDefaultCustom).toBe("#334455");
    expect(copy.seedCalendarDefaultMode).toBe("custom");
    expect(copy.seedCalendarDefaultCustom).toBe("#334455");
  });

  it("pegs iconLabel to base on built-in clones", () => {
    const fromLight = cloneTheme(lightTheme, "fork-light", "Fork Light");
    const fromDark = cloneTheme(darkTheme, "fork-dark", "Fork Dark");
    expect(fromLight.iconLabel).toBe("light");
    expect(fromLight.seedIconLabel).toBe("light");
    expect(fromDark.iconLabel).toBe("dark");
    expect(fromDark.seedIconLabel).toBe("dark");
  });

  it("starts built-in clones with calendar defaults matching their base", () => {
    const fromLight = cloneTheme(lightTheme, "fork-light", "Fork Light");
    const fromDark = cloneTheme(darkTheme, "fork-dark", "Fork Dark");

    expect(fromLight.calendarDefaultMode).toBe("light");
    expect(fromLight.seedCalendarDefaultMode).toBe("light");
    expect(fromLight.calendarDefaultCustom).toBe(
      BASE_APP_TOKENS.light["--background"],
    );
    expect(fromDark.calendarDefaultMode).toBe("dark");
    expect(fromDark.seedCalendarDefaultMode).toBe("dark");
    expect(fromDark.calendarDefaultCustom).toBe(
      BASE_APP_TOKENS.dark["--background"],
    );
  });
});

describe("toUserThemeSnapshot", () => {
  it("projects built-ins into the full user-theme export shape", () => {
    const snapshot = toUserThemeSnapshot(lightTheme);
    const parsed = JSON.parse(serializeTheme(snapshot)) as Record<string, unknown>;

    expect(snapshot.kind).toBe("user");
    expect(snapshot.id).toBe(lightTheme.id);
    expect(snapshot.displayName).toBe(lightTheme.displayName);
    expect(parsed.schemaVersion).toBe(1);
    expect(Object.keys(parsed.appTokens as Record<string, unknown>)).toEqual(
      APP_TOKEN_KEYS,
    );
    expect(Object.keys(parsed.calendarTokens as Record<string, unknown>)).toEqual(
      CALENDAR_TOKEN_KEYS,
    );
  });

  it("returns existing user themes without cloning", () => {
    const theme = makeUserTheme();
    expect(toUserThemeSnapshot(theme)).toBe(theme);
  });
});

describe("themeIdCollisionError", () => {
  it("rejects ids that belong to built-in themes", () => {
    expect(themeIdCollisionError("light", BUILTIN_THEME_REGISTRY)).toBe(
      "id must not collide with a built-in theme",
    );
  });

  it("rejects ids that belong to user themes", () => {
    const custom = makeUserTheme({ id: "focus" });
    const registry = { ...BUILTIN_THEME_REGISTRY, [custom.id]: custom };
    expect(themeIdCollisionError("focus", registry)).toBe(
      'id "focus" is already used by another theme',
    );
  });

  it("allows ids absent from the current registry", () => {
    expect(themeIdCollisionError("new-theme", BUILTIN_THEME_REGISTRY)).toBeUndefined();
  });
});

describe("canResetTokenToSeed", () => {
  it("keeps linked app tokens reset through their source", () => {
    const theme = makeUserTheme({
      appTokens: {
        ...BASE_APP_TOKENS.dark,
        "--card": "#333333",
      },
      seedAppTokens: {
        ...BASE_APP_TOKENS.dark,
        "--card": "#202020",
      },
      appIsolated: new Set(),
      seedAppIsolated: new Set(),
    });

    expect(canResetTokenToSeed(theme, "app", "--card")).toBe(false);
  });

  it("allows isolated app tokens to reset their own value", () => {
    const isolated = new Set(["--card"]);
    const theme = makeUserTheme({
      appTokens: {
        ...BASE_APP_TOKENS.dark,
        "--card": "#333333",
      },
      seedAppTokens: {
        ...BASE_APP_TOKENS.dark,
        "--card": "#202020",
      },
      appIsolated: isolated,
      seedAppIsolated: isolated,
    });

    expect(canResetTokenToSeed(theme, "app", "--card")).toBe(true);
  });

  it("allows rows linked after a seed-isolated clone to reset their isolation", () => {
    const theme = makeUserTheme({
      appIsolated: new Set(),
      seedAppIsolated: new Set(["--card"]),
    });

    expect(canResetTokenToSeed(theme, "app", "--card")).toBe(true);
  });

  it("allows source rows to reset when the source changed", () => {
    const theme = makeUserTheme({
      sources: {
        ...DEFAULT_SOURCES_DARK,
        canvas: "#333333",
      },
      seedSources: {
        ...DEFAULT_SOURCES_DARK,
        canvas: "#101010",
      },
    });

    expect(canResetTokenToSeed(theme, "source", "canvas")).toBe(true);
  });
});

describe("mergeThemePatch", () => {
  it("preserves the original id even if the patch tries to spread one", () => {
    const current = makeUserTheme({ id: "user-1" });
    const next = mergeThemePatch(current, { displayName: "renamed" });
    expect(next.id).toBe("user-1");
    expect(next.displayName).toBe("renamed");
  });

  it("replaces eventPalette wholesale when the patch supplies one", () => {
    const current = makeUserTheme();
    const patched = [...current.eventPalette];
    patched[2] = "#ff0000";
    const next = mergeThemePatch(current, { eventPalette: patched });
    expect(next.eventPalette[2]).toBe("#ff0000");
    expect(next.eventPalette[12]).toBe(current.eventPalette[12]);
  });

  it("does not mutate the source palette", () => {
    const current = makeUserTheme();
    const before = current.eventPalette[2];
    const patched = [...current.eventPalette];
    patched[2] = "#ff0000";
    mergeThemePatch(current, { eventPalette: patched });
    expect(current.eventPalette[2]).toBe(before);
  });

  it("clones the patch palette so later mutations do not leak", () => {
    const current = makeUserTheme();
    const patched = [...current.eventPalette];
    patched[2] = "#ff0000";
    const next = mergeThemePatch(current, { eventPalette: patched });
    patched[2] = "#0000ff";
    expect(next.eventPalette[2]).toBe("#ff0000");
  });

  it("updates blendCanvas when the patch supplies one", () => {
    const current = makeUserTheme({ blendCanvas: "#101010" });
    const next = mergeThemePatch(current, { blendCanvas: "#abcdef" });
    expect(next.blendCanvas).toBe("#abcdef");
  });

  it("leaves blendCanvas alone when the patch omits it", () => {
    const current = makeUserTheme({ blendCanvas: "#101010" });
    const next = mergeThemePatch(current, { displayName: "renamed" });
    expect(next.blendCanvas).toBe("#101010");
  });
});

describe("nextUniqueDisplayName", () => {
  it("returns the base name when nothing collides", () => {
    expect(nextUniqueDisplayName("Solarized", [])).toBe("Solarized");
    expect(nextUniqueDisplayName("Solarized", ["Other"])).toBe("Solarized");
  });

  it("appends 2 on first collision", () => {
    expect(nextUniqueDisplayName("Solarized", ["Solarized"])).toBe("Solarized 2");
  });

  it("walks the suffix until a free slot is found", () => {
    expect(
      nextUniqueDisplayName("Theme", ["Theme", "Theme 2", "Theme 3"]),
    ).toBe("Theme 4");
  });

  it("compares case-insensitively", () => {
    expect(nextUniqueDisplayName("SOLARIZED", ["solarized"])).toBe(
      "SOLARIZED 2",
    );
  });

  it("accepts iterables, not just arrays", () => {
    const set = new Set(["Theme"]);
    expect(nextUniqueDisplayName("Theme", set)).toBe("Theme 2");
  });
});

describe("normalizeDisplayName", () => {
  it("trims surrounding whitespace", () => {
    expect(normalizeDisplayName("  hi  ")).toBe("hi");
  });

  it("returns undefined for empty or whitespace-only input", () => {
    expect(normalizeDisplayName("")).toBeUndefined();
    expect(normalizeDisplayName("   ")).toBeUndefined();
    expect(normalizeDisplayName("\t\n")).toBeUndefined();
  });

  it("caps length at 60 characters", () => {
    const long = "a".repeat(120);
    const result = normalizeDisplayName(long);
    expect(result).toBeDefined();
    expect(result!.length).toBe(60);
  });

  it("leaves short names untouched", () => {
    expect(normalizeDisplayName("Solarized")).toBe("Solarized");
  });
});
