// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import ConnectAndroidHarness from "./ConnectAndroidHarness.test.svelte";
import ConnectInlineHarness from "./ConnectInlineHarness.test.svelte";

vi.mock("$lib/api/vault-handoff", () => ({
  joinVaultThroughServer: vi.fn(),
  recoverVaultThroughServer: vi.fn(),
  releaseStarterVault: vi.fn(),
  decodePairingQr: vi.fn(),
  decodeServerSetupQr: vi.fn(),
}));

/** jsdom has no Web Animations API, so transitions get an animation that finishes on the next microtask. */
function finishedAnimation(): Animation {
  const animation = { onfinish: null, cancel: () => undefined, currentTime: 0, playState: "finished", effect: null } as unknown as Animation;
  queueMicrotask(() => animation.onfinish?.(new Event("finish") as AnimationPlaybackEvent));
  return animation;
}

beforeAll(() => {
  Element.prototype.animate = finishedAnimation;
});

describe("ConnectSteps inline with ConnectSourceTabs", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;
  const onSubmit = vi.fn();

  beforeEach(() => {
    onSubmit.mockReset();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(ConnectInlineHarness, { target, props: { onSubmit } });
    flushSync();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
  });

  function tab(id: string): HTMLButtonElement {
    const button = target.querySelector<HTMLButtonElement>(`#tab-${id}`);
    if (!button) throw new Error(`missing tab ${id}`);
    return button;
  }

  function press(element: HTMLElement, key: string): void {
    element.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
    flushSync();
  }

  it("writes the sources into the title as a localized list and starts on the caller's panel", () => {
    expect(target.querySelector("h1")?.textContent?.replace(/\s+/g, " ").trim())
      .toBe("Connect to your phone, another computer, or a server");
    expect(target.querySelector("[data-phone-panel]")).not.toBeNull();
    expect(target.querySelector("[data-connect-submit]")).toBeNull();
    expect(target.querySelector('[role="tabpanel"]')?.getAttribute("aria-labelledby")).toBe("tab-phone");
  });

  it("switches to the computer steps from the keyboard without a modal and keeps focus on the tabs", async () => {
    tab("phone").focus();
    press(tab("phone"), "ArrowRight");
    await vi.waitFor(() => expect(document.activeElement).toBe(tab("computer")));
    expect(tab("computer").getAttribute("aria-selected")).toBe("true");
    expect(target.querySelector("#pairing-code")).not.toBeNull();
    await vi.waitFor(() => expect(target.querySelector("[data-phone-panel]")).toBeNull());
    press(tab("computer"), "End");
    await vi.waitFor(() => expect(document.activeElement).toBe(tab("server")));
    press(tab("server"), "Home");
    await vi.waitFor(() => expect(document.activeElement).toBe(tab("phone")));
  });

  it("submits a pairing code with Enter in its field", () => {
    tab("computer").click();
    flushSync();
    const input = target.querySelector<HTMLInputElement>("#pairing-code");
    if (!input) throw new Error("missing pairing field");
    input.value = " pairing-code ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    press(input, "Enter");
    expect(onSubmit).toHaveBeenCalledWith("pairing-code");
  });

  it("keeps one Connect button in place while switching between the computer and server steps", async () => {
    tab("computer").click();
    flushSync();
    const button = target.querySelector("[data-connect-submit]");
    expect(button?.textContent?.trim()).toBe("Connect");
    tab("server").click();
    flushSync();
    await vi.waitFor(() => expect(target.querySelector("#pairing-code")).toBeNull());
    expect(target.querySelectorAll("[data-connect-submit]")).toHaveLength(1);
    expect(target.querySelector("[data-connect-submit]")).toBe(button);
  });

  it("removes the steps and their submit button when the caller's panel returns", async () => {
    tab("server").click();
    flushSync();
    expect(target.querySelector("#server-connect-code")).not.toBeNull();
    tab("phone").click();
    flushSync();
    await vi.waitFor(() => expect(target.querySelector("[data-connect-submit]")).toBeNull());
    expect(target.querySelector("#server-connect-code")).toBeNull();
    expect(target.querySelector("[data-phone-panel]")).not.toBeNull();
  });
});

describe("ConnectSteps on Android", () => {
  let target: HTMLDivElement;
  let component: ReturnType<typeof mount> | null = null;
  const onSubmit = vi.fn();

  function render(autoScan: boolean): void {
    component = mount(ConnectAndroidHarness, { target, props: { source: "computer", autoScan, onSubmit } });
    flushSync();
  }

  function link(text: string): HTMLButtonElement | undefined {
    return [...target.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === text);
  }

  beforeEach(() => {
    onSubmit.mockReset();
    target = document.createElement("div");
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
  });

  it("starts with the code field and offers the scanner when it does not scan first", () => {
    render(false);
    expect(target.querySelector("#pairing-code")).not.toBeNull();
    expect(target.querySelector("[data-connect-submit]")).not.toBeNull();
    expect(link("Scan the QR code instead")).toBeDefined();
  });

  it("returns to the code field with a notice when the camera cannot start", async () => {
    render(false);
    link("Scan the QR code instead")?.click();
    flushSync();
    await vi.waitFor(() => expect(target.textContent).toContain("The camera could not start."));
    expect(target.querySelector("#pairing-code")).not.toBeNull();
    expect(link("Scan the QR code instead")).toBeDefined();
  });
});
