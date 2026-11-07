import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

/**
 * Whether date and time pickers use the touch presentation: centered dialogs instead of anchored popovers,
 * touch-size calendar days, and the clock dial instead of the time list.
 */
export const TOUCH_PICKERS = BUILD_PLATFORM_PROFILE.shell === "mobile";
