// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { finishDesktopReadiness } from "./desktop-readiness";

/** Creates a readiness barrier controlled by the test. */
function deferred() {
  let resolve: () => void = () => undefined;
  let reject: (error: unknown) => void = () => undefined;
  const promise = new Promise<void>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

/** Installs the app root and returns readiness options with test-controlled barriers. */
function setup() {
  document.body.innerHTML = '<div id="app"></div>';
  const fonts = deferred();
  Object.defineProperty(document, "fonts", { configurable: true, value: { ready: fonts.promise } });
  const mounted = deferred();
  const flushed = deferred();
  const flushUpdates = vi.fn(() => flushed.promise);
  const revealMainWindow = vi.fn(async () => {
    expect(document.getElementById("app")?.textContent).not.toBe("");
  });
  const onError = vi.fn();
  const options = { document, mounted: mounted.promise, flushUpdates, revealMainWindow, onError };
  return { options, mounted, flushed, fonts };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

describe("desktop readiness", () => {
  it.each(["vault setup", "configured workspace"])(
    "reveals %s once after updates and fonts, without receiving animation frames",
    async (root) => {
      const frames = vi.fn(() => 1);
      vi.stubGlobal("requestAnimationFrame", frames);
      const { options, mounted, flushed, fonts } = setup();
      const ready = finishDesktopReadiness(options);
      expect(options.flushUpdates).not.toHaveBeenCalled();
      expect(options.revealMainWindow).not.toHaveBeenCalled();

      document.getElementById("app")!.textContent = root;
      mounted.resolve();
      await Promise.resolve();
      expect(options.flushUpdates).toHaveBeenCalledOnce();
      expect(options.revealMainWindow).not.toHaveBeenCalled();
      flushed.resolve();
      await Promise.resolve();
      expect(options.revealMainWindow).not.toHaveBeenCalled();

      fonts.resolve();
      await ready;
      expect(options.revealMainWindow).toHaveBeenCalledOnce();
      expect(options.onError).not.toHaveBeenCalled();
      expect(frames).not.toHaveBeenCalled();
    },
  );

  it("finishes detached and overlay roots without revealing the main window", async () => {
    const { options, mounted, flushed, fonts } = setup();
    const ready = finishDesktopReadiness({ ...options, revealMainWindow: null });
    mounted.resolve();
    flushed.resolve();
    fonts.resolve();
    await ready;
    expect(options.revealMainWindow).not.toHaveBeenCalled();
    expect(options.onError).not.toHaveBeenCalled();
  });

  it.each(["mount", "flush", "fonts"])("reports %s failure without signaling native readiness", async (stage) => {
    const { options, mounted, flushed, fonts } = setup();
    const error = new Error(`${stage} failed`);
    const ready = finishDesktopReadiness(options);
    if (stage === "mount") mounted.reject(error);
    else {
      mounted.resolve();
      await Promise.resolve();
      if (stage === "flush") flushed.reject(error);
      else {
        flushed.resolve();
        await Promise.resolve();
        fonts.reject(error);
      }
    }
    await expect(ready).resolves.toBeUndefined();
    expect(options.onError).toHaveBeenCalledExactlyOnceWith(error);
    expect(options.revealMainWindow).not.toHaveBeenCalled();
  });

  it("reports native reveal failure without retrying or hiding the error", async () => {
    const { options, mounted, flushed, fonts } = setup();
    const error = new Error("native reveal failed");
    options.revealMainWindow.mockRejectedValue(error);
    const ready = finishDesktopReadiness(options);
    mounted.resolve();
    flushed.resolve();
    fonts.resolve();
    await ready;
    expect(options.revealMainWindow).toHaveBeenCalledOnce();
    expect(options.onError).toHaveBeenCalledExactlyOnceWith(error);
  });
});
