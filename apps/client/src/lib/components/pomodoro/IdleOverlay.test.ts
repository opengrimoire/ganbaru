// @vitest-environment jsdom

import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

const appWindow = vi.hoisted(() => ({
  isFullscreen: vi.fn(async () => false),
  isVisible: vi.fn(async () => true),
  setAlwaysOnTop: vi.fn(async (_onTop: boolean) => undefined),
  setFullscreen: vi.fn(async (_fullscreen: boolean) => undefined),
  setFocus: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => appWindow,
}));

describe("IdleOverlay", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.clearAllMocks();
  });

  async function renderTakenOverlay(onResume: () => Promise<void> = async () => undefined): Promise<void> {
    target = document.createElement("div");
    document.body.append(target);
    const { default: IdleOverlay } = await import("./IdleOverlay.svelte");
    component = mount(IdleOverlay, {
      target,
      props: { idleSeconds: 30, onResume, onVisible: async () => undefined },
    });
    await vi.waitFor(() => expect(appWindow.setFocus).toHaveBeenCalledOnce());
  }

  it("hands the window back when the overlay closes without a resume from it", async () => {
    await renderTakenOverlay();
    expect(appWindow.setFullscreen).toHaveBeenCalledWith(true);
    expect(appWindow.setAlwaysOnTop).toHaveBeenCalledWith(true);

    if (component) await unmount(component);
    component = null;

    await vi.waitFor(() => expect(appWindow.setAlwaysOnTop).toHaveBeenLastCalledWith(false));
    expect(appWindow.setFullscreen).toHaveBeenLastCalledWith(false);
  });

  it("hands the window back once when a resume is followed by the overlay closing", async () => {
    await renderTakenOverlay();
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "Space" }));
    await vi.waitFor(() => expect(appWindow.setAlwaysOnTop).toHaveBeenLastCalledWith(false));

    if (component) await unmount(component);
    component = null;
    await Promise.resolve();
    await Promise.resolve();

    expect(appWindow.setFullscreen.mock.calls.filter(([fullscreen]) => !fullscreen)).toHaveLength(1);
    expect(appWindow.setAlwaysOnTop.mock.calls.filter(([onTop]) => !onTop)).toHaveLength(1);
  });
});
