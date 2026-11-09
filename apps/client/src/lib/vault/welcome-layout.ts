/**
 * The welcome screens' shared column: the header, the scene or list under it, and the navigation all line up on it, so
 * widening it widens the whole screen at once. The sizes are container queries on the welcome's own container.
 */
export const WELCOME_COLUMN_CLASS = "mx-auto w-full max-w-240 px-4 @min-[560px]:px-8 @min-[760px]:px-10";

/** The room above a welcome screen's header, kept short on wide screens so the header starts near the top. */
export const WELCOME_TOP_PADDING_CLASS = "pt-12 @min-[860px]:pt-14";

/** A welcome screen's quiet text actions under its header, such as the license or another data folder. */
export const WELCOME_LINK_CLASS =
  "inline-flex min-h-10 items-center gap-1.5 text-sm font-medium text-foreground underline-offset-4 hover:underline disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:no-underline";

/** One of an onboarding screen's other choices, quieter than the primary action. */
export const ONBOARDING_SECONDARY_BUTTON_CLASS =
  "inline-flex min-h-11 min-w-0 items-center justify-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-accent active:bg-accent disabled:cursor-not-allowed disabled:opacity-60";
