/**
 * The welcome screens' shared column: the header, the scene or list under it, and the navigation all line up on it, so
 * widening it widens the whole screen at once. The sizes are container queries on the welcome's own container.
 */
export const WELCOME_COLUMN_CLASS = "mx-auto w-full max-w-240 px-4 @min-[560px]:px-8 @min-[760px]:px-10";

/** The room above a welcome screen's header, kept short on wide screens so the header starts near the top. */
export const WELCOME_TOP_PADDING_CLASS = "pt-12 @min-[860px]:pt-14";
