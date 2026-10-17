// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { containMusicDialogFocus } from "./dialog-focus";

afterEach(() => {
  document.body.replaceChildren();
});

function dialogFixture(): { trigger: HTMLButtonElement; dialog: HTMLDivElement; first: HTMLButtonElement; last: HTMLButtonElement } {
  const trigger = document.createElement("button");
  const dialog = document.createElement("div");
  const first = document.createElement("button");
  const disabled = document.createElement("button");
  const last = document.createElement("button");
  disabled.disabled = true;
  dialog.tabIndex = -1;
  dialog.append(first, disabled, last);
  document.body.append(trigger, dialog);
  trigger.focus();
  return { trigger, dialog, first, last };
}

describe("Music dialog focus containment", () => {
  it("focuses the first action, wraps Tab, and restores the trigger", async () => {
    const { trigger, dialog, first, last } = dialogFixture();
    const action = containMusicDialogFocus(dialog, { onEscape: vi.fn() });
    await Promise.resolve();
    expect(document.activeElement).toBe(first);

    last.focus();
    last.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(first);

    first.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(last);
    action.destroy();
    await Promise.resolve();
    expect(document.activeElement).toBe(trigger);
  });

  it("routes Escape only while closing is allowed", () => {
    const { dialog } = dialogFixture();
    const onEscape = vi.fn();
    const action = containMusicDialogFocus(dialog, { onEscape, escapeDisabled: true });
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onEscape).not.toHaveBeenCalled();
    action.update({ onEscape, escapeDisabled: false });
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(onEscape).toHaveBeenCalledOnce();
    action.destroy();
  });

  it("routes Enter only when a confirmation action is enabled", () => {
    const { dialog } = dialogFixture();
    const onEscape = vi.fn();
    const onEnter = vi.fn();
    const action = containMusicDialogFocus(dialog, { onEscape, onEnter, enterDisabled: true });
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(onEnter).not.toHaveBeenCalled();
    action.update({ onEscape, onEnter, enterDisabled: false });
    dialog.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(onEnter).toHaveBeenCalledOnce();
    action.destroy();
  });
});
