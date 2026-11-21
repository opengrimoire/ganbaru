import type { FocusAccessStep } from "$lib/mobile/focus-access-steps";

/**
 * The system settings review of one Focus access step: none, settings opened for `step` while the app is still in
 * front, or the app left for those settings. Only a return after leaving finishes the review, so focus that comes
 * back before Android shows its settings does not.
 */
export type FocusAccessReview =
  | { phase: "none" }
  | { phase: "opening"; step: FocusAccessStep }
  | { phase: "away"; step: FocusAccessStep };

export type FocusAccessReviewEvent =
  /** Settings for `step` are opening; it replaces any review still waiting. */
  | { type: "opened"; step: FocusAccessStep }
  /** The settings failed to open. */
  | { type: "failed" }
  /** The app was hidden or lost focus. */
  | { type: "left" }
  /** The app became visible or regained focus. */
  | { type: "returned" };

export const NO_FOCUS_ACCESS_REVIEW: FocusAccessReview = { phase: "none" };

/**
 * Apply one review event. An event that changes nothing returns the same state object, so callers can tell an
 * ignored move by identity; a `returned` that yields a new state means the review of the previous step finished.
 */
export function transitionFocusAccessReview(state: FocusAccessReview, event: FocusAccessReviewEvent): FocusAccessReview {
  switch (event.type) {
    case "opened":
      return { phase: "opening", step: event.step };
    case "failed":
      return state.phase === "none" ? state : NO_FOCUS_ACCESS_REVIEW;
    case "left":
      return state.phase === "opening" ? { phase: "away", step: state.step } : state;
    case "returned":
      return state.phase === "away" ? NO_FOCUS_ACCESS_REVIEW : state;
  }
}
