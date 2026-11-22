import { describe, expect, it } from "vitest";
import {
  IDLE_PAGE_HISTORY_ACTION,
  isCurrentPageHistoryAction,
  transitionPageHistoryAction,
  type NotesPageHistoryAction,
} from "./page-history-state";

const running: NotesPageHistoryAction = { phase: "running", kind: "restore", pageId: "a", token: 1 };

describe("page history action transitions", () => {
  it("starts from idle or after a failure", () => {
    expect(transitionPageHistoryAction(IDLE_PAGE_HISTORY_ACTION, { type: "started", kind: "restore", pageId: "a", token: 1 })).toEqual(running);
    const failed: NotesPageHistoryAction = { phase: "failed", kind: "copy", pageId: "a", error: "Copy failed" };
    expect(transitionPageHistoryAction(failed, { type: "started", kind: "restore", pageId: "a", token: 2 })).toEqual({
      ...running,
      token: 2,
    });
  });

  it("refuses a second start while an action runs", () => {
    expect(transitionPageHistoryAction(running, { type: "started", kind: "copy", pageId: "a", token: 2 })).toBe(running);
  });

  it("finishes or fails only the running token", () => {
    expect(transitionPageHistoryAction(running, { type: "finished", token: 1 })).toBe(IDLE_PAGE_HISTORY_ACTION);
    expect(transitionPageHistoryAction(running, { type: "finished", token: 2 })).toBe(running);
    expect(transitionPageHistoryAction(running, { type: "failed", token: 2, error: "Late" })).toBe(running);
    expect(transitionPageHistoryAction(running, { type: "failed", token: 1, error: "Restore failed" })).toEqual({
      phase: "failed",
      kind: "restore",
      pageId: "a",
      error: "Restore failed",
    });
  });

  it("ignores the end of an action after a reset", () => {
    const reset = transitionPageHistoryAction(running, { type: "reset" });
    expect(reset).toBe(IDLE_PAGE_HISTORY_ACTION);
    expect(transitionPageHistoryAction(reset, { type: "failed", token: 1, error: "Late" })).toBe(reset);
    expect(transitionPageHistoryAction(reset, { type: "finished", token: 1 })).toBe(reset);
    expect(transitionPageHistoryAction(IDLE_PAGE_HISTORY_ACTION, { type: "reset" })).toBe(IDLE_PAGE_HISTORY_ACTION);
  });

  it("names the current action only while its token runs", () => {
    expect(isCurrentPageHistoryAction(running, 1)).toBe(true);
    expect(isCurrentPageHistoryAction(running, 2)).toBe(false);
    expect(isCurrentPageHistoryAction(IDLE_PAGE_HISTORY_ACTION, 1)).toBe(false);
  });
});
