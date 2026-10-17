export interface OrganizationalScrollMetrics {
  scrollHeight: number;
  clientHeight: number;
  scrollTop: number;
}

export const ORGANIZATIONAL_SCROLL_FOLLOW_THRESHOLD = 24;

/** Reports whether an organizational message surface should remain pinned to its end. */
export function organizationalScrollFollowsEnd(
  metrics: OrganizationalScrollMetrics,
  threshold = ORGANIZATIONAL_SCROLL_FOLLOW_THRESHOLD,
): boolean {
  const maximumScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  return maximumScrollTop - Math.max(0, metrics.scrollTop) <= Math.max(0, threshold);
}
