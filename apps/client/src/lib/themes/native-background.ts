import { hexToRgb, type RgbColor } from "$lib/color/math";

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
