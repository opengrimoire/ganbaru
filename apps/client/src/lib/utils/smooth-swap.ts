import type { Attachment } from "svelte/attachments";
import { linear } from "svelte/easing";
import type { TransitionConfig } from "svelte/transition";

/** Timing shared by a container that changes height and the content swapped inside it. */
export interface SmoothSwapTiming {
  /** How long the container takes to reach a new height. */
  heightMs: number;
  /** How long leaving content takes to fade out. */
  fadeOutMs: number;
  /**
   * How long arriving content waits before it fades in. It waits until the height has nearly settled, so a growing
   * container never reveals visible content from the top down.
   */
  fadeInDelayMs: number;
  /** How long arriving content takes to fade in. */
  fadeInMs: number;
}

/** The default timing: content leaves quickly, the height settles, and then the new content fades in. */
export const SMOOTH_SWAP_TIMING: SmoothSwapTiming = { heightMs: 280, fadeOutMs: 120, fadeInDelayMs: 220, fadeInMs: 200 };

/** No motion at all, for users who ask the system for less. */
const NO_MOTION: SmoothSwapTiming = { heightMs: 0, fadeOutMs: 0, fadeInDelayMs: 0, fadeInMs: 0 };

/** Ease for height changes that starts fast and settles softly. */
const HEIGHT_EASING = "cubic-bezier(0.2, 0, 0, 1)";

/** Inline properties {@link swapOut} sets to pin leaving content in place. */
const PINNED_PROPERTIES = ["position", "top", "left", "width", "margin"] as const;

/** Whether the user asked the system for less motion. */
export function prefersReducedMotion(): boolean {
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** {@link SMOOTH_SWAP_TIMING}, or no motion at all when the user prefers less. */
export function smoothSwapTiming(): SmoothSwapTiming {
  return prefersReducedMotion() ? NO_MOTION : SMOOTH_SWAP_TIMING;
}

/**
 * Animates the element's height whenever its first child changes size, from whatever height it shows at that moment.
 * The child is the content box: it must be `position: relative` so {@link swapOut} can pin leaving content inside it,
 * and pinned content leaves its height, so the container moves straight to the arriving content's height.
 */
export function smoothHeight(timing: SmoothSwapTiming): Attachment<HTMLElement> {
  return (container) => {
    const content = container.firstElementChild;
    if (!(content instanceof HTMLElement) || typeof ResizeObserver === "undefined") return;
    let settled = content.offsetHeight;
    let animation: Animation | null = null;

    function finish(): void {
      animation = null;
      container.style.overflow = "";
    }

    const observer = new ResizeObserver(() => {
      const next = content.offsetHeight;
      if (next === settled) return;
      const from = animation ? container.getBoundingClientRect().height : settled;
      settled = next;
      animation?.cancel();
      finish();
      if (timing.heightMs <= 0 || Math.abs(from - next) < 1) return;
      container.style.overflow = "hidden";
      animation = container.animate(
        [{ height: `${from}px` }, { height: `${next}px` }],
        { duration: timing.heightMs, easing: HEIGHT_EASING },
      );
      animation.onfinish = finish;
    });
    observer.observe(content);
    return () => {
      observer.disconnect();
      animation?.cancel();
      finish();
    };
  };
}

/**
 * Svelte outro that pins the element where it is, out of the flow, and fades it. The arriving content takes its place
 * at once, so the container's height changes only once.
 */
export function swapOut(node: HTMLElement, timing: SmoothSwapTiming): TransitionConfig {
  if (timing.fadeOutMs > 0) {
    const width = node.getBoundingClientRect().width;
    const { offsetTop, offsetLeft } = node;
    Object.assign(node.style, { position: "absolute", top: `${offsetTop}px`, left: `${offsetLeft}px`, width: `${width}px`, margin: "0" });
  }
  return { duration: timing.fadeOutMs, easing: linear, css: (t) => `opacity: ${t}` };
}

/**
 * Svelte intro that waits for the leaving content to fade and the height to settle, then fades the element in. It also releases the pin
 * {@link swapOut} set, for content that returns before its outro finishes.
 */
export function swapIn(node: HTMLElement, timing: SmoothSwapTiming): TransitionConfig {
  for (const property of PINNED_PROPERTIES) node.style.removeProperty(property);
  return { delay: timing.fadeInDelayMs, duration: timing.fadeInMs, easing: linear, css: (t) => `opacity: ${t}` };
}
