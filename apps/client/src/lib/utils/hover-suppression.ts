/**
 * Keeps a resting pointer from hovering content that a layout change moved under it, such as a sidebar collapsing.
 * While suppressed, the root carries {@link HOVER_SUPPRESSED_ATTRIBUTE} and the app stylesheet makes tab content ignore
 * the pointer. The first pointer event only records where the pointer rests, because engines may replay it after
 * layout; suppression ends once the pointer travels {@link HOVER_RESUME_DISTANCE_PX} from there, a button is pressed, or
 * the wheel turns, since content that ignores the pointer would not scroll either.
 */

/** Pointer travel, in CSS pixels, that counts as deliberate movement rather than a resting pointer. */
export const HOVER_RESUME_DISTANCE_PX = 3;

/** Root attribute that the app stylesheet reads while hover is suppressed. */
export const HOVER_SUPPRESSED_ATTRIBUTE = "data-hover-suppressed";

/** Where suppression is marked, which events end it, and whether the device hovers at all. */
export interface HoverSuppressionHost {
  root: Pick<Element, "setAttribute" | "removeAttribute">;
  events: Pick<EventTarget, "addEventListener" | "removeEventListener">;
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
 * Suppresses hover until the pointer deliberately moves. Touch-only devices are left alone, so a tap is never swallowed.
 * Calling it again while active restarts the wait from the pointer's next reported position.
 */
export function suppressHoverUntilPointerMoves(host: HoverSuppressionHost | null = documentHost()): void {
  if (!host || !host.canHover()) return;
  releaseActive?.();

  let origin: { x: number; y: number } | null = null;
  const options: AddEventListenerOptions = { capture: true, passive: true };

  const release = (): void => {
    host.root.removeAttribute(HOVER_SUPPRESSED_ATTRIBUTE);
    host.events.removeEventListener("pointermove", handleMove, options);
    host.events.removeEventListener("pointerdown", release, options);
    host.events.removeEventListener("wheel", release, options);
    if (releaseActive === release) releaseActive = null;
  };

  function handleMove(event: Event): void {
    const { clientX, clientY } = event as PointerEvent;
    if (!origin) {
      origin = { x: clientX, y: clientY };
      return;
    }
    if (Math.hypot(clientX - origin.x, clientY - origin.y) >= HOVER_RESUME_DISTANCE_PX) release();
  }

  host.root.setAttribute(HOVER_SUPPRESSED_ATTRIBUTE, "");
  host.events.addEventListener("pointermove", handleMove, options);
  host.events.addEventListener("pointerdown", release, options);
  host.events.addEventListener("wheel", release, options);
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
