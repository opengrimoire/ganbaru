// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { activateModalKeyboardLayer } from "$lib/modal-focus";
import { onboardingPrimaryAction } from "./onboarding-primary-action";

describe("onboarding primary action", () => {
  let button: HTMLButtonElement;
  let destroy: (() => void) | undefined;
  let click: Mock<() => void>;

  beforeEach(() => {
    button = document.createElement("button");
    click = vi.fn();
    button.addEventListener("click", click);
    document.body.append(button);
    destroy = onboardingPrimaryAction(button)?.destroy;
  });

  afterEach(() => {
    destroy?.();
    document.body.replaceChildren();
  });

  function pressEnter(options: KeyboardEventInit = {}, target: EventTarget = window): KeyboardEvent {
    const event = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
      ...options,
    });
    target.dispatchEvent(event);
    return event;
  }

  it("activates the main button without requiring focus", () => {
    const event = pressEnter();

    expect(click).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);
  });

  it.each([
    { key: "Escape" },
    { repeat: true },
    { isComposing: true },
    { altKey: true },
    { ctrlKey: true },
    { metaKey: true },
    { shiftKey: true },
  ])("ignores other keys, repeats, composition, and modifiers: %j", (options) => {
    pressEnter(options);

    expect(click).not.toHaveBeenCalled();
  });

  it("preserves events already handled by another control", () => {
    const preventDefault = (event: KeyboardEvent): void => event.preventDefault();
    document.addEventListener("keydown", preventDefault);
    try {
      pressEnter({}, document.body);
      expect(click).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", preventDefault);
    }
  });

  it.each(["button", "a", "input", "textarea", "select"])(
    "preserves Enter for a focused %s",
    (tag) => {
      const control = document.createElement(tag);
      if (control instanceof HTMLAnchorElement) control.href = "#";
      document.body.append(control);
      control.focus();

      const event = pressEnter({}, control);

      expect(click).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    },
  );

  it("preserves native activation when the main button is focused", () => {
    button.focus();

    const event = pressEnter({}, button);

    expect(event.defaultPrevented).toBe(false);
    expect(click).not.toHaveBeenCalled();
  });

  it("preserves Enter inside editable content", () => {
    const editor = document.createElement("div");
    editor.setAttribute("contenteditable", "plaintext-only");
    const content = document.createElement("span");
    editor.append(content);
    document.body.append(editor);

    pressEnter({}, content);

    expect(click).not.toHaveBeenCalled();
  });

  it("leaves keyboard input to an open modal", () => {
    const deactivate = activateModalKeyboardLayer(() => undefined);
    try {
      pressEnter();
      expect(click).not.toHaveBeenCalled();
    } finally {
      deactivate();
    }
  });

  it("waits until a busy button becomes enabled", () => {
    button.disabled = true;
    pressEnter();
    expect(click).not.toHaveBeenCalled();

    button.disabled = false;
    pressEnter();
    expect(click).toHaveBeenCalledOnce();
  });

  it("ignores an inactive screen", () => {
    const screen = document.createElement("section");
    screen.setAttribute("inert", "");
    document.body.append(screen);
    screen.append(button);

    pressEnter();

    expect(click).not.toHaveBeenCalled();
  });

  it("stops activating the previous action when the screen changes", () => {
    destroy?.();
    button.remove();
    const nextButton = document.createElement("button");
    const nextClick = vi.fn();
    nextButton.addEventListener("click", nextClick);
    document.body.append(nextButton);
    destroy = onboardingPrimaryAction(nextButton)?.destroy;

    pressEnter();

    expect(click).not.toHaveBeenCalled();
    expect(nextClick).toHaveBeenCalledOnce();
  });
});
