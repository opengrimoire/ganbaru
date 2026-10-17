import { describe, expect, it, vi } from "vitest";
import { MobilePersistenceLifecycleController } from "./persistence-lifecycle";

class DocumentTarget extends EventTarget {
  visibilityState: DocumentVisibilityState = "visible";
}

describe("MobilePersistenceLifecycleController", () => {
  it("flushes each persistence boundary when the document becomes hidden", async () => {
    const documentTarget = new DocumentTarget();
    const windowTarget = new EventTarget();
    let observeFlush = (): void => undefined;
    const observedFlush = new Promise<void>((resolve) => {
      observeFlush = resolve;
    });
    const flushConfig = vi.fn(async () => {
      observeFlush();
    });
    const flushNotes = vi.fn(async () => undefined);
    const controller = new MobilePersistenceLifecycleController({
      documentTarget,
      windowTarget,
      flushers: { config: flushConfig, notes: flushNotes },
    });

    const detach = controller.attach();
    documentTarget.visibilityState = "hidden";
    documentTarget.dispatchEvent(new Event("visibilitychange"));
    await observedFlush;

    expect(flushConfig).toHaveBeenCalledOnce();
    expect(flushNotes).toHaveBeenCalledOnce();
    detach();
  });

  it("runs a final pass when pagehide arrives during an active flush", async () => {
    const documentTarget = new DocumentTarget();
    const windowTarget = new EventTarget();
    let releaseFirstPass = (): void => undefined;
    const firstPass = new Promise<void>((resolve) => {
      releaseFirstPass = resolve;
    });
    const flusher = vi.fn()
      .mockImplementationOnce(() => firstPass)
      .mockResolvedValue(undefined);
    const controller = new MobilePersistenceLifecycleController({
      documentTarget,
      windowTarget,
      flushers: { notes: flusher },
    });

    controller.attach();
    const pending = controller.flush();
    windowTarget.dispatchEvent(new Event("pagehide"));
    releaseFirstPass();
    await pending;

    expect(flusher).toHaveBeenCalledTimes(2);
  });

  it("reports one flusher failure without skipping the others", async () => {
    const documentTarget = new DocumentTarget();
    const windowTarget = new EventTarget();
    const onError = vi.fn();
    const flushNotes = vi.fn(async () => undefined);
    const controller = new MobilePersistenceLifecycleController({
      documentTarget,
      windowTarget,
      flushers: {
        config: async () => Promise.reject(new Error("write failed")),
        notes: flushNotes,
      },
      onError,
    });

    await controller.flush();

    expect(flushNotes).toHaveBeenCalledOnce();
    expect(onError).toHaveBeenCalledWith("config", expect.any(Error));
  });
});
