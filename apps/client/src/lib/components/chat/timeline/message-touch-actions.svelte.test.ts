// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActionReturn } from "svelte/action";
import { TOUCH_LONG_PRESS_MS } from "$lib/utils/touch-hold";
import {
  dismissHeldMessageActions,
  heldMessageActionsKey,
  messageTouchActions,
} from "./message-touch-actions.svelte";

type TouchActionsHandle = ActionReturn<string | null>;

function pointer(type: string, pointerType = "touch"): PointerEvent {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    pointerId: { value: 1 },
    pointerType: { value: pointerType },
    isPrimary: { value: true },
    clientX: { value: 10 },
    clientY: { value: 10 },
  });
  return event as PointerEvent;
}

function mountMessage(key: string | null): { scroller: HTMLElement; message: HTMLElement; handle: TouchActionsHandle } {
  const scroller = document.createElement("div");
  const message = document.createElement("article");
  message.innerHTML = `<p class="copy">Hello</p><button type="button" class="reply">Reply</button><div class="message-action-controls"><button type="button" class="copy-action">Copy</button></div>`;
  scroller.append(message);
  document.body.append(scroller);
  const handle = messageTouchActions(message, key) as TouchActionsHandle;
  return { scroller, message, handle };
}

function hold(target: Element): void {
  target.dispatchEvent(pointer("pointerdown"));
  vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS);
  window.dispatchEvent(pointer("pointerup"));
}

describe("message touch actions", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    dismissHeldMessageActions();
    document.body.replaceChildren();
    vi.useRealTimers();
  });

  it("reveals actions after a still touch hold but not after a tap", () => {
    const { message } = mountMessage("a");
    const copy = message.querySelector(".copy")!;

    copy.dispatchEvent(pointer("pointerdown"));
    vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS - 1);
    window.dispatchEvent(pointer("pointerup"));
    expect(heldMessageActionsKey()).toBeNull();

    hold(copy);
    expect(heldMessageActionsKey()).toBe("a");
  });

  it("ignores mouse presses, holds on controls, and rows without actions", () => {
    const { message } = mountMessage("a");
    message.querySelector(".copy")!.dispatchEvent(pointer("pointerdown", "mouse"));
    vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS);
    expect(heldMessageActionsKey()).toBeNull();

    hold(message.querySelector(".reply")!);
    expect(heldMessageActionsKey()).toBeNull();

    const { message: plainRow } = mountMessage(null);
    hold(plainRow.querySelector(".copy")!);
    expect(heldMessageActionsKey()).toBeNull();
  });

  it("keeps actions open for toolbar taps and hides them for other taps", () => {
    const { message } = mountMessage("a");
    hold(message.querySelector(".copy")!);

    message.querySelector(".copy-action")!.dispatchEvent(pointer("pointerdown"));
    expect(heldMessageActionsKey()).toBe("a");

    message.querySelector(".copy")!.dispatchEvent(pointer("pointerdown"));
    expect(heldMessageActionsKey()).toBeNull();
  });

  it("moves the revealed actions to the most recently held message", () => {
    const first = mountMessage("a");
    const second = mountMessage("b");
    hold(first.message.querySelector(".copy")!);

    hold(second.message.querySelector(".copy")!);

    expect(heldMessageActionsKey()).toBe("b");
  });

  it("hides actions when the message list scrolls but not when another surface scrolls", () => {
    const { scroller, message } = mountMessage("a");
    const picker = document.createElement("div");
    document.body.append(picker);
    hold(message.querySelector(".copy")!);

    picker.dispatchEvent(new Event("scroll"));
    expect(heldMessageActionsKey()).toBe("a");

    scroller.dispatchEvent(new Event("scroll"));
    expect(heldMessageActionsKey()).toBeNull();
  });

  it("hides actions when the held message unmounts or changes identity", () => {
    const first = mountMessage("a");
    hold(first.message.querySelector(".copy")!);
    first.handle.update?.("c");
    expect(heldMessageActionsKey()).toBeNull();

    const second = mountMessage("b");
    hold(second.message.querySelector(".copy")!);
    second.handle.destroy?.();
    expect(heldMessageActionsKey()).toBeNull();
  });
});
