import { describe, expect, it, vi } from "vitest";
import { flushBeforeWindowClose, registerWindowCloseFlusher } from "./close-flush";

describe("window close flush", () => {
  it("runs every registered save and stops after unregistration", async () => {
    const first = vi.fn(async () => undefined);
    const second = vi.fn(async () => undefined);
    const unregisterFirst = registerWindowCloseFlusher(first);
    const unregisterSecond = registerWindowCloseFlusher(second);

    await flushBeforeWindowClose();
    unregisterFirst();
    await flushBeforeWindowClose();
    unregisterSecond();

    expect(first).toHaveBeenCalledOnce();
    expect(second).toHaveBeenCalledTimes(2);
  });

  it("finishes the other saves before reporting a failed one", async () => {
    const failure = new Error("disk full");
    const saved = vi.fn(async () => undefined);
    const unregisterFailing = registerWindowCloseFlusher(async () => { throw failure; });
    const unregisterSaved = registerWindowCloseFlusher(saved);

    await expect(flushBeforeWindowClose()).rejects.toBe(failure);
    unregisterFailing();
    unregisterSaved();

    expect(saved).toHaveBeenCalledOnce();
  });
});
