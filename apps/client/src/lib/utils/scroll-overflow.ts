/** Scroll ranges within this many pixels count as no scrolling, absorbing fractional layout. */
export const SCROLL_OVERFLOW_TOLERANCE = 1;

/** Whether a container's content is taller than its visible area. */
export function hasVerticalOverflow(metrics: { scrollHeight: number; clientHeight: number }): boolean {
  return metrics.scrollHeight - metrics.clientHeight > SCROLL_OVERFLOW_TOLERANCE;
}

/**
 * Calls `refresh` now and whenever the scroll metrics of `node` may have changed: on size changes of the node or its
 * children, and when children are added or removed. Returns a function that stops observing.
 */
export function observeScrollContent(node: HTMLElement, refresh: () => void): () => void {
  // Resize observations arrive after layout and before paint, so refreshing there updates in the same frame as the size change.
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(refresh);
  observer?.observe(node);
  for (const child of node.children) observer?.observe(child);
  // Children added later, such as rows that load after opening, change the scroll height without resizing the capped container.
  const mutations = typeof MutationObserver === "undefined" ? null : new MutationObserver((records) => {
    for (const record of records) for (const added of record.addedNodes) if (added instanceof Element) observer?.observe(added);
    refresh();
  });
  mutations?.observe(node, { childList: true });
  refresh();
  return () => {
    observer?.disconnect();
    mutations?.disconnect();
  };
}

/**
 * Svelte action that sets `data-scroll-overflow` on a vertical scroll container while its content overflows, so the
 * stylesheet can keep rows clear of a visible scrollbar.
 */
export function scrollOverflowAction(node: HTMLElement) {
  const stop = observeScrollContent(node, () => {
    node.toggleAttribute("data-scroll-overflow", hasVerticalOverflow(node));
  });
  return {
    destroy() {
      stop();
      node.removeAttribute("data-scroll-overflow");
    },
  };
}
