import { hasVerticalOverflow, observeScrollContent, SCROLL_OVERFLOW_TOLERANCE } from "./scroll-overflow";

/** Edges of a scroll container that fade because more content lies beyond them. */
export type ScrollEdgeFade = "none" | "top" | "bottom" | "both";

/** Scroll offsets within this many pixels of an end count as reaching it, absorbing fractional layout. */
const SCROLL_EDGE_TOLERANCE = SCROLL_OVERFLOW_TOLERANCE;

/** Resolve which edges fade from a container's vertical scroll metrics. */
export function scrollEdgeFade(metrics: { scrollTop: number; scrollHeight: number; clientHeight: number }): ScrollEdgeFade {
  if (!hasVerticalOverflow(metrics)) return "none";
  const maxScrollTop = metrics.scrollHeight - metrics.clientHeight;
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
  const stop = observeScrollContent(node, refresh);
  node.addEventListener("scroll", refresh, { passive: true });
  return {
    destroy() {
      stop();
      node.removeEventListener("scroll", refresh);
      node.removeAttribute("data-scroll-fade");
    },
  };
}
