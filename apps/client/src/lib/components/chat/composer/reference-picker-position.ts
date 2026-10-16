import { tick } from "svelte";
import { anchoredPanelStyle, verticalBorderWidth } from "$lib/utils/anchored-panel";
import { portal } from "$lib/utils/portal";

/** Inputs for the reference picker that hangs from a Chat composer. */
export interface ChatReferencePickerPosition {
  /** The composer box; the picker sits outside it so it never covers the text being typed. */
  composer: HTMLElement | null;
  /** The picker's scrolling list, used to size the picker to its content. */
  scrollArea: () => HTMLElement | null;
  /** Changes whenever the listed content changes, so the picker resizes to it. */
  contentKey: unknown;
}

const PREFERRED_MAX_HEIGHT_REM = 22;

/**
 * Portal the reference picker to the composer's floating root and keep it against the composer, above it when below
 * lacks room, while the composer, the viewport, or the content changes size or scrolls.
 *
 * @param node The picker element.
 * @param initial The composer and the picker's scrolling list.
 * @returns A Svelte action lifecycle.
 */
export function chatReferencePickerPosition(node: HTMLElement, initial: ChatReferencePickerPosition) {
  let params = initial;
  const composer = initial.composer;
  if (!composer) return {};
  const portaled = portal(node, composer.closest<HTMLElement>("[data-floating-root]") ?? document.body);
  const viewport = window.visualViewport;
  let disposed = false;

  const update = () => {
    if (disposed) return;
    const rect = composer.getBoundingClientRect();
    const offsetLeft = viewport?.offsetLeft ?? 0;
    const offsetTop = viewport?.offsetTop ?? 0;
    const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
    const scrollArea = params.scrollArea();
    node.style.cssText = anchoredPanelStyle({
      triggerRect: {
        left: rect.left - offsetLeft,
        right: rect.right - offsetLeft,
        top: rect.top - offsetTop,
        bottom: rect.bottom - offsetTop,
      },
      viewportWidth: viewport?.width ?? window.innerWidth,
      viewportHeight: viewport?.height ?? window.innerHeight,
      preferredWidth: rect.width,
      preferredMaxHeight: PREFERRED_MAX_HEIGHT_REM * rem,
      contentHeight: scrollArea ? scrollArea.scrollHeight + verticalBorderWidth(node) : undefined,
    });
    node.style.left = `${Number.parseFloat(node.style.left) + offsetLeft}px`;
    node.style.top = `${Number.parseFloat(node.style.top) + offsetTop}px`;
  };
  const scroll = (event: Event) => {
    if (!(event.target instanceof Node) || !node.contains(event.target)) update();
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
  observer?.observe(node);
  observer?.observe(composer);
  document.addEventListener("scroll", scroll, true);
  window.addEventListener("resize", update);
  viewport?.addEventListener("resize", update);
  viewport?.addEventListener("scroll", update);
  update();
  void tick().then(update);

  return {
    update(next: ChatReferencePickerPosition) {
      params = next;
      void tick().then(update);
    },
    destroy() {
      disposed = true;
      observer?.disconnect();
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", update);
      viewport?.removeEventListener("resize", update);
      viewport?.removeEventListener("scroll", update);
      portaled.destroy();
    },
  };
}
