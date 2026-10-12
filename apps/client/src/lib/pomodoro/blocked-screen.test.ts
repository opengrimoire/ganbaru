import { describe, expect, it } from "vitest";
import {
  delayMsUntil,
  elapsedSecondsSince,
  formatBreakEndEarlyShortcut,
  formatBreakExtensionHint,
  formatBreakExtensionShortcut,
  formatBlockedScreenDateTime,
  formatBlockedScreenDuration,
  isBlockedScreenAcknowledgementState,
  isPomodoroCompletionScreenState,
  nextIntervalTargetAfter,
  parsePomodoroOverlayBlockerAction,
  parsePomodoroBlockedScreenState,
  pomodoroBlockedScreenPalette,
  pomodoroBlockedScreenCopy,
  pomodoroBlockedScreenStateFromOverlayKind,
  remainingSecondsUntil,
  shouldScheduleIdleAlert,
  shouldShowBlockedScreenDateTime,
} from "./blocked-screen";

describe("blocked pomodoro screen helpers", () => {
  it("formats minute timers", () => {
    expect(formatBlockedScreenDuration(0)).toBe("00:00");
    expect(formatBlockedScreenDuration(59)).toBe("00:59");
    expect(formatBlockedScreenDuration(65)).toBe("01:05");
  });

  it("formats hour timers only when needed", () => {
    expect(formatBlockedScreenDuration(3600)).toBe("01:00:00");
    expect(formatBlockedScreenDuration(3723)).toBe("01:02:03");
  });

  it("clamps invalid negative timers", () => {
    expect(formatBlockedScreenDuration(-12)).toBe("00:00");
  });

  it("formats date and time through Intl", () => {
    const date = new Date("2026-05-31T13:45:00Z");
    const label = formatBlockedScreenDateTime(date, "en-US", { timeZone: "UTC" });
    expect(label).toBe("Sunday, May 31, 2026 | 1:45 PM");
  });

  it("shows date and time except on the break finished screen", () => {
    expect(shouldShowBlockedScreenDateTime("break_countdown")).toBe(true);
    expect(shouldShowBlockedScreenDateTime("idle")).toBe(true);
    expect(shouldShowBlockedScreenDateTime("idle_failed")).toBe(true);
    expect(shouldShowBlockedScreenDateTime("break_finished")).toBe(false);
    expect(shouldShowBlockedScreenDateTime("event_finished")).toBe(false);
    expect(shouldShowBlockedScreenDateTime("day_complete")).toBe(false);
    expect(shouldShowBlockedScreenDateTime("workweek_complete")).toBe(false);
  });

  it("uses distinct state palettes", () => {
    expect(pomodoroBlockedScreenPalette("idle")).toMatchObject({
      background: "#A33728",
      mainText: "#F9D573",
    });
    expect(pomodoroBlockedScreenPalette("break_countdown")).toMatchObject({
      background: "#035B33",
      mainText: "#FFFFFF",
    });
    expect(pomodoroBlockedScreenPalette("break_finished")).toMatchObject({
      background: "#EEBA04",
      mainText: "#0D0502",
    });
    expect(pomodoroBlockedScreenPalette("event_finished")).toMatchObject({
      background: "#0E7490",
      mainText: "#FFFFFF",
    });
    expect(pomodoroBlockedScreenPalette("day_complete")).toMatchObject({
      background: "#EEBA04",
      mainText: "#0D0502",
    });
    expect(pomodoroBlockedScreenPalette("workweek_complete")).toMatchObject({
      background: "#1D4ED8",
      mainText: "#FFFFFF",
    });
  });

  it("identifies acknowledgement and completion screens", () => {
    expect(isBlockedScreenAcknowledgementState("break_finished")).toBe(true);
    expect(isBlockedScreenAcknowledgementState("event_finished")).toBe(true);
    expect(isBlockedScreenAcknowledgementState("day_complete")).toBe(true);
    expect(isBlockedScreenAcknowledgementState("workweek_complete")).toBe(true);
    expect(isBlockedScreenAcknowledgementState("idle")).toBe(false);
    expect(isPomodoroCompletionScreenState("event_finished")).toBe(true);
    expect(isPomodoroCompletionScreenState("break_finished")).toBe(false);
  });

  it("maps overlay query state to blocked screen state", () => {
    expect(parsePomodoroBlockedScreenState("idle_failed")).toBe("idle_failed");
    expect(parsePomodoroBlockedScreenState("break_finished")).toBe("break_finished");
    expect(parsePomodoroBlockedScreenState("event_finished")).toBe("event_finished");
    expect(parsePomodoroBlockedScreenState("day_complete")).toBe("day_complete");
    expect(parsePomodoroBlockedScreenState("workweek_complete")).toBe("workweek_complete");
    expect(parsePomodoroBlockedScreenState("unknown")).toBe("break_countdown");
    expect(pomodoroBlockedScreenStateFromOverlayKind("idle")).toBe("idle");
    expect(pomodoroBlockedScreenStateFromOverlayKind("break")).toBe("break_countdown");
    expect(pomodoroBlockedScreenStateFromOverlayKind("completion")).toBe("event_finished");
  });

  it("parses blocker actions from unknown event payloads", () => {
    expect(parsePomodoroOverlayBlockerAction({ kind: "pointer" })).toEqual({
      kind: "pointer",
    });
    expect(
      parsePomodoroOverlayBlockerAction({
        kind: "keydown",
        code: "Space",
        key: " ",
        ctrlKey: true,
        metaKey: true,
      }),
    ).toEqual({
      kind: "keydown",
      code: "Space",
      key: " ",
      ctrlKey: true,
      metaKey: true,
      shiftKey: false,
    });
    expect(parsePomodoroOverlayBlockerAction({ kind: "keydown", code: "Space" })).toBeNull();
    expect(parsePomodoroOverlayBlockerAction(null)).toBeNull();
  });

  it("computes remaining seconds from an absolute target", () => {
    expect(remainingSecondsUntil(10_000, 8_100)).toBe(2);
    expect(remainingSecondsUntil(10_000, 10_000)).toBe(0);
    expect(remainingSecondsUntil(10_000, 11_000)).toBe(0);
  });

  it("computes elapsed seconds from an absolute start", () => {
    expect(elapsedSecondsSince(10_000, 10_999)).toBe(0);
    expect(elapsedSecondsSince(10_000, 11_000)).toBe(1);
    expect(elapsedSecondsSince(10_000, 9_000)).toBe(0);
  });

  it("computes non-negative delay to a target", () => {
    expect(delayMsUntil(10_000, 9_250)).toBe(750);
    expect(delayMsUntil(10_000, 10_000)).toBe(0);
    expect(delayMsUntil(10_000, 11_000)).toBe(0);
  });

  it("skips missed interval targets after a delayed callback", () => {
    expect(nextIntervalTargetAfter(10_000, 10_000, 10_500)).toBe(20_000);
    expect(nextIntervalTargetAfter(10_000, 10_000, 25_000)).toBe(30_000);
    expect(nextIntervalTargetAfter(10_000, 10_000, 30_000)).toBe(40_000);
  });

  it("keeps the exact focus failure boundary for the failure sound", () => {
    expect(shouldScheduleIdleAlert(59_999, 60_000)).toBe(true);
    expect(shouldScheduleIdleAlert(60_000, 60_000)).toBe(false);
    expect(shouldScheduleIdleAlert(60_001, 60_000)).toBe(false);
  });

  it("keeps break extension hint stable until the cap is reached", () => {
    expect(formatBreakExtensionHint(0, 3, "Linux x86_64")).toBe(
      "Press Ctrl + Shift + Space to extend the break",
    );
    expect(formatBreakExtensionHint(1, 3, "MacIntel")).toBe(
      "Press Cmd + Shift + Space to extend the break",
    );
    expect(formatBreakExtensionHint(3, 3)).toBeNull();
    expect(formatBreakExtensionHint(0, null)).toBeNull();
  });

  it("formats the break extension shortcut for the platform", () => {
    expect(formatBreakExtensionShortcut("Linux x86_64")).toBe("Ctrl + Shift + Space");
    expect(formatBreakExtensionShortcut("MacIntel")).toBe("Cmd + Shift + Space");
  });

  it("formats configurable break ending Esc hints", () => {
    expect(formatBreakEndEarlyShortcut(0, 10)).toBe("10x Esc");
    expect(formatBreakEndEarlyShortcut(4, 10)).toBe("6x Esc");
    expect(formatBreakEndEarlyShortcut(0, 1)).toBe("1x Esc");
    expect(formatBreakEndEarlyShortcut(0, null)).toBeNull();
  });

  it("uses direct break countdown action copy", () => {
    const copy = pomodoroBlockedScreenCopy("break_countdown");
    expect(copy.primaryHint?.label).toBe("extend the break");
    expect(copy.secondaryHint?.key).toBe("10x Esc");
    expect(copy.secondaryHint?.label).toBe("end your break now");
  });

  it("uses restart copy for failed idle sessions", () => {
    const copy = pomodoroBlockedScreenCopy("idle_failed");
    expect(copy.title).toBe("Focus session failed");
    expect(copy.status).toBeNull();
    expect(copy.primaryHint?.label).toBe("restart focus");
    expect(copy.secondaryHint).toBeNull();
  });

  it("does not expose a stop action on idle screens", () => {
    expect(pomodoroBlockedScreenCopy("idle").status).toBeNull();
    expect(pomodoroBlockedScreenCopy("idle").secondaryHint).toBeNull();
    expect(pomodoroBlockedScreenCopy("idle_failed").secondaryHint).toBeNull();
  });

  it("uses generic break completion acknowledgement copy", () => {
    const copy = pomodoroBlockedScreenCopy("break_finished");
    expect(copy.title).toBe("Break complete");
    expect(copy.subtitle).toBe("press any key to continue");
  });

  it("uses terminal completion copy", () => {
    expect(pomodoroBlockedScreenCopy("event_finished").title).toBe("Event finished");
    expect(pomodoroBlockedScreenCopy("day_complete").title).toBe("Day completed");
    expect(pomodoroBlockedScreenCopy("workweek_complete").title).toBe(
      "Workweek completed",
    );
  });
});
