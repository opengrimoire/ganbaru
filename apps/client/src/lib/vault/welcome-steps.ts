/** The welcome screens before a vault exists, in order: the language, then the choice of where data lives. */
export const WELCOME_SETUP_STEPS = ["language", "data"] as const;

/** The welcome screens once the vault is open, in order: one per feature, then the app's own credentials. */
export const WELCOME_TOUR_STEPS = ["calendar", "blocking", "limits", "music", "tasks", "projects", "notes", "quickNotes", "chat", "about"] as const;

/** One welcome screen before the vault exists. */
export type WelcomeSetupStep = (typeof WELCOME_SETUP_STEPS)[number];

/** One welcome screen of the feature tour. */
export type WelcomeTourStep = (typeof WELCOME_TOUR_STEPS)[number];

/** One welcome screen. */
export type WelcomeStep = WelcomeSetupStep | WelcomeTourStep;

/** The two runs of welcome screens: `setup` before the vault exists and `tour` once it is open. */
export type WelcomeStage = "setup" | "tour";

/** The position of the data screen among the setup screens, where a vault that failed to open lands. */
export const WELCOME_DATA_STEP = WELCOME_SETUP_STEPS.indexOf("data");

/** The welcome screens of `stage`, in order. */
export function welcomeStageSteps(stage: WelcomeStage): readonly WelcomeStep[] {
  return stage === "setup" ? WELCOME_SETUP_STEPS : WELCOME_TOUR_STEPS;
}

/** The position `index` clamped to the screens of `stage`, with fractions dropped and non-numbers at the start. */
export function clampWelcomeStep(stage: WelcomeStage, index: number): number {
  const last = welcomeStageSteps(stage).length - 1;
  return Number.isFinite(index) ? Math.min(last, Math.max(0, Math.trunc(index))) : 0;
}

/** The screen of `stage` at `index`, clamped to its first and last screen. */
export function welcomeStepAt(stage: WelcomeStage, index: number): WelcomeStep {
  return welcomeStageSteps(stage)[clampWelcomeStep(stage, index)];
}

/** Where the screen of `stage` at `index` sits among every welcome screen, setup first, for the progress dots. */
export function welcomeProgress(stage: WelcomeStage, index: number): { index: number; count: number } {
  const offset = stage === "setup" ? 0 : WELCOME_SETUP_STEPS.length;
  return {
    index: offset + clampWelcomeStep(stage, index),
    count: WELCOME_SETUP_STEPS.length + WELCOME_TOUR_STEPS.length,
  };
}
