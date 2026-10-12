export interface MusicVirtualWindowInput {
  count: number;
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  overscan?: number;
}

export interface MusicVirtualWindow {
  startIndex: number;
  endIndex: number;
  topSpacer: number;
  bottomSpacer: number;
  totalHeight: number;
}

export function musicVirtualWindow(input: MusicVirtualWindowInput): MusicVirtualWindow {
  const count = Math.max(0, Math.floor(input.count));
  const rowHeight = Math.max(1, input.rowHeight);
  const overscan = Math.max(0, Math.floor(input.overscan ?? 5));
  const scrollTop = Math.max(0, input.scrollTop);
  const viewportHeight = Math.max(0, input.viewportHeight);
  const visibleStart = Math.floor(scrollTop / rowHeight);
  const visibleEnd = Math.ceil((scrollTop + viewportHeight) / rowHeight);
  const startIndex = Math.max(0, Math.min(count, visibleStart - overscan));
  const endIndex = Math.max(startIndex, Math.min(count, visibleEnd + overscan));
  const totalHeight = count * rowHeight;
  return {
    startIndex,
    endIndex,
    topSpacer: startIndex * rowHeight,
    bottomSpacer: Math.max(0, totalHeight - endIndex * rowHeight),
    totalHeight,
  };
}

export function revealMusicVirtualIndex(
  index: number,
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
): number {
  if (index < 0 || viewportHeight <= 0 || rowHeight <= 0) return Math.max(0, scrollTop);
  const rowTop = index * rowHeight;
  const rowBottom = rowTop + rowHeight;
  if (rowTop < scrollTop) return rowTop;
  if (rowBottom > scrollTop + viewportHeight) return Math.max(0, rowBottom - viewportHeight);
  return Math.max(0, scrollTop);
}
