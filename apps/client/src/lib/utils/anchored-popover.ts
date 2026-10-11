import { pickSelectPopoverGeometry, type SelectPopoverHorizontalAlign, type SelectPopoverRect } from "./select-popover-position";

/** Options for {@link anchoredPopover}. */
export interface AnchoredPopoverOptions {
  /** The control the popover opens from; it stays inside for dismissal. */
  anchor: HTMLElement | undefined;
  /** Other elements that count as inside, such as a nested panel opened from this one. */
  inside?: readonly (HTMLElement | undefined)[];
  /** Called on a pointer press outside the popover, its anchor, and `inside`. */
  onOutside: () => void;
  horizontalAlign?: SelectPopoverHorizontalAlign;
}

/** The visible viewport that floating panels stay inside. */
function viewportRect(): SelectPopoverRect {
  const viewport = window.visualViewport;
  const left = viewport?.offsetLeft ?? 0;
  const top = viewport?.offsetTop ?? 0;
  const width = viewport?.width ?? window.innerWidth;
  const height = viewport?.height ?? window.innerHeight;
  return { left, top, right: left + width, bottom: top + height, width, height };
}

/**
 * Keep a fixed popover beside its anchor, below it or above it when there is more room, while the page scrolls,
 * the viewport resizes, or its content changes; and report pointer presses outside it.
 *
 * Pair it with `use:portal` and `position: fixed`. The popover stays hidden until it is first placed.
 */
export function anchoredPopover(node: HTMLElement, options: AnchoredPopoverOptions) {
  let current = options;
  node.style.visibility = "hidden";

  function place(): void {
    const anchor = current.anchor;
    if (!anchor?.isConnected) return;
    const geometry = pickSelectPopoverGeometry({
      triggerRect: anchor.getBoundingClientRect(),
      boundaryRect: viewportRect(),
      contentHeight: node.scrollHeight,
      contentWidth: node.getBoundingClientRect().width,
      horizontalAlign: current.horizontalAlign ?? "start",
    });
    node.style.left = `${geometry.left}px`;
    node.style.top = `${geometry.top}px`;
    node.style.maxHeight = `${geometry.maxHeight}px`;
    node.style.maxWidth = `${geometry.maxWidth}px`;
    node.style.visibility = "";
  }

  function outside(event: PointerEvent): void {
    const target = event.target;
    if (!(target instanceof Node)) return;
    const owners = [node, current.anchor, ...(current.inside ?? [])];
    if (!owners.some((owner) => owner?.contains(target))) current.onOutside();
  }

  const frame = requestAnimationFrame(place);
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
  observer?.observe(node);
  place();
  window.addEventListener("pointerdown", outside, true);
  window.addEventListener("resize", place);
  window.addEventListener("scroll", place, true);
  window.visualViewport?.addEventListener("resize", place);

  return {
    update(next: AnchoredPopoverOptions) {
      current = next;
      place();
    },
    destroy() {
      cancelAnimationFrame(frame);
      observer?.disconnect();
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
      window.visualViewport?.removeEventListener("resize", place);
    },
  };
}
