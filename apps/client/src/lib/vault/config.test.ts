import {
  describe,
  it,
  expect,
  beforeEach,
  afterEach,
  vi,
  type Mock,
} from "vitest";

// Each test imports a fresh module so the cache and load promise reset
// between cases.
async function loadModule() {
  return await import("./config");
}

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...(args as [string, unknown])),
}));

function setReadResponse(json: string) {
  invokeMock.mockImplementation(((cmd: string) => {
    if (cmd === "vault_read_config") return Promise.resolve(json);
    if (cmd === "vault_patch_config") return Promise.resolve();
    return Promise.reject(new Error(`unexpected command: ${cmd}`));
  }) as unknown as Mock);
}

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
  setReadResponse("{}");
});

afterEach(() => {
  vi.useRealTimers();
});

describe("ensureConfigLoaded", () => {
  it("invokes vault_read_config exactly once across repeat calls", async () => {
    const { ensureConfigLoaded } = await loadModule();
    await Promise.all([
      ensureConfigLoaded(),
      ensureConfigLoaded(),
      ensureConfigLoaded(),
    ]);
    const reads = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "vault_read_config",
    );
    expect(reads.length).toBe(1);
  });

  it("rejects backend read failures so startup can show a setup error", async () => {
    invokeMock.mockImplementation(((cmd: string) => {
      if (cmd === "vault_read_config") return Promise.reject(new Error("permission denied"));
      if (cmd === "vault_patch_config") return Promise.resolve();
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    }) as unknown as Mock);
    const { ensureConfigLoaded } = await loadModule();

    await expect(ensureConfigLoaded()).rejects.toThrow("permission denied");
  });

  it("treats malformed JSON from the backend as an empty config", async () => {
    setReadResponse("not json");
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { ensureConfigLoaded, getConfigKey } = await loadModule();
    try {
      await ensureConfigLoaded();
      expect(getConfigKey("anything", "fallback")).toBe("fallback");
      expect(errorSpy).toHaveBeenCalledTimes(1);
      expect(errorSpy.mock.calls[0]?.[0]).toBe("vault_read_config failed");
    } finally {
      errorSpy.mockRestore();
    }
  });

  it("treats a non-object root as an empty config", async () => {
    setReadResponse("[1, 2, 3]");
    const { ensureConfigLoaded, getConfigKey } = await loadModule();
    await ensureConfigLoaded();
    expect(getConfigKey("anything", "fallback")).toBe("fallback");
  });
});

describe("getConfigKey", () => {
  it("returns the fallback when the dotted path is absent", async () => {
    setReadResponse(JSON.stringify({ theme: { activeId: "dark" } }));
    const { ensureConfigLoaded, getConfigKey } = await loadModule();
    await ensureConfigLoaded();
    expect(getConfigKey("preferences.fontScale", 1)).toBe(1);
  });

  it("walks dotted paths into nested objects", async () => {
    setReadResponse(
      JSON.stringify({
        theme: { activeId: "midnight" },
        preferences: { fontScale: 1.1 },
      }),
    );
    const { ensureConfigLoaded, getConfigKey } = await loadModule();
    await ensureConfigLoaded();
    expect(getConfigKey("theme.activeId", "dark")).toBe("midnight");
    expect(getConfigKey("preferences.fontScale", 1)).toBe(1.1);
  });

  it("ignores prototype-chain keys", async () => {
    setReadResponse("{}");
    const { ensureConfigLoaded, getConfigKey } = await loadModule();
    await ensureConfigLoaded();
    // toString lives on Object.prototype but readPath uses Object.hasOwn.
    expect(getConfigKey("toString", "fallback")).toBe("fallback");
  });
});

describe("setConfigKey", () => {
  it("stores values under nested dotted paths", async () => {
    vi.useFakeTimers();
    const { ensureConfigLoaded, setConfigKey, flushConfig, getConfigKey } =
      await loadModule();
    await ensureConfigLoaded();

    setConfigKey("theme.activeId", "midnight");
    expect(getConfigKey("theme.activeId", "")).toBe("midnight");
    await vi.runAllTimersAsync();
    await flushConfig();
    const writes = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "vault_patch_config",
    );
    expect(writes.length).toBeGreaterThan(0);
    const payload = writes[writes.length - 1][1] as { patches: Array<{
      path: string[];
      remove: boolean;
      value: unknown;
    }> };
    expect(payload.patches).toEqual([{
      path: ["theme", "activeId"],
      remove: false,
      value: "midnight",
    }]);
  });

  it("debounces a burst of edits into a single write", async () => {
    vi.useFakeTimers();
    const { ensureConfigLoaded, setConfigKey, flushConfig } = await loadModule();
    await ensureConfigLoaded();

    setConfigKey("theme.activeId", "a");
    setConfigKey("theme.activeId", "b");
    setConfigKey("theme.activeId", "c");

    await vi.runAllTimersAsync();
    await flushConfig();

    const writes = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "vault_patch_config",
    );
    expect(writes.length).toBe(1);
    const payload = writes[0][1] as { patches: Array<{ value: unknown }> };
    expect(payload.patches).toHaveLength(1);
    expect(payload.patches[0]?.value).toBe("c");
  });

  it("removes a key when set to undefined", async () => {
    vi.useFakeTimers();
    setReadResponse(
      JSON.stringify({ preferences: { fontFamilyId: "inter" } }),
    );
    const { ensureConfigLoaded, setConfigKey, flushConfig, getConfigKey } =
      await loadModule();
    await ensureConfigLoaded();
    setConfigKey("preferences.fontFamilyId", undefined);
    await vi.runAllTimersAsync();
    await flushConfig();
    expect(getConfigKey("preferences.fontFamilyId", "missing")).toBe("missing");
    const writes = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "vault_patch_config",
    );
    expect(writes[0]?.[1]).toEqual({
      patches: [{
        path: ["preferences", "fontFamilyId"],
        remove: true,
        value: null,
      }],
    });
  });
});

describe("flushConfig", () => {
  it("forces a pending debounced write to flush early", async () => {
    vi.useFakeTimers();
    const { ensureConfigLoaded, setConfigKey, flushConfig } = await loadModule();
    await ensureConfigLoaded();

    setConfigKey("theme.activeId", "a");
    // Do not advance timers; flushConfig should bypass the debounce.
    await flushConfig();
    const writes = invokeMock.mock.calls.filter(
      ([cmd]) => cmd === "vault_patch_config",
    );
    expect(writes.length).toBe(1);
  });
});
