import { createContext } from "svelte";
import { cubicInOut, cubicOut } from "svelte/easing";
import type { SlideParams } from "svelte/transition";
import { prefersReducedMotion } from "$lib/utils/smooth-swap";

/** How long a section of the floating desktop panel slides open or closed, in milliseconds. */
const PANEL_SECTION_SLIDE_MS = 180;
/**
 * How long a section of the mobile event sheet slides open or closed, in milliseconds. The sheet rises to make room
 * for an opening section over the same time and curve, so both read as one motion.
 */
export const SHEET_SECTION_SLIDE_MS = 360;
/** CSS curve that matches `cubicInOut`, for the sheet's rise beside a sliding section. */
export const SHEET_SECTION_SLIDE_CSS_EASING = "cubic-bezier(0.65, 0, 0.35, 1)";

const [getSectionLayout, setSectionLayout] = createContext<() => "panel" | "sheet">();

/** Let the event panel's sections know whether it shows as a floating panel or as a mobile bottom sheet. */
export function provideEventPanelSectionLayout(layout: () => "panel" | "sheet"): void {
  setSectionLayout(layout);
}

/**
 * Return the slide parameters for a section body of the event panel. Call it while the section initializes; the
 * returned function reads the current layout each time the section opens or closes.
 */
export function eventPanelSectionSlide(): () => SlideParams {
  const layout = getSectionLayout();
  return () => {
    if (prefersReducedMotion()) return { duration: 0 };
    return layout() === "sheet"
      ? { duration: SHEET_SECTION_SLIDE_MS, easing: cubicInOut }
      : { duration: PANEL_SECTION_SLIDE_MS, easing: cubicOut };
  };
}
