import { describe, expect, it, vi } from "vitest";
import {
  emitWindowSync,
  listenWindowSync,
} from "./sync-transport.mobile";

describe("mobile window synchronization transport", () => {
  it("completes publishes without dispatching cross-window work", async () => {
    await expect(
      emitWindowSync("calendar-window-sync", { kind: "data-changed" }),
    ).resolves.toBeUndefined();
  });

  it("returns a safe unlisten function without invoking the listener", async () => {
    const listener = vi.fn();
    const unlisten = await listenWindowSync("theme-window-sync", listener);

    expect(listener).not.toHaveBeenCalled();
    expect(unlisten()).toBeUndefined();
  });
});
