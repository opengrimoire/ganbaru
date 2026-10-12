/** Edges of a scroll container that fade because more content lies beyond them. */
export type ScrollEdgeFade = "none" | "top" | "bottom" | "both";

/** Scroll offsets within this many pixels of an end count as reaching it, absorbing fractional layout. */
const SCROLL_EDGE_TOLERANCE = 1;

/** Resolve which edges fade from a container's vertical scroll metrics. */
export function scrollEdgeFade(metrics: { scrollTop: number; scrollHeight: number; clientHeight: number }): ScrollEdgeFade {
  const maxScrollTop = metrics.scrollHeight - metrics.clientHeight;
  if (maxScrollTop <= SCROLL_EDGE_TOLERANCE) return "none";
  const up = metrics.scrollTop > SCROLL_EDGE_TOLERANCE;
  const down = metrics.scrollTop < maxScrollTop - SCROLL_EDGE_TOLERANCE;
  if (up && down) return "both";
  if (up) return "top";
  return down ? "bottom" : "none";
}

/**
 * Svelte action that fades a scroll container's top or bottom edge while content lies beyond it.
 * It sets `data-scroll-fade` on the node, which the global stylesheet turns into a mask, and refreshes on scroll, on size changes of the node or its children, and when children are added or removed.
 */
export function scrollEdgeFadeAction(node: HTMLElement) {
  const refresh = () => {
    const fade = scrollEdgeFade(node);
    if (fade === "none") node.removeAttribute("data-scroll-fade");
    else node.setAttribute("data-scroll-fade", fade);
  };
  // Resize observations arrive after layout and before paint, so refreshing there shows the fade in the same frame as the size change.
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(refresh);
  observer?.observe(node);
  for (const child of node.children) observer?.observe(child);
  // Children added later, such as rows that load after opening, change the scroll height without resizing the capped container.
  const mutations = typeof MutationObserver === "undefined" ? null : new MutationObserver((records) => {
    for (const record of records) for (const added of record.addedNodes) if (added instanceof Element) observer?.observe(added);
    refresh();
  });
  mutations?.observe(node, { childList: true });
  node.addEventListener("scroll", refresh, { passive: true });
  refresh();
  return {
    destroy() {
      observer?.disconnect();
      mutations?.disconnect();
      node.removeEventListener("scroll", refresh);
      node.removeAttribute("data-scroll-fade");
    },
  };
}
