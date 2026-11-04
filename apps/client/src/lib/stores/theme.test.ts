import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UserThemeRead } from "../api/themes";

const { getConfigKey, listenSyncApplied, loadAllUserThemes, loadDismissals, setConfigKey } = vi.hoisted(() => ({
  getConfigKey: vi.fn(),
  listenSyncApplied: vi.fn(),
  loadAllUserThemes: vi.fn(),
  loadDismissals: vi.fn(),
  setConfigKey: vi.fn(),
}));

vi.mock("../api/sync", () => ({ listenSyncApplied }));
vi.mock("../api/themes", () => ({ loadAllUserThemes, loadDismissals }));
vi.mock("../vault/config", () => ({ getConfigKey, setConfigKey }));
vi.mock("$lib/windows/sync-transport", () => ({
  emitWindowSync: vi.fn(async () => undefined),
  listenWindowSync: vi.fn(async () => () => undefined),
}));

const SAVED_THEME_ID = "harbor-night";

/** A stored user theme with no token rows, so every token takes its base fallback. */
function storedTheme(id: string): UserThemeRead {
  return {
    theme: {
      id,
      display_name: "Harbor night",
      blend_canvas: "#101820",
      seed_blend_canvas: "#101820",
      derivation_engine_version: 1,
      calendar_default_mode: "app-canvas",
      calendar_default_custom: "#101820",
      seed_calendar_default_mode: "app-canvas",
      seed_calendar_default_custom: "#101820",
      icon_label: "dark",
      seed_icon_label: "dark",
      created_at_ms: 0,
      updated_at_ms: 0,
      has_conflict: false,
    },
    tokens: [],
    palette: [],
    seedTokens: [],
    seedPalette: [],
  };
}

/** Config where the device has saved `activeId` and no quick-toggle pair. */
function savedSelection(activeId: string): void {
  getConfigKey.mockImplementation((key: string, fallback: unknown) => (key === "theme.activeId" ? activeId : fallback));
}

describe("hydrateUserThemes", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    getConfigKey.mockImplementation((_key: string, fallback: unknown) => fallback);
    listenSyncApplied.mockResolvedValue(() => undefined);
    loadDismissals.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("finishes on the built-in themes without writing the saved selection when the stored themes fail to load", async () => {
    savedSelection(SAVED_THEME_ID);
    loadAllUserThemes.mockRejectedValue(new Error("load themes: database is locked"));
    const { getTheme, hydrateUserThemes } = await import("./theme.svelte");

    await expect(hydrateUserThemes()).resolves.toBeUndefined();

    const theme = getTheme();
    expect(theme.customThemes).toEqual({});
    expect(theme.registry[theme.id]).toBeDefined();
    expect(theme.id).not.toBe(SAVED_THEME_ID);
    expect(listenSyncApplied).toHaveBeenCalledOnce();
    expect(setConfigKey).not.toHaveBeenCalled();
  });

  it("restores the saved theme once a later load succeeds after a failed boot load", async () => {
    savedSelection(SAVED_THEME_ID);
    loadAllUserThemes
      .mockRejectedValueOnce(new Error("load themes: database is locked"))
      .mockResolvedValue([storedTheme(SAVED_THEME_ID)]);
    const { getTheme, hydrateUserThemes } = await import("./theme.svelte");

    await hydrateUserThemes();
    await vi.runAllTimersAsync();

    const theme = getTheme();
    expect(Object.keys(theme.customThemes)).toEqual([SAVED_THEME_ID]);
    expect(theme.id).toBe(SAVED_THEME_ID);
    expect(setConfigKey).not.toHaveBeenCalled();
  });

  it("stops retrying after a bounded number of failed loads", async () => {
    savedSelection(SAVED_THEME_ID);
    loadAllUserThemes.mockRejectedValue(new Error("load themes: database is locked"));
    const { getTheme, hydrateUserThemes } = await import("./theme.svelte");

    await hydrateUserThemes();
    await vi.runAllTimersAsync();

    expect(loadAllUserThemes.mock.calls.length).toBeGreaterThan(1);
    expect(vi.getTimerCount()).toBe(0);
    expect(getTheme().id).not.toBe(SAVED_THEME_ID);
    expect(setConfigKey).not.toHaveBeenCalled();
  });
});
