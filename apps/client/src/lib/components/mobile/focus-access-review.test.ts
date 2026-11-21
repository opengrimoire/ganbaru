import { describe, expect, it } from "vitest";
import {
  NO_FOCUS_ACCESS_REVIEW,
  transitionFocusAccessReview,
  type FocusAccessReview,
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
