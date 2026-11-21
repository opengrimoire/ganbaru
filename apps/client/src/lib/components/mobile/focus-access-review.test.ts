import { describe, expect, it } from "vitest";
import {
  FIRST_FOCUS_STEP,
  NO_FOCUS_ACCESS_REVIEW,
  transitionFocusAccessReview,
  transitionFocusStep,
  type FocusAccessReview,
  type FocusStepEvent,
  type FocusStepState,
} from "./focus-access-review";

function opened(state: FocusAccessReview = NO_FOCUS_ACCESS_REVIEW): FocusAccessReview {
  return transitionFocusAccessReview(state, { type: "opened", step: "battery" });
}

describe("transitionFocusAccessReview", () => {
  it("finishes a review once the app comes back after leaving for settings", () => {
    const away = transitionFocusAccessReview(opened(), { type: "left" });
    expect(away).toEqual({ phase: "away", step: "battery" });
    expect(transitionFocusAccessReview(away, { type: "returned" })).toEqual(NO_FOCUS_ACCESS_REVIEW);
  });

  it("keeps waiting when the app regains focus before it ever left", () => {
    const opening = opened();
    expect(transitionFocusAccessReview(opening, { type: "returned" })).toBe(opening);
  });

  it("drops the review when the settings fail to open", () => {
    expect(transitionFocusAccessReview(opened(), { type: "failed" })).toEqual(NO_FOCUS_ACCESS_REVIEW);
  });

  it("replaces a waiting review when another step opens its settings", () => {
    const away = transitionFocusAccessReview(opened(), { type: "left" });
    expect(transitionFocusAccessReview(away, { type: "opened", step: "usage-access" }))
      .toEqual({ phase: "opening", step: "usage-access" });
  });

  it("ignores leaving and returning without a review", () => {
    expect(transitionFocusAccessReview(NO_FOCUS_ACCESS_REVIEW, { type: "left" })).toBe(NO_FOCUS_ACCESS_REVIEW);
    expect(transitionFocusAccessReview(NO_FOCUS_ACCESS_REVIEW, { type: "returned" })).toBe(NO_FOCUS_ACCESS_REVIEW);
    expect(transitionFocusAccessReview(NO_FOCUS_ACCESS_REVIEW, { type: "failed" })).toBe(NO_FOCUS_ACCESS_REVIEW);
    const away = transitionFocusAccessReview(opened(), { type: "left" });
    expect(transitionFocusAccessReview(away, { type: "left" })).toBe(away);
  });
});

function step(events: FocusStepEvent[], from: FocusStepState = FIRST_FOCUS_STEP): FocusStepState {
  return events.reduce(transitionFocusStep, from);
}

describe("transitionFocusStep", () => {
  it("arrives at a step marked by whether its access is already on", () => {
    expect(step([{ type: "arrived", index: 2, granted: false }])).toEqual({ index: 2, outcome: "idle", open: "ready" });
    expect(step([{ type: "arrived", index: 1, granted: true }])).toEqual({ index: 1, outcome: "granted", open: "ready" });
  });

  it("opens settings once at a time and is ready again after the open ends", () => {
    const opening = step([{ type: "openStarted" }]);
    expect(opening.open).toBe("opening");
    expect(transitionFocusStep(opening, { type: "openStarted" })).toBe(opening);
    expect(transitionFocusStep(opening, { type: "openEnded" })).toEqual(FIRST_FOCUS_STEP);
  });

  it("keeps a failed open until the next try or another step", () => {
    const failed = step([{ type: "openStarted" }, { type: "openFailed" }, { type: "openEnded" }]);
    expect(failed.open).toBe("failed");
    expect(transitionFocusStep(failed, { type: "openStarted" }).open).toBe("opening");
    expect(transitionFocusStep(failed, { type: "arrived", index: 1, granted: false }).open).toBe("ready");
    expect(transitionFocusStep(failed, { type: "openEnded" })).toBe(failed);
  });

  it("never cancels an open still running when another step arrives", () => {
    const opening = step([{ type: "openStarted" }]);
    expect(transitionFocusStep(opening, { type: "arrived", index: 1, granted: false }).open).toBe("opening");
  });

  it("records a return from settings with the access still off or now on", () => {
    expect(step([{ type: "settled", granted: false }]).outcome).toBe("returned");
    const granted = step([{ type: "settled", granted: true }]);
    expect(granted.outcome).toBe("granted");
    expect(transitionFocusStep(granted, { type: "settled", granted: true })).toBe(granted);
  });

  it("keeps a prompt answered during the open as the step's outcome", () => {
    const answered = step([{ type: "openStarted" }, { type: "settled", granted: true }, { type: "openEnded" }]);
    expect(answered).toEqual({ index: 0, outcome: "granted", open: "ready" });
  });
});
