// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import IconPicker from "./IconPicker.svelte";
import type { IconPickerAsset, IconPickerUploadAdapter } from "./types";

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...await importOriginal<typeof import("@tauri-apps/api/core")>(),
  invoke: async (command: string) => (command === "vault_read_config" ? "{}" : null),
}));

type Deferred<T> = { promise: Promise<T>; resolve: (value: T) => void; reject: (cause: unknown) => void };

function deferred<T>(): Deferred<T> {
  let resolve: (value: T) => void = () => {};
  let reject: (cause: unknown) => void = () => {};
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, resolve, reject };
}

/** A PNG signature and IHDR chunk for a 1 by 1 image, enough for the picker's header inspection. */
function onePixelPngHeader(): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(33);
  const view = new DataView(bytes.buffer);
  bytes.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  view.setUint32(8, 13);
  bytes.set([0x49, 0x48, 0x44, 0x52], 12);
  view.setUint32(16, 1);
  view.setUint32(20, 1);
  bytes.set([8, 6, 0, 0, 0], 24);
  return bytes;
}

function adapter(saveImageDataUrl: IconPickerUploadAdapter["saveImageDataUrl"]): IconPickerUploadAdapter {
  return {
    pickImageFile: async () => null,
    saveImageDataUrl,
    assetUrl: async (asset) => `asset://${asset.relativePath}`,
    selectAsset: () => {},
  };
}

describe("IconPicker uploads", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    });
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
  });

  async function openUploadTab(uploadAdapter: IconPickerUploadAdapter): Promise<HTMLElement> {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(IconPicker, {
      target,
      props: { value: "", onChange: () => {}, ariaLabel: "Icon", uploadAdapter, initiallyOpen: true },
    });
    await tick();
    const tab = document.querySelector<HTMLButtonElement>('[data-icon-picker-tab="upload"]');
    if (!tab) throw new Error("The upload tab did not render");
    tab.click();
    await tick();
    const panel = document.querySelector<HTMLElement>('[role="dialog"]');
    if (!panel) throw new Error("The picker panel did not render");
    return panel;
  }

  function paste(panel: HTMLElement, name: string): void {
    const event = new Event("paste", { bubbles: true, cancelable: true });
    const file = new File([onePixelPngHeader()], name, { type: "image/png" });
    Object.defineProperty(event, "clipboardData", { value: { files: [file] } });
    panel.dispatchEvent(event);
  }

  it("ignores a pasted image while another upload is still saving", async () => {
    const first = deferred<IconPickerAsset>();
    const save = vi.fn<IconPickerUploadAdapter["saveImageDataUrl"]>()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue({ relativePath: "second.png" });
    const panel = await openUploadTab(adapter(save));

    paste(panel, "first.png");
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    paste(panel, "second.png");
    await new Promise((resolve) => setTimeout(resolve, 20));
    flushSync();

    expect(save).toHaveBeenCalledTimes(1);
    first.resolve({ relativePath: "first.png" });
  });

  it("drops an upload failure that lands after the picker closed", async () => {
    const pending = deferred<IconPickerAsset>();
    const save = vi.fn<IconPickerUploadAdapter["saveImageDataUrl"]>().mockReturnValueOnce(pending.promise);
    const panel = await openUploadTab(adapter(save));

    paste(panel, "slow.png");
    await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1));
    const trigger = target?.querySelector<HTMLButtonElement>("button");
    if (!trigger) throw new Error("The picker trigger did not render");
    trigger.click();
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    pending.reject(new Error("disk full"));
    await new Promise((resolve) => setTimeout(resolve, 0));

    trigger.click();
    await tick();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(document.body.textContent).not.toContain("Could not add this image.");
  });
});
