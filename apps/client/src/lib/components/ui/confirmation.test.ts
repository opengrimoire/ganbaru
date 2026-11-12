// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConfirmationHost from "./ConfirmationHost.svelte";
import {
  activeConfirmation,
  answerConfirmation,
  registerConfirmationHost,
  requestConfirmation,
  type ConfirmationRequest,
} from "./confirmation.svelte";

const request: ConfirmationRequest = { message: "Discard the draft?", confirmLabel: "Discard", danger: true };

function openId(): number {
  const open = activeConfirmation();
  if (!open) throw new Error("expected an open confirmation");
  return open.id;
}

describe("shared confirmation service", () => {
  const cleanups: (() => void)[] = [];

  afterEach(async () => {
    for (const cleanup of cleanups.splice(0).reverse()) cleanup();
    await tick();
    vi.restoreAllMocks();
  });

  function withHost(): void {
    cleanups.push(registerConfirmationHost());
  }

  it("resolves true when confirmed and false when cancelled", async () => {
    withHost();
    const confirmed = requestConfirmation(request);
    expect(activeConfirmation()?.request).toEqual(request);
    answerConfirmation(openId(), true);
    await expect(confirmed).resolves.toBe(true);
    expect(activeConfirmation()).toBeNull();

    const cancelled = requestConfirmation(request);
    answerConfirmation(openId(), false);
    await expect(cancelled).resolves.toBe(false);
  });

  it("answers an earlier open request with false when a new one arrives", async () => {
    withHost();
    const first = requestConfirmation(request);
    const firstId = openId();
    const second = requestConfirmation({ ...request, message: "Close the terminal?" });
    await expect(first).resolves.toBe(false);
    expect(activeConfirmation()?.request.message).toBe("Close the terminal?");

    answerConfirmation(firstId, true);
    expect(activeConfirmation()).not.toBeNull();
    answerConfirmation(openId(), true);
    await expect(second).resolves.toBe(true);
  });

  it("answers false without a host and when the last host unmounts", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await expect(requestConfirmation(request)).resolves.toBe(false);
    expect(activeConfirmation()).toBeNull();

    const unregister = registerConfirmationHost();
    const open = requestConfirmation(request);
    unregister();
    unregister();
    await expect(open).resolves.toBe(false);
    await expect(requestConfirmation(request)).resolves.toBe(false);
  });

  it("renders the request in the shared dialog and resolves from its buttons and dismissal", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const host = mount(ConfirmationHost, { target });
    cleanups.push(() => {
      void unmount(host);
      target.remove();
    });
    flushSync();

    const confirmed = requestConfirmation({ ...request, cancelLabel: "Keep" });
    flushSync();
    expect(target.querySelector("[role='dialog']")?.textContent).toContain("Discard the draft?");
    const buttons = Array.from(target.querySelectorAll("button"));
    expect(buttons.map((button) => button.textContent?.trim())).toEqual(["Keep (Esc)", "Discard (Enter)"]);
    buttons[1]?.click();
    await expect(confirmed).resolves.toBe(true);
    flushSync();
    expect(target.querySelector("[role='dialog']")).toBeNull();

    const dismissed = requestConfirmation(request);
    flushSync();
    target.querySelector<HTMLElement>("[role='dialog']")
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await expect(dismissed).resolves.toBe(false);
  });
});
