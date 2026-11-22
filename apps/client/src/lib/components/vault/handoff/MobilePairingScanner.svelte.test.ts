// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MobilePairingScanner from "./MobilePairingScanner.svelte";

const camera = vi.hoisted(() => ({
  scanFrame: vi.fn<() => Promise<string>>(),
}));

vi.mock("$lib/vault/pairing-camera", () => ({
  PairingCameraScanner: class {
    start(): Promise<void> {
      return Promise.resolve();
    }

    scanFrame(): Promise<string> {
      return camera.scanFrame();
    }

    stop(): void {}
  },
}));

describe("MobilePairingScanner", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    camera.scanFrame.mockReset();
    camera.scanFrame.mockRejectedValue(new Error("no code in frame"));
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    component = null;
    target.remove();
    vi.useRealTimers();
  });

  it("resumes scanning once a pause for a running connection ends", async () => {
    const props = $state({ disabled: false });
    const onInvitation = vi.fn();
    component = mount(MobilePairingScanner, {
      target,
      props: {
        get disabled() {
          return props.disabled;
        },
        onInvitation,
        onError: () => undefined,
      },
    });
    flushSync();
    await vi.advanceTimersByTimeAsync(200);
    expect(camera.scanFrame).toHaveBeenCalledTimes(1);

    props.disabled = true;
    flushSync();
    await vi.advanceTimersByTimeAsync(1_000);
    const scansWhileDisabled = camera.scanFrame.mock.calls.length;
    await vi.advanceTimersByTimeAsync(1_000);
    expect(camera.scanFrame).toHaveBeenCalledTimes(scansWhileDisabled);

    camera.scanFrame.mockResolvedValue("ganbaru-anchor:code");
    props.disabled = false;
    flushSync();
    await vi.advanceTimersByTimeAsync(200);
    expect(onInvitation).toHaveBeenCalledWith("ganbaru-anchor:code");
  });
});
