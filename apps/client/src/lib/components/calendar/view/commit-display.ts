import type { CalendarEvent } from "$lib/calendar/types";

/** The projection one committing operation holds on screen until its commit and refresh end. */
export interface CommitDisplayHold {
  frozenEvents: CalendarEvent[];
}

/**
 * The holds of the panel save and the deletion, each owned by its controller. Keeping them apart means the end of
 * one operation clears only its own hold, never the hold of an operation still committing beside it.
 */
export interface CommitDisplaySlots {
  save: CommitDisplayHold | null;
  delete: CommitDisplayHold | null;
}

/** How the view paints while operations commit. */
export interface CommitDisplay {
  /** Whether the editor panel is hidden; only a deletion hides it. */
  panelHidden: boolean;
  /** Whether the editing glow is hidden; only a save hides it. */
  suppressGlow: boolean;
  suppressPreview: boolean;
  frozenEvents: CalendarEvent[] | null;
}

export const NO_COMMIT_DISPLAY: CommitDisplaySlots = { save: null, delete: null };

/** Merge the holds; a deletion's projection wins because it removes the events a save would still show. */
export function commitDisplay(slots: CommitDisplaySlots): CommitDisplay {
  return {
    panelHidden: slots.delete !== null,
    suppressGlow: slots.save !== null,
    suppressPreview: slots.save !== null || slots.delete !== null,
    frozenEvents: slots.delete?.frozenEvents ?? slots.save?.frozenEvents ?? null,
  };
}
