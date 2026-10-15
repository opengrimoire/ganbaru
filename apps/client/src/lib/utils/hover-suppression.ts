/**
 * Keeps a resting pointer from hovering content that a layout change moved under it, such as a sidebar collapsing.
 * While suppressed, the root carries {@link HOVER_SUPPRESSED_ATTRIBUTE} and the app stylesheet makes tab content ignore
 * the pointer. Suppression ends once the pointer deliberately moves, as {@link waitForPointerTravel} defines it.
 */

/** Pointer travel, in CSS pixels, that counts as deliberate movement rather than a resting pointer. */
export const HOVER_RESUME_DISTANCE_PX = 3;

/**
 * Largest distance, in CSS pixels, between two pointer moves that still reads as one movement. It stays below half
 * the narrowest tab, the least a dragged tab travels to trade places, so a replay of where a drag started never
 * reads as a step.
 */
export const POINTER_STEP_MAX_PX = 10;

/** Root attribute that the app stylesheet reads while hover is suppressed. */
export const HOVER_SUPPRESSED_ATTRIBUTE = "data-hover-suppressed";

/** Source of the pointer events that end a wait for deliberate movement. */
export type PointerTravelEvents = Pick<EventTarget, "addEventListener" | "removeEventListener">;

/** Where suppression is marked, which events end it, and whether the device hovers at all. */
export interface HoverSuppressionHost {
  root: Pick<Element, "setAttribute" | "removeAttribute">;
  events: PointerTravelEvents;
  canHover: () => boolean;
}

let releaseActive: (() => void) | null = null;

function documentHost(): HoverSuppressionHost | null {
  if (typeof window === "undefined" || typeof document === "undefined") return null;
  return {
    root: document.documentElement,
    events: window,
    canHover: () => typeof window.matchMedia === "function" && window.matchMedia("(hover: hover) and (pointer: fine)").matches,
  };
}

/**
 * Calls `onTravel` once when the pointer deliberately moves: when consecutive pointer moves, each at most
 * {@link POINTER_STEP_MAX_PX} from the one before, add up to {@link HOVER_RESUME_DISTANCE_PX}, or when a button is
 * pressed or the wheel turns. Engines replay where the pointer rests after layout, and after a drag even where the
 * drag started, so a move that jumps farther than a step only starts a new count. Returns a function that stops
 * waiting without calling `onTravel`.
 */
export function waitForPointerTravel(events: PointerTravelEvents, onTravel: () => void): () => void {
  let previous: { x: number; y: number } | null = null;
  let travelled = 0;
  const options: AddEventListenerOptions = { capture: true, passive: true };

  const stop = (): void => {
    events.removeEventListener("pointermove", handleMove, options);
    events.removeEventListener("pointerdown", travel, options);
    events.removeEventListener("wheel", travel, options);
  };

  function travel(): void {
    stop();
    onTravel();
  }

  function handleMove(event: Event): void {
    const { clientX, clientY } = event as PointerEvent;
    const step = previous ? Math.hypot(clientX - previous.x, clientY - previous.y) : Number.POSITIVE_INFINITY;
    previous = { x: clientX, y: clientY };
    travelled = step > POINTER_STEP_MAX_PX ? 0 : travelled + step;
    if (travelled >= HOVER_RESUME_DISTANCE_PX) travel();
  }

  events.addEventListener("pointermove", handleMove, options);
  events.addEventListener("pointerdown", travel, options);
  events.addEventListener("wheel", travel, options);
  return stop;
}

/**
 * Suppresses hover until the pointer deliberately moves. Touch-only devices are left alone, so a tap is never swallowed.
 * Calling it again while active restarts the wait from the pointer's next reported position.
 */
export function suppressHoverUntilPointerMoves(host: HoverSuppressionHost | null = documentHost()): void {
  if (!host || !host.canHover()) return;
  releaseActive?.();

  const release = (): void => {
    stopWaiting();
    host.root.removeAttribute(HOVER_SUPPRESSED_ATTRIBUTE);
    if (releaseActive === release) releaseActive = null;
  };

  host.root.setAttribute(HOVER_SUPPRESSED_ATTRIBUTE, "");
  const stopWaiting = waitForPointerTravel(host.events, release);
  releaseActive = release;
}

/**
 * Returns an observer for a layout toggle, such as a sidebar's collapsed state, that suppresses hover whenever the
 * value changes. The first reading only records the starting value, so mounting a component never suppresses hover.
 */
export function suppressHoverOnToggle(
  host?: HoverSuppressionHost | null,
): (value: boolean) => void {
  let previous: boolean | null = null;
  return (value) => {
    if (previous !== null && value !== previous) suppressHoverUntilPointerMoves(host === undefined ? documentHost() : host);
    previous = value;
  };
}
