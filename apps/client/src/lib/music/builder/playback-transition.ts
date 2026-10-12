export type MusicBuilderPlaybackTransition =
  | "internal-navigation"
  | "explicit-playback"
  | "automation-boundary";

export type MusicBuilderPlaybackDecision =
  | "continue-review"
  | "release-review"
  | "supersede-review"
  | "no-review-action";

/** Decides how an active Review audition responds to a builder transition. */
export function musicBuilderPlaybackDecision(
  auditionActive: boolean,
  transition: MusicBuilderPlaybackTransition,
): MusicBuilderPlaybackDecision {
  if (!auditionActive) return "no-review-action";
  if (transition === "internal-navigation") return "continue-review";
  if (transition === "explicit-playback") return "release-review";
  return "supersede-review";
}
