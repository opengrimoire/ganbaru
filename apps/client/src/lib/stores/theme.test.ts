import { beforeEach, describe, expect, it, vi } from "vitest";

const { listenSyncApplied, loadAllUserThemes, loadDismissals } = vi.hoisted(() => ({
  listenSyncApplied: vi.fn(),
  loadAllUserThemes: vi.fn(),
  loadDismissals: vi.fn(),
}));

vi.mock("../api/sync", () => ({ listenSyncApplied }));
vi.mock("../api/themes", () => ({ loadAllUserThemes, loadDismissals }));
vi.mock("../vault/config", () => ({ getConfigKey: vi.fn(), setConfigKey: vi.fn() }));
vi.mock("$lib/windows/sync-transport", () => ({
  emitWindowSync: vi.fn(async () => undefined),
  listenWindowSync: vi.fn(async () => () => undefined),
}));

describe("hydrateUserThemes", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    listenSyncApplied.mockResolvedValue(() => undefined);
    loadDismissals.mockResolvedValue([]);
  });

  it("finishes on the built-in themes when the stored themes fail to load", async () => {
    loadAllUserThemes.mockRejectedValue(new Error("load themes: database is locked"));
    const { getTheme, hydrateUserThemes } = await import("./theme.svelte");

    await expect(hydrateUserThemes()).resolves.toBeUndefined();

    const theme = getTheme();
    expect(theme.customThemes).toEqual({});
    expect(theme.registry[theme.id]).toBeDefined();
    expect(listenSyncApplied).toHaveBeenCalledOnce();
  });
});
