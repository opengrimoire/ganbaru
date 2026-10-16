export type ViewportOrientation = "portrait" | "landscape";

export const ANDROID_INSETS_EVENT_NAME = "ganbaru:android-insets";

export interface SystemBarInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface PointerCapabilities {
  coarsePointer: boolean;
  hoverAvailable: boolean;
}

interface MediaQueryState {
  readonly matches: boolean;
}

const MAX_SYSTEM_BAR_INSET_CSS_PIXELS = 4096;

/** Parse the bounded, read-only Android system-bar bridge response. */
export function parseAndroidSystemBarInsets(value: unknown): SystemBarInsets | null {
  if (typeof value !== "string") return null;
  const parts = value.split(",");
  if (parts.length !== 4 || parts.some((part) => part.trim() === "")) return null;
  const values = parts.map(Number);
  if (
    values.some(
      (inset) =>
        !Number.isFinite(inset) ||
        inset < 0 ||
        inset > MAX_SYSTEM_BAR_INSET_CSS_PIXELS,
    )
  ) {
    return null;
  }
  const [top, right, bottom, left] = values;
  return { top, right, bottom, left };
}

/**
 * Parse the Android soft keyboard height bridge response, in CSS pixels. The native shell already resizes the WebView
 * above the keyboard, so this reports that a keyboard is open rather than an area the page must avoid.
 */
export function parseAndroidKeyboardHeight(value: unknown): number | null {
  if (typeof value !== "string" || value.trim() === "") return null;
  const height = Number(value);
  if (!Number.isFinite(height) || height < 0 || height > MAX_SYSTEM_BAR_INSET_CSS_PIXELS) return null;
  return height;
}

/**
 * Returns the part of the layout viewport hidden below the visual viewport.
 *
 * A soft keyboard that overlays the page without resizing it shows up here.
 * The Android shell resizes the WebView above the keyboard instead, so this
 * stays zero there and the keyboard bridge reports that it is open.
 */
export function calculateKeyboardInset(
  layoutHeight: number,
  visualHeight: number,
  visualOffsetTop: number,
): number {
  if (![layoutHeight, visualHeight, visualOffsetTop].every(Number.isFinite)) return 0;
  return Math.max(0, Math.round(layoutHeight - visualHeight - visualOffsetTop));
}

/** Return a stable orientation for the current drawable viewport. */
export function viewportOrientation(width: number, height: number): ViewportOrientation {
  return width > height ? "landscape" : "portrait";
}

/** Resolve pointer capabilities with a touch-safe fallback when media queries are unavailable. */
export function resolvePointerCapabilities(
  coarsePointerQuery: MediaQueryState | null,
  hoverQuery: MediaQueryState | null,
  maxTouchPoints: number,
): PointerCapabilities {
  const touchAvailable = Number.isFinite(maxTouchPoints) && maxTouchPoints > 0;
  return {
    coarsePointer: coarsePointerQuery?.matches ?? touchAvailable,
    hoverAvailable: hoverQuery?.matches ?? !touchAvailable,
  };
}
