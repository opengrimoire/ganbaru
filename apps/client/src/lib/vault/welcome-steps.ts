/** The welcome screens shown before vault setup, in order: the language, one per feature, then the app's own credentials. */
export const WELCOME_STEPS = ["language", "calendar", "blocking", "limits", "music", "projects", "about"] as const;

/** One welcome screen. */
export type WelcomeStep = (typeof WELCOME_STEPS)[number];

/** The welcome step at `index`, clamped to the first and last step. */
export function welcomeStepAt(index: number): WelcomeStep {
  const clamped = Number.isFinite(index) ? Math.min(WELCOME_STEPS.length - 1, Math.max(0, Math.trunc(index))) : 0;
  return WELCOME_STEPS[clamped];
}
