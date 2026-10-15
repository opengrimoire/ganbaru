/** Tabs the phone bar shows beside its tabs button. */
export const MOBILE_TAB_BAR_CAPACITY = 4;

/** Largest open tab count the tabs button prints in full. */
const MOBILE_TAB_COUNT_LIMIT = 99;

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

/** Count the tabs button prints, capped so it fits the button, with numbers in the user's locale. */
export function mobileTabCountLabel(count: number, formatNumber: (value: number) => string): string {
  return count > MOBILE_TAB_COUNT_LIMIT ? `${formatNumber(MOBILE_TAB_COUNT_LIMIT)}+` : formatNumber(count);
}
