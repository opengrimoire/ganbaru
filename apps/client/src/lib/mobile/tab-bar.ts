import type { WorkspaceTabPlacement } from "$lib/workspace-tabs/model";

/** Tabs the phone bar shows beside its tabs button. */
export const MOBILE_TAB_BAR_CAPACITY = 4;

/** Largest count of tabs past the bar that the tabs button prints in full. */
const MOBILE_TAB_OVERFLOW_LIMIT = 99;

/** One reorder step, applied in sequence with the workspace tab `move`. */
export interface MobileTabMove {
  id: string;
  targetId: string;
  placement: WorkspaceTabPlacement;
}

/**
 * Tabs the phone bar shows, in tab order: the first ones that fit, with the active tab taking the last slot
 * when it sits past them, so the bar always shows where the user is.
 */
export function mobileTabBarSlots<T extends { id: string }>(
  tabs: readonly T[],
  activeTabId: string,
  capacity: number = MOBILE_TAB_BAR_CAPACITY,
): T[] {
  if (capacity <= 0) return [];
  const leading = tabs.slice(0, capacity);
  if (leading.some((tab) => tab.id === activeTabId)) return leading;
  const active = tabs.find((tab) => tab.id === activeTabId);
  return active ? [...leading.slice(0, capacity - 1), active] : leading;
}

/**
 * What the tabs button shows: an add mark while the bar shows every tab, or how many tabs sit past the bar, capped
 * so it fits the button, with numbers in the user's locale.
 */
export function mobileTabsButtonBadge(
  tabCount: number,
  shownCount: number,
  formatNumber: (value: number) => string,
): { kind: "add" } | { kind: "more"; label: string } {
  const hidden = tabCount - shownCount;
  if (hidden <= 0) return { kind: "add" };
  return { kind: "more", label: `+${formatNumber(Math.min(hidden, MOBILE_TAB_OVERFLOW_LIMIT))}` };
}

/**
 * Moves that put the bar tab `id` into the bar slot `targetIndex`, leaving the bar in the order the drag showed. When
 * the active tab borrows the last slot from past the bar, it first moves into that slot, so the tabs the bar shows are
 * the leading tabs and the dragged one cannot land past the bar.
 */
export function mobileTabBarDropMoves(
  tabs: readonly { id: string }[],
  slots: readonly { id: string }[],
  id: string,
  targetIndex: number,
): MobileTabMove[] {
  const source = slots.findIndex((slot) => slot.id === id);
  const target = slots[targetIndex];
  if (source < 0 || !target || targetIndex === source) return [];
  const moves: MobileTabMove[] = [];
  const last = slots[slots.length - 1];
  if (last && tabs[slots.length - 1]?.id !== last.id) {
    const before = slots[slots.length - 2];
    const first = tabs[0];
    if (before) moves.push({ id: last.id, targetId: before.id, placement: "after" });
    else if (first && first.id !== last.id) moves.push({ id: last.id, targetId: first.id, placement: "before" });
  }
  moves.push({ id, targetId: target.id, placement: targetIndex > source ? "after" : "before" });
  return moves;
}
