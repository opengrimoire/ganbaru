/** Most location segments shown before the middle of a path collapses. */
export const SOURCE_LOCATION_MAX_SEGMENTS = 4;

/** One visible piece of a compact source location. */
export type SourceLocationSegment =
  | { kind: "title"; text: string; last: boolean }
  | { kind: "gap" };

/**
 * Shortens a location path for display, keeping its outermost segment and the segments closest to the source.
 *
 * Paths longer than `maxSegments` keep the first segment, a gap for the hidden middle, and the final
 * `maxSegments - 2` segments, so "Group / Project / A / B / C / Page" becomes "Group / … / C / Page".
 *
 * @param segments Location titles from the outermost one, already resolved to display text.
 * @param maxSegments Most visible pieces, gap included; values below 3 are treated as 3.
 * @returns The visible pieces in display order, with the final title marked as `last`.
 */
export function compactSourceLocation(segments: readonly string[], maxSegments = SOURCE_LOCATION_MAX_SEGMENTS): SourceLocationSegment[] {
  const limit = Math.max(3, Math.floor(maxSegments));
  const titleSegment = (text: string, index: number, all: readonly string[]): SourceLocationSegment => ({ kind: "title", text, last: index === all.length - 1 });
  if (segments.length <= limit) return segments.map((text, index) => titleSegment(text, index, segments));
  const tail = segments.slice(segments.length - (limit - 2));
  return [
    { kind: "title", text: segments[0] ?? "", last: false },
    { kind: "gap" },
    ...tail.map((text, index) => titleSegment(text, index, tail)),
  ];
}
