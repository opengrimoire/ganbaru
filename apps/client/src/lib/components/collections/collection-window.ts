export interface CollectionWindow {
  start: number;
  end: number;
  beforePx: number;
  afterPx: number;
}

/** Bound a card window using measured heights, including the gaps between cards. */
export function collectionWindow(
  heights: readonly number[],
  scrollTop: number,
  viewportHeight: number,
  gap: number,
  overscan: number,
): CollectionWindow {
  const offsets = [0];
  for (const height of heights) offsets.push(offsets[offsets.length - 1] + Math.max(1, height) + gap);
  let first = 0;
  while (first < heights.length && offsets[first + 1] <= Math.max(0, scrollTop)) first++;
  first = Math.min(first, Math.max(0, heights.length - 1));
  let last = first;
  while (last < heights.length && offsets[last] < Math.max(0, scrollTop) + Math.max(1, viewportHeight)) last++;
  const start = Math.max(0, first - overscan);
  const end = Math.min(heights.length, Math.max(first + 1, last) + overscan);
  return {
    start,
    end,
    beforePx: start > 0 ? Math.max(0, offsets[start] - gap) : 0,
    afterPx: end < heights.length ? Math.max(0, offsets[heights.length] - offsets[end] - gap) : 0,
  };
}
