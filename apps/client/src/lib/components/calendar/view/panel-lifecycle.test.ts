import { describe, expect, it, vi } from "vitest";
import { CalendarViewPanelLifecycle } from "./panel-lifecycle.svelte";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("CalendarViewPanelLifecycle", () => {
  it("deduplicates lazy loading and suppresses stale request marks", async () => {
    const panel = deferred<{ default: never }>();
    const mark = vi.fn();
    const lifecycle = new CalendarViewPanelLifecycle({
      importPanel: () => panel.promise,
      mark,
      afterRender: (callback) => callback(),
      afterPaint: (callback) => callback(),
    });
    const first = lifecycle.beginOpen("edit", "open");
    const firstReady = lifecycle.ensureReady(first);
    const second = lifecycle.beginOpen("create", "switch");
    const secondReady = lifecycle.ensureReady(second);
    panel.resolve({ default: (() => undefined) as never });
    await Promise.all([firstReady, secondReady]);

    expect(mark).not.toHaveBeenCalledWith("panel.module-ready", { request: first });
    expect(mark).toHaveBeenCalledWith("panel.module-ready", { request: second });
  });

  it("drops stale paint completions after close", () => {
    const renders: Array<() => void> = [];
    const mark = vi.fn();
    const lifecycle = new CalendarViewPanelLifecycle({
      importPanel: () => Promise.reject(new Error("unused")),
      mark,
      afterRender: (callback) => { renders.push(callback); },
      afterPaint: (callback) => callback(),
    });
    const request = lifecycle.beginOpen("create", "open");
    lifecycle.markPaintDone(request);
    lifecycle.invalidate();
    renders[0]?.();

    expect(mark).not.toHaveBeenCalledWith("panel.flush-done", { request });
  });

  it("releases the current interaction after a panel load failure", () => {
    const lifecycle = new CalendarViewPanelLifecycle({
      importPanel: () => Promise.reject(new Error("failed")),
      mark: vi.fn(),
      afterRender: (callback) => callback(),
      afterPaint: (callback) => callback(),
    });
    const failedRequest = lifecycle.beginOpen("create", "open");
    lifecycle.pendingEditEventId = "pending";
    lifecycle.setSurfaceStatus("tentative", "pending");

    expect(lifecycle.recoverFailedOpen(failedRequest)).toBe(true);
    expect(lifecycle.isCurrent(failedRequest)).toBe(false);
    expect(lifecycle.pendingEditEventId).toBeUndefined();
    expect(lifecycle.surfaceStatus).toBeUndefined();
    expect(lifecycle.surfaceStatusEventId).toBeUndefined();
    expect(lifecycle.recoverFailedOpen(failedRequest)).toBe(false);
  });
});
