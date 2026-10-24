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

  it("restores the saved values of a refused write and tells subscribers", async () => {
    const saved = JSON.stringify({ theme: { activeId: "light" }, preferences: { fontScale: 1.1 } });
    invokeMock.mockImplementation(((cmd: string) => {
      if (cmd === "vault_read_config") return Promise.resolve(saved);
      if (cmd === "vault_patch_config") return Promise.reject(new Error("preference path is not registered"));
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    }) as unknown as Mock);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { ensureConfigLoaded, flushConfig, getConfigKey, onConfigChangedElsewhere, setConfigKey } =
      await loadModule();
    try {
      await ensureConfigLoaded();
      const listener = vi.fn();
      onConfigChangedElsewhere(listener);
      setConfigKey("theme.activeId", "dark");
      setConfigKey("preferences.fontScale", undefined);
      setConfigKey("unknown.key", true);

      await flushConfig();

      expect(getConfigKey("theme.activeId", "")).toBe("light");
      expect(getConfigKey("preferences.fontScale", 1)).toBe(1.1);
      expect(getConfigKey("unknown.key", null)).toBeNull();
      expect(listener).toHaveBeenCalledWith(
        new Set(["theme.activeId", "preferences.fontScale", "unknown.key"]),
      );
      const writes = invokeMock.mock.calls.filter(([cmd]) => cmd === "vault_patch_config");
      expect(writes).toHaveLength(1);
      expect(errorSpy.mock.calls[0]?.[1]).toEqual(["theme.activeId", "preferences.fontScale", "unknown.key"]);
    } finally {
      errorSpy.mockRestore();
    }
  });

  it("keeps a newer local edit when an earlier write is refused", async () => {
    let rejectWrite: (error: Error) => void = () => undefined;
    let writes = 0;
    invokeMock.mockImplementation(((cmd: string) => {
      if (cmd === "vault_read_config") return Promise.resolve(JSON.stringify({ theme: { activeId: "light" } }));
      if (cmd === "vault_patch_config") {
        writes += 1;
        if (writes > 1) return Promise.resolve();
        return new Promise<void>((_resolve, reject) => {
          rejectWrite = reject;
        });
      }
      return Promise.reject(new Error(`unexpected command: ${cmd}`));
    }) as unknown as Mock);
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { ensureConfigLoaded, flushConfig, getConfigKey, setConfigKey } = await loadModule();
    try {
      await ensureConfigLoaded();
      setConfigKey("theme.activeId", "dark");
      const first = flushConfig();
      await vi.waitFor(() => {
        expect(invokeMock.mock.calls.some(([cmd]) => cmd === "vault_patch_config")).toBe(true);
      });
      setConfigKey("theme.activeId", "midnight");
      rejectWrite(new Error("disk full"));
      await first;

      expect(getConfigKey("theme.activeId", "")).toBe("midnight");
      expect(writes).toBe(2);
    } finally {
      errorSpy.mockRestore();
    }
  });
});

describe("refreshConfigKeysFromDisk", () => {
  it("copies only the received keys into the cache and tells subscribers", async () => {
    setReadResponse(JSON.stringify({ notes: { favorites: ["a"] }, theme: { activeId: "light" } }));
    const { ensureConfigLoaded, getConfigKey, onConfigChangedElsewhere, refreshConfigKeysFromDisk } = await loadModule();
    await ensureConfigLoaded();
    const listener = vi.fn();
    onConfigChangedElsewhere(listener);
    setReadResponse(JSON.stringify({ notes: { favorites: ["a", "b"] }, theme: { activeId: "dark" } }));

    await refreshConfigKeysFromDisk(["notes.favorites"]);

    expect(getConfigKey("notes.favorites", [])).toEqual(["a", "b"]);
    expect(getConfigKey("theme.activeId", "")).toBe("light");
    expect(listener).toHaveBeenCalledWith(new Set(["notes.favorites"]));
  });

  it("removes a received key that another window deleted", async () => {
    setReadResponse(JSON.stringify({ projects: { starredProjectIds: ["p"] } }));
    const { ensureConfigLoaded, getConfigKey, refreshConfigKeysFromDisk } = await loadModule();
    await ensureConfigLoaded();
    setReadResponse(JSON.stringify({ projects: {} }));

    await refreshConfigKeysFromDisk(["projects.starredProjectIds"]);

    expect(getConfigKey("projects.starredProjectIds", null)).toBeNull();
  });

  it("keeps this window's unsaved value for a key or branch it is still writing", async () => {
    vi.useFakeTimers();
    setReadResponse(JSON.stringify({ chatSidebar: { orderByProject: { p: "name" } } }));
    const { ensureConfigLoaded, getConfigKey, onConfigChangedElsewhere, refreshConfigKeysFromDisk, setConfigKey } = await loadModule();
    await ensureConfigLoaded();
    const listener = vi.fn();
    onConfigChangedElsewhere(listener);
    setConfigKey("chatSidebar.orderByProject", { p: "created" });
    setReadResponse(JSON.stringify({ chatSidebar: { orderByProject: { p: "modified" }, starredChannelIds: ["c"] } }));

    await refreshConfigKeysFromDisk(["chatSidebar.orderByProject.p", "chatSidebar.starredChannelIds"]);

    expect(getConfigKey("chatSidebar.orderByProject", {})).toEqual({ p: "created" });
    expect(getConfigKey("chatSidebar.starredChannelIds", [])).toEqual(["c"]);
    expect(listener).toHaveBeenCalledWith(new Set(["chatSidebar.starredChannelIds"]));
  });

  it("does not write, and stays quiet when nothing applied", async () => {
    const { ensureConfigLoaded, onConfigChangedElsewhere, refreshConfigKeysFromDisk } = await loadModule();
    await ensureConfigLoaded();
    const listener = vi.fn();
    onConfigChangedElsewhere(listener);

    await refreshConfigKeysFromDisk(["", "."]);

    expect(listener).not.toHaveBeenCalled();
    expect(invokeMock.mock.calls.some(([cmd]) => cmd === "vault_patch_config")).toBe(false);
  });

  it("leaves the cache alone when the file is no longer a JSON object", async () => {
    setReadResponse(JSON.stringify({ chat: { enabled: true } }));
    const { ensureConfigLoaded, getConfigKey, refreshConfigKeysFromDisk } = await loadModule();
    await ensureConfigLoaded();
    setReadResponse("not json");

    await expect(refreshConfigKeysFromDisk(["chat"])).rejects.toThrow(
      "vault configuration is not a JSON object",
    );

    expect(getConfigKey("chat", null)).toEqual({ enabled: true });
  });
});

describe("changedConfigKeys", () => {
  it("names only the keys whose values differ, member by member inside shared objects", async () => {
    const { changedConfigKeys } = await loadModule();
    const previous = {
      profile: { displayName: "Ana", image: { path: "profile/a.png", byteSize: 10 } },
      notes: { favorites: ["a"] },
      theme: { activeId: "light" },
      removed: { inner: 1 },
    };
    const next = {
      profile: { displayName: "Ana", image: { path: "profile/b.png", byteSize: 10 } },
      notes: { favorites: ["a", "b"] },
      theme: { activeId: "light" },
      added: true,
    };

    expect(changedConfigKeys(previous, next).sort()).toEqual(
      ["added", "notes.favorites", "profile.image.path", "removed"].sort(),
    );
  });

  it("compares a value that changes between an object and another shape whole", async () => {
    const { changedConfigKeys } = await loadModule();

    expect(changedConfigKeys({ a: { b: 1 } }, { a: [1] })).toEqual(["a"]);
    expect(changedConfigKeys({ a: null }, { a: { b: 1 } })).toEqual(["a"]);
    expect(changedConfigKeys({ a: { b: [1, 2] } }, { a: { b: [1, 2] } })).toEqual([]);
  });
});

describe("refreshConfigFromSync", () => {
  it("copies replicated changes into the cache and tells subscribers which keys changed", async () => {
    setReadResponse(JSON.stringify({ profile: { displayName: "Ana" }, preferences: { language: "en" } }));
    const { ensureConfigLoaded, getConfigKey, onConfigChangedElsewhere, refreshConfigFromSync } = await loadModule();
    await ensureConfigLoaded();
    const listener = vi.fn();
    onConfigChangedElsewhere(listener);
    setReadResponse(JSON.stringify({ profile: { displayName: "Bea" }, preferences: { language: "en" } }));

    await refreshConfigFromSync();

    expect(getConfigKey("profile.displayName", "")).toBe("Bea");
    expect(listener).toHaveBeenCalledWith(new Set(["profile.displayName"]));
  });

  it("keeps keys this window is still writing and stays quiet when nothing else changed", async () => {
    vi.useFakeTimers();
    setReadResponse(JSON.stringify({ profile: { displayName: "Ana" } }));
    const { ensureConfigLoaded, getConfigKey, onConfigChangedElsewhere, refreshConfigFromSync, setConfigKey } = await loadModule();
    await ensureConfigLoaded();
    const listener = vi.fn();
    onConfigChangedElsewhere(listener);
    setConfigKey("profile.displayName", "Local");
    setReadResponse(JSON.stringify({ profile: { displayName: "Remote" } }));

    await refreshConfigFromSync();

    expect(getConfigKey("profile.displayName", "")).toBe("Local");
    expect(listener).not.toHaveBeenCalled();
    expect(invokeMock.mock.calls.some(([cmd]) => cmd === "vault_patch_config")).toBe(false);
  });
});

describe("startConfigWindowSync", () => {
  it("refreshes from the view only when sync applied preference rows", async () => {
    let onApplied: ((tables: string[]) => void) | undefined;
    const listenSyncApplied = (listener: (tables: string[]) => void) => {
      onApplied = listener;
      return Promise.resolve(() => undefined);
    };
    vi.doMock("$lib/windows/vault-change-channel", () => ({
      createVaultChangeChannel: () => ({
        publish: () => undefined,
        listen: () => Promise.resolve(() => undefined),
      }),
    }));
    setReadResponse(JSON.stringify({ theme: { quickToggleDarkId: "a" } }));
    const { ensureConfigLoaded, getConfigKey, onConfigChangedElsewhere, startConfigWindowSync, PREFERENCES_SYNC_TABLE } =
      await loadModule();
    try {
      await ensureConfigLoaded();
      const listener = vi.fn();
      onConfigChangedElsewhere(listener);
      startConfigWindowSync(listenSyncApplied);
      startConfigWindowSync(listenSyncApplied);
      setReadResponse(JSON.stringify({ theme: { quickToggleDarkId: "b" } }));

      onApplied?.(["quick_notes"]);
      await Promise.resolve();
      expect(invokeMock.mock.calls.filter(([cmd]) => cmd === "vault_read_config")).toHaveLength(1);
      onApplied?.([PREFERENCES_SYNC_TABLE]);
      await vi.waitFor(() => expect(listener).toHaveBeenCalledWith(new Set(["theme.quickToggleDarkId"])));

      expect(getConfigKey("theme.quickToggleDarkId", "")).toBe("b");
      expect(invokeMock.mock.calls.filter(([cmd]) => cmd === "vault_read_config")).toHaveLength(2);
    } finally {
      vi.doUnmock("$lib/windows/vault-change-channel");
    }
  });
});

describe("configKeyChanged", () => {
  it("matches the key itself and its parent or child branches only", async () => {
    const { configKeyChanged } = await loadModule();

    expect(configKeyChanged(new Set(["preferences.fontScale"]), "preferences")).toBe(true);
    expect(configKeyChanged(new Set(["preferences"]), "preferences.fontScale")).toBe(true);
    expect(configKeyChanged(new Set(["preferences.fontScale"]), "preferences.fontScale")).toBe(true);
    expect(configKeyChanged(new Set(["preferencesExtra.value"]), "preferences")).toBe(false);
    expect(configKeyChanged(new Set(["profile.color"]), "preferences")).toBe(false);
  });
});
