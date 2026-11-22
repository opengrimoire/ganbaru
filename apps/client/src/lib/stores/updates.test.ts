import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  Channel: class {
    constructor(public onmessage: (message: unknown) => void) {}
  },
}));
vi.mock("$lib/vault/config", () => ({ getConfigKey: (_key: string, fallback: unknown) => fallback, setConfigKey: vi.fn() }));

/** Creates a native request the test settles by hand. */
function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}

/** Loads a fresh update manager whose native commands answer through `handlers`. */
async function updateManager(handlers: Record<string, () => Promise<unknown>>) {
  tauri.invoke.mockImplementation(async (command) => {
    const handler = handlers[command];
    if (!handler) throw new Error(`Unexpected command ${command}`);
    return handler();
  });
  const { getUpdateManager } = await import("./updates.svelte");
  return getUpdateManager();
}

const selfUpdater = async () => ({ selfUpdater: true, packageManager: null, copyCommand: null, releasePageUrl: null });
const offered = async () => ({ version: "0.3.0", currentVersion: "0.2.0", body: null });

describe("update manager install", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.stubGlobal("__GANBARU_BUILD_REF__", "0.2.0+abc123");
    vi.stubGlobal("__GANBARU_GITHUB_REPOSITORY__", "opengrimoire/ganbaru");
  });

  it("starts one install when Install is pressed twice", async () => {
    const install = deferred<undefined>();
    const updates = await updateManager({
      updater_install_context: selfUpdater,
      updates_check: offered,
      updates_install: () => install.promise,
      restart_app: async () => undefined,
    });
    await updates.checkForUpdates();
    expect(updates.status).toBe("available");

    const first = updates.installUpdate();
    const second = updates.installUpdate();
    expect(updates.status).toBe("downloading");
    install.resolve(undefined);
    await Promise.all([first, second]);

    expect(tauri.invoke.mock.calls.filter(([command]) => command === "updates_install")).toHaveLength(1);
    expect(updates.status).toBe("installed");
  });

  it("keeps the update offered when this install cannot update itself", async () => {
    const updates = await updateManager({
      updater_install_context: async () => ({ selfUpdater: false, packageManager: null, copyCommand: null, releasePageUrl: null }),
      updates_check: offered,
    });
    await updates.checkForUpdates();

    await updates.installUpdate();

    expect(updates.status).toBe("available");
    expect(tauri.invoke.mock.calls.some(([command]) => command === "updates_install")).toBe(false);
  });
});
