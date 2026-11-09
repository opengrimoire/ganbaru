/**
 * The welcome screens' shared column: the header, the scene or list under it, and the navigation all line up on it, so
 * widening it widens the whole screen at once. The sizes are container queries on the welcome's own container.
 */
export const WELCOME_COLUMN_CLASS = "mx-auto w-full max-w-240 px-4 @min-[560px]:px-8 @min-[760px]:px-10";

/** The room above a welcome screen's header, kept short on wide screens so the header starts near the top. */
export const WELCOME_TOP_PADDING_CLASS = "pt-12 @min-[860px]:pt-14";

/** An onboarding screen's other choices, laid out above its navigation: stacked when narrow, side by side when there is room. */
export const ONBOARDING_SECONDARY_ROW_CLASS = "grid gap-2 @min-[480px]:auto-cols-fr @min-[480px]:grid-flow-col";

/** One of an onboarding screen's other choices, quieter than the primary action. */
export const ONBOARDING_SECONDARY_BUTTON_CLASS =
  "inline-flex min-h-11 min-w-0 items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-accent active:bg-accent disabled:cursor-not-allowed disabled:opacity-60";
