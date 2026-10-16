// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  activateModalFocus,
  captureFocusReturn,
  activateIsolatingKeyboardLayer,
  activateModalKeyboardLayer,
  installModalKeyboardRouter,
  isAppKeyboardIsolated,
  trapModalTabKey,
} from "./modal-focus";

describe("modal focus", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("wraps forward and reverse Tab navigation", () => {
    const modal = document.createElement("div");
    const first = document.createElement("button");
    const last = document.createElement("button");
    modal.append(first, last);
    document.body.append(modal);

    last.focus();
    expect(trapModalTabKey(modal, new KeyboardEvent("keydown", { key: "Tab", cancelable: true })))
      .toBe(true);
    expect(document.activeElement).toBe(first);

    first.focus();
    expect(trapModalTabKey(
      modal,
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, cancelable: true }),
    )).toBe(true);
    expect(document.activeElement).toBe(last);
  });

  function keyboardFocused(element: HTMLElement, focusVisible: boolean): void {
    const matches = element.matches.bind(element);
    vi.spyOn(element, "matches").mockImplementation((selector) =>
      selector === ":focus-visible" ? focusVisible : matches(selector));
    element.focus();
  }

  it("restores focus to a keyboard-focused invoking control", async () => {
    const trigger = document.createElement("button");
    const modal = document.createElement("div");
    const close = document.createElement("button");
    modal.append(close);
    document.body.append(trigger, modal);
    keyboardFocused(trigger, true);

    const deactivate = activateModalFocus(modal, close);
    await Promise.resolve();
    expect(document.activeElement).toBe(close);
    deactivate();
    await Promise.resolve();
    expect(document.activeElement).toBe(trigger);
  });

  it("does not re-arm a pointer-focused trigger or keep focus in a hidden surface", async () => {
    const trigger = document.createElement("button");
    const surface = document.createElement("div");
    const row = document.createElement("button");
    surface.append(row);
    document.body.append(trigger, surface);
    keyboardFocused(trigger, false);

    const returnFocus = captureFocusReturn();
    row.focus();
    surface.hidden = true;
    returnFocus(surface);
    await Promise.resolve();
    expect(document.activeElement).toBe(document.body);
  });

  it("returns pointer-opened nested surfaces to the dialog that contained the trigger", async () => {
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    dialog.tabIndex = -1;
    const trigger = document.createElement("button");
    dialog.append(trigger);
    const picker = document.createElement("div");
    const option = document.createElement("button");
    picker.append(option);
    document.body.append(dialog, picker);
    keyboardFocused(trigger, false);

    const deactivate = activateModalFocus(picker, option);
    await Promise.resolve();
    picker.remove();
    deactivate();
    await Promise.resolve();
    expect(document.activeElement).toBe(dialog);
  });

  it("leaves focus the user moved outside the closing surface", async () => {
    const trigger = document.createElement("button");
    const surface = document.createElement("div");
    const elsewhere = document.createElement("input");
    document.body.append(trigger, surface, elsewhere);
    keyboardFocused(trigger, true);

    const returnFocus = captureFocusReturn();
    elsewhere.focus();
    returnFocus(surface);
    await Promise.resolve();
    expect(document.activeElement).toBe(elsewhere);
  });

  it("does not let an inert parent modal compete with a nested modal", () => {
    const parent = document.createElement("div");
    const parentButton = document.createElement("button");
    const nested = document.createElement("div");
    const nestedButton = document.createElement("button");
    parent.append(parentButton);
    nested.append(nestedButton);
    document.body.append(parent, nested);
    parent.setAttribute("inert", "");
    nestedButton.focus();

    const event = new KeyboardEvent("keydown", { key: "Tab", cancelable: true });
    expect(trapModalTabKey(parent, event)).toBe(false);
    expect(event.defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(nestedButton);
  });

  it("routes keys before feature handlers mounted after application bootstrap", () => {
    const uninstallRouter = installModalKeyboardRouter(window);
    const onParentKeydown = vi.fn();
    const onModalKeydown = vi.fn();
    window.addEventListener("keydown", onParentKeydown, true);
    const deactivateModal = activateModalKeyboardLayer(onModalKeydown);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));

    expect(onModalKeydown).toHaveBeenCalledOnce();
    expect(onParentKeydown).not.toHaveBeenCalled();
    deactivateModal();
    window.removeEventListener("keydown", onParentKeydown, true);
    uninstallRouter();
  });

  it("stops only the keys an isolating panel claims and lets the rest reach its controls", () => {
    const uninstallRouter = installModalKeyboardRouter(window);
    const input = document.createElement("input");
    document.body.append(input);
    const onAppKeydown = vi.fn();
    const onInputKeydown = vi.fn();
    window.addEventListener("keydown", onAppKeydown, true);
    input.addEventListener("keydown", onInputKeydown);
    const release = activateIsolatingKeyboardLayer((event) => event.key === "1");

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "1", bubbles: true, cancelable: true }));
    expect(onAppKeydown).not.toHaveBeenCalled();
    expect(onInputKeydown).not.toHaveBeenCalled();

    input.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true }));
    expect(onAppKeydown).toHaveBeenCalledOnce();
    expect(onInputKeydown).toHaveBeenCalledOnce();

    release();
    window.removeEventListener("keydown", onAppKeydown, true);
    uninstallRouter();
  });

  it("reports keyboard isolation while an isolating panel is open, even under a dialog", () => {
    expect(isAppKeyboardIsolated()).toBe(false);
    const releasePanel = activateIsolatingKeyboardLayer(() => false);
    const releaseDialog = activateModalKeyboardLayer(() => undefined);
    expect(isAppKeyboardIsolated()).toBe(true);
    releaseDialog();
    expect(isAppKeyboardIsolated()).toBe(true);
    releasePanel();
    expect(isAppKeyboardIsolated()).toBe(false);
  });
});
