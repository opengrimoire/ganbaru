import { cubicOut } from "svelte/easing";
import type { TransitionConfig } from "svelte/transition";

/** Share of the transition spent changing height. */
const GROW_SHARE = 0.6;
/** Share of the transition spent fading the content; it overlaps the end of the height change slightly. */
const FADE_SHARE = 0.5;

/** Box metrics an element opens to, measured before the transition starts. */
export interface GrowThenFadeBox {
  height: number;
  paddingTop: number;
  paddingBottom: number;
  marginTop: number;
  marginBottom: number;
}

export interface GrowThenFadeParams {
  duration?: number;
  easing?: (t: number) => number;
}

/**
 * Inline style for progress `t` from 0 (closed) to 1 (open): the height grows while the content stays hidden, then
 * the content fades in. Closing runs the same steps backwards, so content never slides or wipes into view.
 */
export function growThenFadeStyle(t: number, box: GrowThenFadeBox, easing: (t: number) => number = cubicOut): string {
  const grow = easing(Math.min(1, t / GROW_SHARE));
  const fade = Math.min(1, Math.max(0, (t - (1 - FADE_SHARE)) / FADE_SHARE));
  return [
    "overflow: hidden",
    `opacity: ${fade}`,
    `height: ${grow * box.height}px`,
    `padding-top: ${grow * box.paddingTop}px`,
    `padding-bottom: ${grow * box.paddingBottom}px`,
    `margin-top: ${grow * box.marginTop}px`,
    `margin-bottom: ${grow * box.marginBottom}px`,
  ].join("; ");
}

/** Svelte transition that opens and closes an element with {@link growThenFadeStyle}. */
export function growThenFade(node: Element, { duration = 240, easing = cubicOut }: GrowThenFadeParams = {}): TransitionConfig {
  const style = getComputedStyle(node);
  const box: GrowThenFadeBox = {
    height: parseFloat(style.height) || 0,
    paddingTop: parseFloat(style.paddingTop) || 0,
    paddingBottom: parseFloat(style.paddingBottom) || 0,
    marginTop: parseFloat(style.marginTop) || 0,
    marginBottom: parseFloat(style.marginBottom) || 0,
  };
  return { duration, css: (t) => growThenFadeStyle(t, box, easing) };
}
