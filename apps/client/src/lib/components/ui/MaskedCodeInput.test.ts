// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MaskedCodeInputHarness from "./MaskedCodeInputHarness.test.svelte";

const CODE = "ganbaru-anchor:AbCdEfGhIjKlMnOp";
const MASKED = "ganbaru-anchor:AbCd••••MnOp";

describe("MaskedCodeInput", () => {
  let target: HTMLDivElement;
  let component: { current: () => string; set: (next: string) => void } | null = null;
  const onInput = vi.fn();

  beforeEach(() => {
    onInput.mockReset();
    target = document.createElement("div");
    document.body.append(target);
    component = mount(MaskedCodeInputHarness, { target, props: { onInput } });
    flushSync();
  });

  afterEach(async () => {
    if (component) await unmount(component);
    target.remove();
    component = null;
  });

  function current(): string {
    if (!component) throw new Error("missing component");
    return component.current();
  }

  function input(): HTMLInputElement {
    const element = target.querySelector<HTMLInputElement>("#code");
    if (!element) throw new Error("missing input");
    return element;
  }

  function clearButton(): HTMLButtonElement | null {
    return target.querySelector<HTMLButtonElement>('button[aria-label="Clear code"]');
  }

  function paste(text: string): ClipboardEvent {
    const event = new Event("paste", { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(event, "clipboardData", { value: { getData: () => text } });
    input().dispatchEvent(event);
    flushSync();
    return event;
  }

  function beforeInput(inputType: string, data: string | null = null): InputEvent {
    const event = new InputEvent("beforeinput", { inputType, data, bubbles: true, cancelable: true });
    input().dispatchEvent(event);
    flushSync();
    return event;
  }

  function type(value: string): void {
    input().value = value;
    input().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  it("shows a pasted code by its ends and keeps the whole trimmed code as the value", () => {
    const event = paste(`  ${CODE}\n`);
    expect(event.defaultPrevented).toBe(true);
    expect(input().value).toBe(MASKED);
    expect(current()).toBe(CODE);
    expect(onInput).toHaveBeenCalledOnce();
    expect(clearButton()).not.toBeNull();
  });

  it("replaces existing text with the pasted code", () => {
    type("ganbaru-");
    paste(CODE);
    expect(current()).toBe(CODE);
  });

  it("leaves an empty paste to the browser", () => {
    expect(paste("  ").defaultPrevented).toBe(false);
    expect(current()).toBe("");
  });

  it("keeps a typed code visible", () => {
    type(CODE);
    expect(input().value).toBe(CODE);
    expect(clearButton()).toBeNull();
    expect(beforeInput("insertText", "x").defaultPrevented).toBe(false);
  });

  it("clears the whole masked code on delete instead of editing the mask", () => {
    paste(CODE);
    expect(beforeInput("deleteContentBackward").defaultPrevented).toBe(true);
    expect(current()).toBe("");
    expect(input().value).toBe("");
  });

  it("starts over with the typed character when typing over a masked code", () => {
    paste(CODE);
    beforeInput("insertText", "g");
    expect(current()).toBe("g");
    expect(input().value).toBe("g");
    expect(clearButton()).toBeNull();
  });

  it("starts over when an uncancelable input edits the mask", () => {
    paste(CODE);
    type(`${MASKED}x`);
    expect(current()).toBe("");
  });

  it("clears the code with its button and returns focus to the field", () => {
    paste(CODE);
    clearButton()?.click();
    flushSync();
    expect(current()).toBe("");
    expect(document.activeElement).toBe(input());
  });

  it("shows a different code its owner sets in full and an emptied one as empty", () => {
    paste(CODE);
    component?.set("ganbaru-anchor:ScannedFromQrCode");
    flushSync();
    expect(input().value).toBe("ganbaru-anchor:ScannedFromQrCode");
    component?.set("");
    flushSync();
    expect(input().value).toBe("");
    expect(clearButton()).toBeNull();
  });
});
