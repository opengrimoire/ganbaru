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

/** Where the shown step stands: untouched, back from settings with the access still off, or with the access on. */
export type FocusStepOutcome = "idle" | "returned" | "granted";

/** Whether the shown step's settings can open, are opening, or failed to open on the last try. */
export type FocusStepOpen = "ready" | "opening" | "failed";

/** The access step on screen, by its position among the steps, and how far the person got with it. */
export interface FocusStepState {
  index: number;
  outcome: FocusStepOutcome;
  open: FocusStepOpen;
}

export type FocusStepEvent =
  /** The step at `index` came on screen; `granted` when Android already reports its access on. */
  | { type: "arrived"; index: number; granted: boolean }
  | { type: "openStarted" }
  | { type: "openFailed" }
  /** The open attempt finished, whether it failed or not. */
  | { type: "openEnded" }
  /** The person came back from the step's settings or answered its prompt; `granted` when the access is on now. */
  | { type: "settled"; granted: boolean };

export const FIRST_FOCUS_STEP: FocusStepState = { index: 0, outcome: "idle", open: "ready" };

/**
 * Apply one step event. Only one open runs at a time, and an arrival clears the failure of the step left behind
 * without cancelling an open still running; an ignored event returns the same state object.
 */
export function transitionFocusStep(state: FocusStepState, event: FocusStepEvent): FocusStepState {
  switch (event.type) {
    case "arrived":
      return {
        index: event.index,
        outcome: event.granted ? "granted" : "idle",
        open: state.open === "opening" ? "opening" : "ready",
      };
    case "openStarted":
      return state.open === "opening" ? state : { ...state, open: "opening" };
    case "openFailed":
      return state.open === "failed" ? state : { ...state, open: "failed" };
    case "openEnded":
      return state.open === "opening" ? { ...state, open: "ready" } : state;
    case "settled": {
      const outcome = event.granted ? "granted" : "returned";
      return state.outcome === outcome ? state : { ...state, outcome };
    }
  }
}
