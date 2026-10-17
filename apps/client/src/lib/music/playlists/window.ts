export const MUSIC_PLAYLIST_ROW_HEIGHT_PX = 36;
export const MUSIC_PLAYLIST_OVERSCAN_ROWS = 6;

export interface MusicPlaylistWindow {
  startIndex: number;
  endIndex: number;
  topSpacerHeight: number;
  bottomSpacerHeight: number;
}

/**
 * Calculates the bounded set of playlist rows needed for the current viewport.
 */
export function musicPlaylistWindow(
  itemCount: number,
  scrollTop: number,
  viewportHeight: number,
  rowHeight = MUSIC_PLAYLIST_ROW_HEIGHT_PX,
  overscanRows = MUSIC_PLAYLIST_OVERSCAN_ROWS,
): MusicPlaylistWindow {
  const count = Math.max(0, Math.floor(itemCount));
  const safeRowHeight = Number.isFinite(rowHeight) && rowHeight > 0 ? rowHeight : 1;
  const safeScrollTop = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0;
  const safeViewportHeight = Number.isFinite(viewportHeight) ? Math.max(0, viewportHeight) : 0;
  const overscan = Number.isFinite(overscanRows) ? Math.max(0, Math.floor(overscanRows)) : 0;
  const firstVisibleIndex = Math.min(count, Math.floor(safeScrollTop / safeRowHeight));
  const visibleCount = Math.max(1, Math.ceil(safeViewportHeight / safeRowHeight));
  const startIndex = Math.max(0, firstVisibleIndex - overscan);
  const endIndex = Math.min(count, firstVisibleIndex + visibleCount + overscan);
  return {
    startIndex,
    endIndex,
    topSpacerHeight: startIndex * safeRowHeight,
    bottomSpacerHeight: Math.max(0, count - endIndex) * safeRowHeight,
  };
}
