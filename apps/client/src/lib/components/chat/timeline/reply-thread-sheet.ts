import { bottomSheetHeights, type BottomSheetHeights } from "$lib/mobile/bottom-sheet";

/** Share of the full height a reply thread sheet opens at, so the channel edge stays in view above it. */
export const REPLY_THREAD_SHEET_PEEK_RATIO = 0.9;
/** How long the sheet takes to settle, slide in, or slide out, in milliseconds. */
export const REPLY_THREAD_SHEET_SETTLE_MS = 280;

/** Return the resting heights of a reply thread sheet whose fully open height is `fullHeight` CSS pixels. */
export function replyThreadSheetHeights(fullHeight: number): BottomSheetHeights {
  return bottomSheetHeights(fullHeight, 0, fullHeight * REPLY_THREAD_SHEET_PEEK_RATIO);
}

/** Where a vertical drag on the sheet started and what lies under it. */
export interface ReplyThreadSheetDragStart {
  /** Whether the drag started on the grabber or the thread header. */
  onHandle: boolean;
  /** Finger movement since the touch began, in CSS pixels; down is positive. */
  deltaY: number;
  /** Scroll offset of the thread entries when the drag is recognized. */
  scrollTop: number;
  /** Whether a nested scroller or slider under the finger keeps the drag, or the finger is on text entry. */
  innerControlKeeps: boolean;
}

/**
 * Return whether a vertical drag moves the sheet instead of the content under it. The handle always moves it; elsewhere
 * only a drag down from the top of the entries does, so the thread scrolls normally at every height.
 */
export function replyThreadSheetClaimsDrag(start: ReplyThreadSheetDragStart): boolean {
  if (start.onHandle) return true;
  if (start.innerControlKeeps) return false;
  return start.deltaY > 0 && start.scrollTop <= 0;
}
