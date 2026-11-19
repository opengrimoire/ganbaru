import { WELCOME_SETUP_STEPS, WELCOME_TOUR_STEPS } from "$lib/vault/welcome-steps";
import { FOCUS_ACCESS_STEP_ORDER } from "./focus-access-steps";

/** The stages of the Android first-run onboarding, in the order they appear. */
export const MOBILE_ONBOARDING_STAGES = ["setup", "tour", "permissions", "linking"] as const;

/** One stage of the Android first-run onboarding. */
export type MobileOnboardingStage = (typeof MOBILE_ONBOARDING_STAGES)[number];

/**
 * How many screens each stage can show: the language and vault screens, the feature tour, the permissions, and the
 * linking screen. Permissions count every step any phone may ask for, so the bar has the same length on every device,
 * and the steps a phone skips pass as done.
 */
const STAGE_SCREENS: Readonly<Record<MobileOnboardingStage, number>> = {
  setup: WELCOME_SETUP_STEPS.length,
  tour: WELCOME_TOUR_STEPS.length,
  permissions: FOCUS_ACCESS_STEP_ORDER.length,
  linking: 1,
};

/** Every screen of the onboarding, one progress segment each. */
export const MOBILE_ONBOARDING_SCREEN_COUNT = MOBILE_ONBOARDING_STAGES.reduce((total, stage) => total + STAGE_SCREENS[stage], 0);

/**
 * The zero-based position across the whole onboarding of `stage`'s screen at `screen`, which is clamped to the stage's
 * screens.
 */
export function onboardingScreenPosition(stage: MobileOnboardingStage, screen = 0): number {
  const stageIndex = MOBILE_ONBOARDING_STAGES.indexOf(stage);
  const offset = MOBILE_ONBOARDING_STAGES.slice(0, stageIndex).reduce((total, earlier) => total + STAGE_SCREENS[earlier], 0);
  const within = Number.isFinite(screen) ? Math.min(STAGE_SCREENS[stage] - 1, Math.max(0, Math.trunc(screen))) : 0;
  return offset + within;
}

/** Which progress segments are filled: every screen up to and including the current one. */
export function onboardingSegmentFills(stage: MobileOnboardingStage, screen = 0): boolean[] {
  const position = onboardingScreenPosition(stage, screen);
  return Array.from({ length: MOBILE_ONBOARDING_SCREEN_COUNT }, (_, index) => index <= position);
}
