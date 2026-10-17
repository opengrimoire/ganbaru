// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import {
  canRunEventPanelSave,
  eventPanelKeyboardAction,
  isEventPanelDeleteActionTarget,
} from "./actions-controller.svelte";

function key(key: string, modifiers: Partial<Pick<KeyboardEvent, "altKey" | "ctrlKey" | "metaKey" | "shiftKey">> = {}) {
  return {
    key,
    altKey: false,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...modifiers,
  };
}

describe("eventPanelKeyboardAction", () => {
  it("maps panel shortcuts without intercepting unrelated input", () => {
    expect(eventPanelKeyboardAction(key("Enter", { ctrlKey: true }), {
      parked: false,
      deleteArmed: false,
    })).toBe("save");
    expect(eventPanelKeyboardAction(key("d", { ctrlKey: true }), {
      parked: false,
      deleteArmed: false,
    })).toBe("delete");
    expect(eventPanelKeyboardAction(key("x"), {
      parked: false,
      deleteArmed: false,
    })).toBeNull();
  });

  it("gives armed confirmation priority and disables shortcuts while parked", () => {
    expect(eventPanelKeyboardAction(key("Enter"), {
      parked: false,
      deleteArmed: true,
    })).toBe("confirm-delete");
    expect(eventPanelKeyboardAction(key("Enter", { ctrlKey: true }), {
      parked: true,
      deleteArmed: false,
    })).toBeNull();
  });
});

describe("canRunEventPanelSave", () => {
  it("allows the explicit read-only Pomodoro path but rejects other disabled saves", () => {
    expect(canRunEventPanelSave({
      parked: false,
      controlsDisabled: true,
      pomodoroReadOnlyInteractive: true,
      savePending: false,
    })).toBe(true);
    expect(canRunEventPanelSave({
      parked: false,
      controlsDisabled: true,
      pomodoroReadOnlyInteractive: false,
      savePending: false,
    })).toBe(false);
    expect(canRunEventPanelSave({
      parked: true,
      controlsDisabled: false,
      pomodoroReadOnlyInteractive: false,
      savePending: false,
    })).toBe(false);
  });
});

describe("isEventPanelDeleteActionTarget", () => {
  it("keeps destructive button taps armed while other panel clicks disarm them", () => {
    const button = document.createElement("button");
    const icon = document.createElement("span");
    const other = document.createElement("button");
    button.dataset.eventPanelDeleteAction = "";
    button.append(icon);

    expect(isEventPanelDeleteActionTarget(button)).toBe(true);
    expect(isEventPanelDeleteActionTarget(icon)).toBe(true);
    expect(isEventPanelDeleteActionTarget(other)).toBe(false);
  });
});
