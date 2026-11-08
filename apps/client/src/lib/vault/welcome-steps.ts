/** The welcome screens shown before vault setup, in order: the language, one per feature, then the app's own credentials. */
export const WELCOME_STEPS = ["language", "calendar", "blocking", "limits", "music", "projects", "about"] as const;

/** One welcome screen. */
export type WelcomeStep = (typeof WELCOME_STEPS)[number];

/** The welcome step at `index`, clamped to the first and last step. */
export function welcomeStepAt(index: number): WelcomeStep {
  const clamped = Number.isFinite(index) ? Math.min(WELCOME_STEPS.length - 1, Math.max(0, Math.trunc(index))) : 0;
  return WELCOME_STEPS[clamped];
}

/** How far through the welcome screens the user is while the step at `index` shows, from 0 to 1. */
export function welcomeProgress(index: number): number {
  return (WELCOME_STEPS.indexOf(welcomeStepAt(index)) + 1) / WELCOME_STEPS.length;
}
