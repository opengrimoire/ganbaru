import { hexToRgb, type RgbColor } from "$lib/color/math";

/** Theme surface the desktop shell paints first, the native background of a window before its page paints. */
export const DESKTOP_SHELL_BACKGROUND_TOKEN = "--sidebar";

/** Computed `rgb()` or fully opaque `rgba()` color, the form browsers serialize sRGB colors in. */
const OPAQUE_RGB_PATTERN = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*1(?:\.0+)?\s*)?\)$/;

/**
 * Reads the active value of a theme surface token for the native window and WebView layers. Those layers paint
 * before the page and wherever it is transparent, so matching them to the theme avoids a white flash.
 *
 * @param root Element whose computed style holds the applied theme tokens.
 * @param token CSS custom property name, such as `--sidebar`.
 * @returns The opaque surface color, or null when the token is not a hex color.
 */
export function readNativeBackground(root: Element, token: string): RgbColor | null {
  return hexToRgb(getComputedStyle(root).getPropertyValue(token).trim());
}

/**
 * Parses a computed CSS color that is opaque sRGB.
 *
 * @param value Computed color, such as `rgb(38, 38, 38)`.
 * @returns The color, or null when it is translucent, out of range, or another color space.
 */
export function parseOpaqueComputedColor(value: string): RgbColor | null {
  const match = OPAQUE_RGB_PATTERN.exec(value.trim());
  if (!match) return null;
  const [r, g, b] = match.slice(1, 4).map(Number);
  return [r, g, b].every((channel) => channel <= 255) ? { r, g, b } : null;
}

/**
 * Reads the background a mounted root paints first, for the native layers that show while its window maps.
 *
 * @param mountTarget Element the root component mounted into.
 * @param fallbackToken Theme surface token used when the root's first element paints no opaque sRGB background.
 * @returns The opaque surface color, or null when neither source yields one.
 */
export function readMountedRootBackground(mountTarget: Element, fallbackToken: string): RgbColor | null {
  const surface = mountTarget.firstElementChild;
  const painted = surface ? parseOpaqueComputedColor(getComputedStyle(surface).backgroundColor) : null;
  return painted ?? readNativeBackground(mountTarget.ownerDocument.documentElement, fallbackToken);
}
