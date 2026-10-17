import type { EventColor } from "$lib/calendar/types";
import { getPastEventColor, type ColorEntry } from "$lib/calendar/utils";
import { contrastRatio } from "$lib/color/math";
import type { Theme } from "$lib/themes";

/** Select the black or white foreground with the highest WCAG contrast. */
export function mostContrastingQuickNoteText(background: string): "#000000" | "#ffffff" {
  return contrastRatio(background, "#000000") >= contrastRatio(background, "#ffffff")
    ? "#000000"
    : "#ffffff";
}

/** Resolve a muted quick note color with a maximum-contrast foreground. */
export function getQuickNoteColor(color: EventColor, theme: Theme): ColorEntry {
  const muted = getPastEventColor(color, theme);
  return {
    bg: muted.bg,
    text: mostContrastingQuickNoteText(muted.bg),
  };
}
