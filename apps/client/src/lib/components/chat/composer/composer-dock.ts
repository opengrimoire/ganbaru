/** Custom property, set on the dock, where its opaque backdrop starts below the composer's rounded top corners. */
export const CHAT_COMPOSER_DOCK_BACKDROP_TOP_PROPERTY = "--chat-composer-dock-backdrop-top";

/** Selects the composer box whose top corner radius decides where the dock backdrop starts. */
const COMPOSER_FRAME_SELECTOR = "[data-organizational-composer] > [data-chat-composer-container]";

/** Vertical measurements of a composer dock and the composer box inside it, in CSS pixels. */
export interface ChatComposerDockGeometry {
  /** Top edge of the dock. */
  dockTop: number;
  /** Bottom edge of the dock. */
  dockBottom: number;
  /** Top edge of the composer box, or `null` when no composer box is rendered. */
  composerTop: number | null;
  /** Radius of the composer box's top corners. */
  cornerRadius: number;
}

/** Layout the timeline behind a composer dock needs. */
export interface ChatComposerDockMetrics {
  /** Height the timeline reserves at its end so its last message clears the dock. */
  height: number;
  /** Offset from the dock top where its opaque backdrop starts. */
  backdropTop: number;
}

/**
 * Measures a composer dock that floats over the end of a timeline. The dock reserves its whole height in the timeline,
 * and its backdrop starts where the composer's top corner curve ends, so messages scrolling under the dock stay visible
 * around the surfaces above the composer and inside its rounded corners, and are cut cleanly below that line.
 *
 * @param geometry Measured dock and composer edges.
 * @returns The reserved height and the backdrop start, both clamped to the dock.
 */
export function chatComposerDockMetrics(geometry: ChatComposerDockGeometry): ChatComposerDockMetrics {
  const height = Math.max(0, geometry.dockBottom - geometry.dockTop);
  if (geometry.composerTop === null) return { height, backdropTop: 0 };
  const cornerEnd = geometry.composerTop - geometry.dockTop + Math.max(0, geometry.cornerRadius);
  return { height, backdropTop: Math.min(height, Math.max(0, cornerEnd)) };
}

/** Options for {@link chatComposerDock}. */
export interface ChatComposerDockOptions {
  /** Receives the dock metrics after every size change, starting with the first measurement. */
  onMeasure: (metrics: ChatComposerDockMetrics) => void;
}

/**
 * Keeps a composer dock's backdrop start in sync with its composer and reports the height its timeline must reserve.
 * Every change inside the dock that moves the composer also resizes the dock, so observing the dock alone is enough.
 *
 * @param node The dock element, positioned over the end of its timeline.
 * @param initial Receives the measured metrics.
 * @returns A Svelte action lifecycle.
 */
export function chatComposerDock(node: HTMLElement, initial: ChatComposerDockOptions) {
  let options = initial;
  const measure = () => {
    const dock = node.getBoundingClientRect();
    const frame = node.querySelector<HTMLElement>(COMPOSER_FRAME_SELECTOR);
    const metrics = chatComposerDockMetrics({
      dockTop: dock.top,
      dockBottom: dock.bottom,
      composerTop: frame ? frame.getBoundingClientRect().top : null,
      cornerRadius: frame ? Number.parseFloat(getComputedStyle(frame).borderTopLeftRadius) || 0 : 0,
    });
    node.style.setProperty(CHAT_COMPOSER_DOCK_BACKDROP_TOP_PROPERTY, `${metrics.backdropTop}px`);
    options.onMeasure(metrics);
  };
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
  observer?.observe(node);
  measure();
  return {
    update(next: ChatComposerDockOptions) {
      options = next;
    },
    destroy() {
      observer?.disconnect();
    },
  };
}
