/** The stages of the Android first-run onboarding, in the order they appear. */
export const MOBILE_ONBOARDING_STAGES = ["welcome", "vault", "permissions", "linking"] as const;

/** One stage of the Android first-run onboarding. */
export type MobileOnboardingStage = (typeof MOBILE_ONBOARDING_STAGES)[number];

/**
 * How full each progress segment is, from 0 to 1. Earlier stages are full, later ones empty, and the current stage
 * shows `fraction`, so a stage with several screens, such as the permissions, fills one screen at a time.
 */
export function onboardingSegmentFills(stage: MobileOnboardingStage, fraction = 1): number[] {
  const current = MOBILE_ONBOARDING_STAGES.indexOf(stage);
  const clamped = Number.isFinite(fraction) ? Math.min(1, Math.max(0, fraction)) : 0;
  return MOBILE_ONBOARDING_STAGES.map((_, index) => (index < current ? 1 : index === current ? clamped : 0));
}

/** The one-based position of a stage, for the screen reader progress text. */
export function onboardingStageNumber(stage: MobileOnboardingStage): number {
  return MOBILE_ONBOARDING_STAGES.indexOf(stage) + 1;
}
