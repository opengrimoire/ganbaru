/**
 * Converts a computed CSS length in `px`, `rem`, or `em` to pixels, resolving `em` against `context`.
 *
 * @param value - The length as `getComputedStyle` reports it, such as the value of an unregistered custom property.
 * @param context - The element whose font size resolves `em`.
 * @param fallback - The result when `value` is not a number.
 * @returns The length in pixels.
 */
export function cssLengthToPixels(value: string, context: HTMLElement, fallback: number): number {
  const trimmed = value.trim();
  const parsed = Number.parseFloat(trimmed);
  if (!Number.isFinite(parsed)) return fallback;
  if (trimmed.endsWith("rem")) return parsed * fontSizePixels(document.documentElement);
  if (trimmed.endsWith("em")) return parsed * fontSizePixels(context);
  return parsed;
}

/** The computed font size of `element`, or the browser default when it cannot be read. */
function fontSizePixels(element: Element): number {
  const fontSize = Number.parseFloat(getComputedStyle(element).fontSize);
  return Number.isFinite(fontSize) ? fontSize : 16;
}
