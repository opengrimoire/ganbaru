import { BUILD_PLATFORM_PROFILE, platformHasCapability, type PlatformProfile } from "$lib/platform";

/** The welcome screens before a vault exists, in order: the language, then the choice of where data lives. */
export const WELCOME_SETUP_STEPS = ["language", "data"] as const;

/**
 * Every welcome screen a tour can show once the vault is open, in order: one per feature, the coding agents this device
 * can run, then the app's own credentials. {@link welcomeTourSteps} drops the screens a platform does not offer.
 */
const ALL_WELCOME_TOUR_STEPS = ["calendar", "blocking", "limits", "music", "tasks", "projects", "notes", "quickNotes", "chat", "agents", "about"] as const;

/** One welcome screen before the vault exists. */
export type WelcomeSetupStep = (typeof WELCOME_SETUP_STEPS)[number];

/** One welcome screen of the feature tour. */
export type WelcomeTourStep = (typeof ALL_WELCOME_TOUR_STEPS)[number];

/** One welcome screen. */
export type WelcomeStep = WelcomeSetupStep | WelcomeTourStep;

/** The two runs of welcome screens: `setup` before the vault exists and `tour` once it is open. */
export type WelcomeStage = "setup" | "tour";

/**
 * The tour screens `profile` shows, in order. The agents screen needs a device that runs coding agents itself, since
 * it checks which ones are installed and signed in there.
 */
export function welcomeTourSteps(profile: PlatformProfile): readonly WelcomeTourStep[] {
  const runsAgents = platformHasCapability(profile, "chat.local-execution");
  return ALL_WELCOME_TOUR_STEPS.filter((step) => step !== "agents" || runsAgents);
}

/** The tour screens of this build, in order. */
export const WELCOME_TOUR_STEPS = welcomeTourSteps(BUILD_PLATFORM_PROFILE);

/** The position of the data screen among the setup screens, where a vault that failed to open lands. */
export const WELCOME_DATA_STEP = WELCOME_SETUP_STEPS.indexOf("data");

/** The welcome screens of `stage`, in order, as `profile` shows them. */
export function welcomeStageSteps(stage: WelcomeStage, profile: PlatformProfile = BUILD_PLATFORM_PROFILE): readonly WelcomeStep[] {
  return stage === "setup" ? WELCOME_SETUP_STEPS : welcomeTourSteps(profile);
}

/** The position `index` clamped to the screens of `stage`, with fractions dropped and non-numbers at the start. */
export function clampWelcomeStep(stage: WelcomeStage, index: number, profile: PlatformProfile = BUILD_PLATFORM_PROFILE): number {
  const last = welcomeStageSteps(stage, profile).length - 1;
  return Number.isFinite(index) ? Math.min(last, Math.max(0, Math.trunc(index))) : 0;
}

/** The screen of `stage` at `index`, clamped to its first and last screen. */
export function welcomeStepAt(stage: WelcomeStage, index: number, profile: PlatformProfile = BUILD_PLATFORM_PROFILE): WelcomeStep {
  return welcomeStageSteps(stage, profile)[clampWelcomeStep(stage, index, profile)];
}

/** The position of the linking screen among the first-use screens: right after the setup screens, before the tour. */
const LINKING_SCREEN_POSITION = WELCOME_SETUP_STEPS.length;

/**
 * Where the screen of `stage` at `index` sits among every first-use screen, for the progress dots: the setup screens,
 * then the linking screen, which is a single screen, then the tour.
 */
export function welcomeProgress(
  stage: WelcomeStage | "linking",
  index = 0,
  profile: PlatformProfile = BUILD_PLATFORM_PROFILE,
): { index: number; count: number } {
  const position = stage === "setup"
    ? clampWelcomeStep(stage, index, profile)
    : stage === "linking"
      ? LINKING_SCREEN_POSITION
      : LINKING_SCREEN_POSITION + 1 + clampWelcomeStep(stage, index, profile);
  return { index: position, count: WELCOME_SETUP_STEPS.length + 1 + welcomeTourSteps(profile).length };
}
