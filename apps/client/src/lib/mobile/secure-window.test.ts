import { afterEach, describe, expect, it, vi } from "vitest";
import { createSecureWindow, type SecureWindowSetter } from "./secure-window";

function recordingSetter(): { calls: boolean[]; setSecure: SecureWindowSetter } {
  const calls: boolean[] = [];
  return {
    calls,
    setSecure: (secure) => {
      calls.push(secure);
      return Promise.resolve();
    },
  };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("createSecureWindow", () => {
  it("keeps the window secure until the last holder releases", async () => {
    const { calls, setSecure } = recordingSetter();
    const acquire = createSecureWindow(setSecure, true);

    const releaseSheet = await acquire();
    expect(calls).toEqual([true]);
    const releasePrompt = await acquire();
    expect(calls).toEqual([true]);

    await releaseSheet();
    expect(calls).toEqual([true]);
    await releaseSheet();
    expect(calls).toEqual([true]);

    await releasePrompt();
    expect(calls).toEqual([true, false]);
  });

  it("ends secure when a release and an acquire overlap", async () => {
    let finishClear: () => void = () => undefined;
    const calls: boolean[] = [];
    const setSecure: SecureWindowSetter = (secure) => {
      calls.push(secure);
      if (secure) return Promise.resolve();
      return new Promise((resolve) => {
        finishClear = resolve;
      });
    };
    const acquire = createSecureWindow(setSecure, true);

    const releaseSheet = await acquire();
    const released = releaseSheet();
    await vi.waitFor(() => expect(calls).toEqual([true, false]));

    const prompt = acquire();
    finishClear();
    await released;
    const releasePrompt = await prompt;
    expect(calls.at(-1)).toBe(true);

    const releasedPrompt = releasePrompt();
    await vi.waitFor(() => expect(calls.at(-1)).toBe(false));
    finishClear();
    await releasedPrompt;
  });

  it("resolves and warns when the platform refuses", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const calls: boolean[] = [];
    const acquire = createSecureWindow((secure) => {
      calls.push(secure);
      return Promise.reject(new Error("the window is gone"));
    }, true);

    const release = await acquire();
    expect(calls).toEqual([true]);
    expect(warn).toHaveBeenCalledTimes(1);

    await expect(release()).resolves.toBeUndefined();
  });

  it("leaves the window alone off Android", async () => {
    const { calls, setSecure } = recordingSetter();
    const acquire = createSecureWindow(setSecure, false);

    const release = await acquire();
    await release();

    expect(calls).toEqual([]);
  });
});
