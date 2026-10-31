import { describe, expect, it } from "vitest";
import { resolveActiveThemeId, resolveQuickToggleThemeId, type ActiveThemeInputs } from "./active-theme";
import { BUILTIN_THEME_REGISTRY, darkTheme, lightTheme, type Theme, type ThemeId } from "./definitions";

const dusk: Theme = { ...darkTheme, id: "dusk", displayName: "Dusk" };
const dawn: Theme = { ...lightTheme, id: "dawn", displayName: "Dawn" };
const registry: Readonly<Record<ThemeId, Theme>> = { ...BUILTIN_THEME_REGISTRY, dusk, dawn };

function inputs(overrides: Partial<ActiveThemeInputs> = {}): ActiveThemeInputs {
  return {
    activeId: undefined,
    quickToggleLightId: "dawn",
    quickToggleDarkId: "dusk",
    prefersDark: false,
    registry,
    ...overrides,
  };
}

describe("resolveQuickToggleThemeId", () => {
  it("keeps a choice that names an existing theme", () => {
    expect(resolveQuickToggleThemeId("dusk", "dark", registry)).toBe("dusk");
    expect(resolveQuickToggleThemeId("dawn", "light", registry)).toBe("dawn");
  });

  it("falls back to the built-in theme of the slot when the choice is missing or deleted", () => {
    expect(resolveQuickToggleThemeId(undefined, "dark", registry)).toBe(darkTheme.id);
    expect(resolveQuickToggleThemeId("deleted", "dark", registry)).toBe(darkTheme.id);
    expect(resolveQuickToggleThemeId("deleted", "light", registry)).toBe(lightTheme.id);
  });

  it("does not treat inherited object keys as theme ids", () => {
    expect(resolveQuickToggleThemeId("toString", "light", registry)).toBe(lightTheme.id);
  });
});

describe("resolveActiveThemeId", () => {
  it("keeps the device's choice while its theme exists, whatever the system scheme", () => {
    expect(resolveActiveThemeId(inputs({ activeId: "dawn", prefersDark: true }))).toBe("dawn");
    expect(resolveActiveThemeId(inputs({ activeId: darkTheme.id }))).toBe(darkTheme.id);
  });

  it("uses the quick-toggle theme matching the system scheme when the device has no choice", () => {
    expect(resolveActiveThemeId(inputs({ prefersDark: false }))).toBe("dawn");
    expect(resolveActiveThemeId(inputs({ prefersDark: true }))).toBe("dusk");
  });

  it("falls back to the quick-toggle theme when another device deleted the active theme", () => {
    expect(resolveActiveThemeId(inputs({ activeId: "deleted", prefersDark: true }))).toBe("dusk");
  });

  it("falls back to the built-in theme when the matching quick-toggle theme was deleted too", () => {
    const withoutDusk: Readonly<Record<ThemeId, Theme>> = { ...BUILTIN_THEME_REGISTRY, dawn };
    expect(resolveActiveThemeId(inputs({ activeId: "dusk", prefersDark: true, registry: withoutDusk }))).toBe(
      darkTheme.id,
    );
    expect(resolveActiveThemeId(inputs({ activeId: "dusk", prefersDark: false, registry: withoutDusk }))).toBe(
      "dawn",
    );
  });
});
