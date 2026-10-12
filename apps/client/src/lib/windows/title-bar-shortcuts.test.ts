import { describe, expect, it } from "vitest";
import {
  isCloseWindowShortcut,
  recordResetShortcutPress,
  titleBarShortcutAction,
  type CloseWindowShortcutEvent,
  type ResetShortcutSequenceOptions,
  type ResetShortcutSequenceState,
} from "./title-bar-shortcuts";

const options: ResetShortcutSequenceOptions = {
  requiredPresses: 10,
  maxGapMs: 8_000,
};

function emptySequence(): ResetShortcutSequenceState {
  return { pressCount: 0, lastPressAtMs: null };
}

function shortcutEvent(overrides: Partial<CloseWindowShortcutEvent>): CloseWindowShortcutEvent {
  return {
    altKey: false,
    ctrlKey: false,
    key: "w",
    metaKey: false,
    shiftKey: false,
    ...overrides,
  };
}

describe("title bar shortcuts", () => {
  it("matches close shortcuts with and without shift", () => {
    expect(isCloseWindowShortcut(shortcutEvent({ ctrlKey: true }))).toBe(true);
    expect(isCloseWindowShortcut(shortcutEvent({ ctrlKey: true, shiftKey: true }))).toBe(true);
  });

  it("rejects close shortcuts with extra modifiers or another key", () => {
    expect(isCloseWindowShortcut(shortcutEvent({ ctrlKey: true, altKey: true }))).toBe(false);
    expect(isCloseWindowShortcut(shortcutEvent({ ctrlKey: true, key: "t" }))).toBe(false);
  });

  it("waits before the reset threshold so close can still run", () => {
    let state = emptySequence();

    for (let press = 1; press < options.requiredPresses; press += 1) {
      const result = recordResetShortcutPress(state, press * 100, options);
      state = result.state;

      expect(result.resetTriggered).toBe(false);
      expect(state.pressCount).toBe(press);
    }
  });

  it("triggers reset and clears the sequence at the required press count", () => {
    let state = emptySequence();

    for (let press = 1; press <= options.requiredPresses; press += 1) {
      const result = recordResetShortcutPress(state, press * 100, options);
      state = result.state;

      if (press < options.requiredPresses) {
        expect(result.resetTriggered).toBe(false);
      } else {
        expect(result.resetTriggered).toBe(true);
        expect(state).toEqual(emptySequence());
      }
    }
  });

  it("starts over when the previous press is outside the shortcut window", () => {
    let state = emptySequence();
    state = recordResetShortcutPress(state, 0, options).state;
    state = recordResetShortcutPress(state, 1_000, options).state;

    const result = recordResetShortcutPress(state, 10_001, options);

    expect(result.resetTriggered).toBe(false);
    expect(result.state).toEqual({ pressCount: 1, lastPressAtMs: 10_001 });
  });

  it("classifies theme and zoom shortcuts", () => {
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, shiftKey: true, key: "l" })))
      .toBe("theme-toggle");
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, shiftKey: true, key: "t" })))
      .toBe("theme-switcher");
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, key: "+" })))
      .toBe("zoom-in");
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, key: "-" })))
      .toBe("zoom-out");
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, key: "0" })))
      .toBe("zoom-reset");
  });

  it("keeps close and theme commands global", () => {
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true }))).toBe("close");
    expect(titleBarShortcutAction(shortcutEvent({ ctrlKey: true, shiftKey: true, key: "l" })))
      .toBe("theme-toggle");
  });
});
