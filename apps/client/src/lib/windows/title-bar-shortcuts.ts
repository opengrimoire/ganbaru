import { hasOnlyShortcutModifier, type KeyboardModifierState } from "$lib/keyboard-shortcuts";

export interface ResetShortcutSequenceState {
  pressCount: number;
  lastPressAtMs: number | null;
}

export interface ResetShortcutSequenceOptions {
  requiredPresses: number;
  maxGapMs: number;
}

export interface ResetShortcutSequenceResult {
  state: ResetShortcutSequenceState;
  resetTriggered: boolean;
}

export type CloseWindowShortcutEvent = Pick<KeyboardEvent, "key"> & KeyboardModifierState;

export type TitleBarShortcutAction =
  | "close"
  | "theme-toggle"
  | "theme-switcher"
  | "zoom-in"
  | "zoom-out"
  | "zoom-reset"
  | "music-toggle"
  | "diagnostics-toggle"
  | "shortcut-help";

export type TitleBarShortcutEvent = Pick<KeyboardEvent, "key"> & KeyboardModifierState;

/**
 * Record one hidden reset shortcut press.
 *
 * The reset shortcut shares its key chord with close. Callers should only
 * consume the keyboard event when `resetTriggered` is true so earlier presses
 * can keep their normal close behavior.
 */
export function recordResetShortcutPress(
  state: ResetShortcutSequenceState,
  nowMs: number,
  options: ResetShortcutSequenceOptions,
): ResetShortcutSequenceResult {
  const sequenceIsActive =
    state.lastPressAtMs !== null
    && nowMs - state.lastPressAtMs <= options.maxGapMs;
  const pressCount = (sequenceIsActive ? state.pressCount : 0) + 1;

  if (pressCount >= options.requiredPresses) {
    return {
      state: { pressCount: 0, lastPressAtMs: null },
      resetTriggered: true,
    };
  }

  return {
    state: { pressCount, lastPressAtMs: nowMs },
    resetTriggered: false,
  };
}

export function isCloseWindowShortcut(event: CloseWindowShortcutEvent): boolean {
  const key = event.key.toLowerCase();
  if (key !== "w") return false;
  return hasOnlyShortcutModifier(event) || hasOnlyShortcutModifier(event, { shift: true });
}

/** Resolve shell-wide shortcuts before view-level handlers run. */
export function titleBarShortcutAction(
  event: TitleBarShortcutEvent,
): TitleBarShortcutAction | null {
  if (isCloseWindowShortcut(event)) return "close";
  const key = event.key.toLowerCase();
  if (key === "f1" && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) return "shortcut-help";
  if (hasOnlyShortcutModifier(event, { shift: true })) {
    if (key === "l") return "theme-toggle";
    if (key === "t") return "theme-switcher";
    if (key === "d") return "diagnostics-toggle";
  }
  if (!hasOnlyShortcutModifier(event)) return null;
  if (key === "m") return "music-toggle";
  if (key === "=" || key === "+") return "zoom-in";
  if (key === "-") return "zoom-out";
  if (key === "0") return "zoom-reset";
  return null;
}
