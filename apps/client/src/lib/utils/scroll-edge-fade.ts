import { cssLengthToPixels } from "./css-length";
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

/** Which edges a container lets fade. */
export type ScrollEdgeFadeEdges = "both" | "bottom";

/** Keep only the fades `edges` allows, so a bottom-only container never fades its top. */
export function limitScrollEdgeFade(fade: ScrollEdgeFade, edges: ScrollEdgeFadeEdges): ScrollEdgeFade {
  if (edges === "both") return fade;
  return fade === "bottom" || fade === "both" ? "bottom" : "none";
}

/** Present on every container the scroll edge fade action manages, so other code leaves its fade alone. */
export const SCROLL_FADE_OWNER_ATTRIBUTE = "data-scroll-fade-owner";

/** Pixel lengths for the top and bottom edge of a scroll container. */
export interface ScrollFadeSizes {
  top: number;
  bottom: number;
}

/** Custom properties holding the full fade size of each edge, which the stylesheet defines per shell. */
const FADE_SIZE_PROPERTIES = { top: "--scroll-fade-size", bottom: "--scroll-fade-bottom-size" } as const;

/** Custom properties the action writes with the distance left to scroll toward each edge. */
const FADE_DISTANCE_PROPERTIES = { top: "--scroll-fade-top-distance", bottom: "--scroll-fade-bottom-distance" } as const;

/** Full fade sizes of `element`'s edges in pixels, or `fallback` for an edge whose size cannot be read. */
export function scrollFadeSizes(element: HTMLElement, fallback: number): ScrollFadeSizes {
  const style = getComputedStyle(element);
  return {
    top: cssLengthToPixels(style.getPropertyValue(FADE_SIZE_PROPERTIES.top), element, fallback),
    bottom: cssLengthToPixels(style.getPropertyValue(FADE_SIZE_PROPERTIES.bottom), element, fallback),
  };
}

/**
 * Distance left to scroll toward each edge that `fade` shows, capped at that edge's fade size, and zero for an edge
 * that does not fade. The stylesheet sizes each fade by this distance, so a fade grows and shrinks with the scroll
 * position instead of switching on and off when an end is reached or left.
 */
export function scrollEdgeFadeDistances(
  metrics: { scrollTop: number; scrollHeight: number; clientHeight: number },
  fade: ScrollEdgeFade,
  sizes: ScrollFadeSizes,
): ScrollFadeSizes {
  const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  const fadesTop = fade === "top" || fade === "both";
  const fadesBottom = fade === "bottom" || fade === "both";
  return {
    top: fadesTop ? Math.round(Math.min(Math.max(0, metrics.scrollTop), sizes.top)) : 0,
    bottom: fadesBottom ? Math.round(Math.min(Math.max(0, maxScrollTop - metrics.scrollTop), sizes.bottom)) : 0,
  };
}

/**
 * Svelte action that fades a scroll container's top or bottom edge while content lies beyond it.
 * It sets `data-scroll-fade` on the node, which the global stylesheet turns into a mask, limited to `edges` (both by
 * default), and writes the distance left to scroll toward each fading edge so the mask follows the scroll position.
 * It refreshes on scroll, on size changes of the node or its children, and when children are added or removed.
 */
export function scrollEdgeFadeAction(node: HTMLElement, options: { edges?: ScrollEdgeFadeEdges } = {}) {
  let edges = options.edges ?? "both";
  // An unreadable size leaves the distance uncapped; the stylesheet caps it again at the fade size.
  let sizes: ScrollFadeSizes = { top: Number.POSITIVE_INFINITY, bottom: Number.POSITIVE_INFINITY };
  // Reading the sizes forces a style computation, so it waits until an edge fades; most containers never overflow.
  let sizesStale = true;
  const written: ScrollFadeSizes = { top: -1, bottom: -1 };
  node.setAttribute(SCROLL_FADE_OWNER_ATTRIBUTE, "");

  // Distances stop changing once they reach the fade size, so scrolling far from an end writes nothing.
  const writeDistance = (edge: keyof ScrollFadeSizes, distance: number) => {
    if (written[edge] === distance) return;
    written[edge] = distance;
    node.style.setProperty(FADE_DISTANCE_PROPERTIES[edge], `${distance}px`);
  };

  const refresh = () => {
    const fade = limitScrollEdgeFade(scrollEdgeFade(node), edges);
    if (fade !== "none" && sizesStale) {
      sizes = scrollFadeSizes(node, Number.POSITIVE_INFINITY);
      sizesStale = false;
    }
    const distances = scrollEdgeFadeDistances(node, fade, sizes);
    writeDistance("top", distances.top);
    writeDistance("bottom", distances.bottom);
    if (fade === "none") node.removeAttribute("data-scroll-fade");
    else node.setAttribute("data-scroll-fade", fade);
  };

  const measureAndRefresh = () => {
    sizesStale = true;
    refresh();
  };

  const stop = observeScrollContent(node, measureAndRefresh);
  node.addEventListener("scroll", refresh, { passive: true });
  return {
    update(next: { edges?: ScrollEdgeFadeEdges } = {}) {
      edges = next.edges ?? "both";
      refresh();
    },
    destroy() {
      stop();
      node.removeEventListener("scroll", refresh);
      node.removeAttribute("data-scroll-fade");
      node.removeAttribute(SCROLL_FADE_OWNER_ATTRIBUTE);
      for (const property of Object.values(FADE_DISTANCE_PROPERTIES)) node.style.removeProperty(property);
    },
  };
}
