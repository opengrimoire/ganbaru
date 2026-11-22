/**
 * Text styles that collection tables size columns for: the cell text, a regular badge, and a medium-weight badge
 * such as a status pill.
 */
export type CollectionTextStyle = "cell" | "badge" | "badgeStrong";

/** Width of one line of text in a collection text style, in CSS pixels. */
export type CollectionTextMeasurer = (text: string, style: CollectionTextStyle) => number;

/** Utility classes that give each style its font; they match the classes the table cells render with. */
const STYLE_CLASSES: Record<CollectionTextStyle, string> = {
  cell: "text-collection font-normal",
  badge: "text-[0.733333rem] font-normal",
  badgeStrong: "text-[0.733333rem] font-medium",
};

const STYLES = Object.keys(STYLE_CLASSES) as CollectionTextStyle[];

/** The canvas font shorthand of an element's computed style. */
function canvasFont(style: CSSStyleDeclaration): string {
  return [style.fontStyle, style.fontWeight, style.fontSize, style.fontFamily].filter(Boolean).join(" ");
}

/**
 * Measure text in the fonts a collection table renders, whatever the language, font family, or font scale.
 * Probes inside `host` resolve each style's real font once; text is then measured on a canvas and cached, so measuring
 * never forces a layout. Create a new measurer when the font family, font scale, or loaded fonts change.
 *
 * @param host A connected element inside the table, so probes inherit its font family.
 * @returns A measurer, or `undefined` when the environment has no canvas text metrics.
 */
export function createCollectionTextMeasurer(host: HTMLElement): CollectionTextMeasurer | undefined {
  const context = document.createElement("canvas").getContext("2d");
  if (!context) return undefined;
  const fonts = {} as Record<CollectionTextStyle, string>;
  for (const style of STYLES) {
    const probe = document.createElement("span");
    probe.className = STYLE_CLASSES[style];
    probe.style.cssText = "position:absolute;visibility:hidden;pointer-events:none;white-space:pre";
    host.appendChild(probe);
    fonts[style] = canvasFont(getComputedStyle(probe));
    probe.remove();
  }
  const cache = new Map<string, number>();
  return (text, style) => {
    const key = `${style}\u0000${text}`;
    const cached = cache.get(key);
    if (cached !== undefined) return cached;
    context.font = fonts[style];
    const width = context.measureText(text).width;
    cache.set(key, width);
    return width;
  };
}
