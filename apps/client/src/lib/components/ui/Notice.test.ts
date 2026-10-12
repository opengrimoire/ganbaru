// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Notice from "./Notice.svelte";

describe("Notice", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;
  const writeText = vi.fn<(value: string) => Promise<void>>();

  beforeEach(() => {
    vi.useFakeTimers();
    writeText.mockReset();
    writeText.mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    vi.useRealTimers();
  });

  function render(props: Record<string, unknown>): HTMLDivElement {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(Notice, { target, props: { message: "Could not load the list.", ...props } });
    flushSync();
    return target;
  }

  function button(root: HTMLElement, selector: string): HTMLButtonElement {
    const element = root.querySelector<HTMLButtonElement>(selector);
    if (!element) throw new Error(`Missing ${selector}`);
    return element;
  }

  async function settle(ms: number): Promise<void> {
    await vi.advanceTimersByTimeAsync(ms);
    await tick();
  }

  it("shows the message as an alert with copy but no retry when nothing can be retried", () => {
    const root = render({});

    expect(root.querySelector("[role='alert']")?.textContent).toContain("Could not load the list.");
    expect(root.querySelector("[data-notice-retry]")).toBeNull();
    expect(root.querySelector("[data-notice-copy]")).not.toBeNull();
  });

  it("keeps the retry icon turning while the retry is pending and ignores repeated presses", async () => {
    let resolveRetry: () => void = () => undefined;
    const onRetry = vi.fn(() => new Promise<void>((resolve) => { resolveRetry = resolve; }));
    const root = render({ onRetry });
    const retry = button(root, "[data-notice-retry]");

    retry.click();
    retry.click();
    await tick();

    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(retry.getAttribute("data-notice-retry")).toBe("pending");
    expect(retry.getAttribute("aria-busy")).toBe("true");

    resolveRetry();
    await settle(0);
    expect(retry.getAttribute("data-notice-retry")).toBe("pending");

    await settle(480);
    expect(retry.getAttribute("data-notice-retry")).toBe("done");

    await settle(1_600);
    expect(retry.getAttribute("data-notice-retry")).toBe("idle");
  });

  it("returns the retry button to idle without a check mark when the retry fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const root = render({ onRetry: () => Promise.reject(new Error("still offline")) });
    const retry = button(root, "[data-notice-retry]");

    retry.click();
    await settle(480);

    expect(retry.getAttribute("data-notice-retry")).toBe("idle");
    expect(warn).toHaveBeenCalledWith("Notice retry failed", expect.any(Error));
    warn.mockRestore();
  });

  it("copies the title and message by default and confirms with a check mark", async () => {
    const root = render({ title: "Launch failed:" });
    const copy = button(root, "[data-notice-copy]");

    copy.click();
    await settle(0);

    expect(writeText).toHaveBeenCalledWith("Launch failed: Could not load the list.");
    expect(copy.getAttribute("data-notice-copy")).toBe("done");
    expect(root.querySelector("[aria-live='polite']")?.textContent).toBe("Message copied");

    await settle(1_600);
    expect(copy.getAttribute("data-notice-copy")).toBe("idle");
  });

  it("copies custom detail text and marks a failed copy", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    writeText.mockRejectedValue(new Error("denied"));
    Object.defineProperty(document, "execCommand", { configurable: true, value: () => false });
    const root = render({ copy: "sqlite error 5" });
    const copy = button(root, "[data-notice-copy]");

    copy.click();
    await settle(0);

    expect(writeText).toHaveBeenCalledWith("sqlite error 5");
    expect(copy.getAttribute("data-notice-copy")).toBe("failed");
    expect(copy.getAttribute("aria-label")).toBe("Could not copy the message");
    warn.mockRestore();
  });

  it("hides the copy button when copying is turned off", () => {
    const root = render({ copy: false, onRetry: () => undefined });

    expect(root.querySelector("[data-notice-copy]")).toBeNull();
    expect(root.querySelector("[data-notice-retry]")).not.toBeNull();
  });

  it("closes through the dismiss button after the other actions", () => {
    const onDismiss = vi.fn();
    const root = render({ layout: "floating", onRetry: () => undefined, onDismiss });
    const buttons = [...root.querySelectorAll<HTMLButtonElement>("button")];

    expect(buttons.at(-1)?.hasAttribute("data-notice-dismiss")).toBe(true);
    buttons.at(-1)?.click();
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("centers the notice in the failed area for the fill layout", () => {
    const root = render({ layout: "fill", role: "status" });

    expect(root.querySelector(".notice-fill > [role='status']")).not.toBeNull();
  });
});
