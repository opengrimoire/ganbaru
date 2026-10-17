/**
 * Moves one playlist to a bounded position without mutating the source array.
 *
 * @param playlists Current ordered playlists.
 * @param playlistId Playlist to move.
 * @param targetIndex Requested zero-based destination.
 * @returns The reordered array, or the original values when the id is absent.
 */
export function moveMusicPlaylistOrder<T extends { id: string }>(
  playlists: readonly T[],
  playlistId: string,
  targetIndex: number,
): T[] {
  const sourceIndex = playlists.findIndex((playlist) => playlist.id === playlistId);
  if (sourceIndex < 0) return [...playlists];
  const boundedTarget = Math.max(0, Math.min(Math.trunc(targetIndex), playlists.length - 1));
  if (sourceIndex === boundedTarget) return [...playlists];
  const reordered = [...playlists];
  const [moved] = reordered.splice(sourceIndex, 1);
  if (!moved) return [...playlists];
  reordered.splice(boundedTarget, 0, moved);
  return reordered;
}

export interface MusicPlaylistGridSlot {
  index: number;
  left: number;
  top: number;
}

export interface MusicPlaylistGridInsertion {
  index: number;
  distanceSquared: number;
}

export interface MusicPlaylistGridPosition {
  left: number;
  top: number;
  width: number;
}

export interface MusicPlaylistGridLayout {
  positions: MusicPlaylistGridPosition[];
  height: number;
  columns: number;
}

export const MUSIC_PLAYLIST_GRID_MIN_CARD_WIDTH = 272;
export const MUSIC_PLAYLIST_GRID_GAP = 8;
export const MUSIC_PLAYLIST_GRID_CARD_HEIGHT = 54;

/** Lays playlists out in a measured row-major grid matching the manager's desktop density. */
export function musicPlaylistGridLayout(
  containerWidth: number,
  heights: readonly number[],
  minimumCardWidth = MUSIC_PLAYLIST_GRID_MIN_CARD_WIDTH,
  gap = MUSIC_PLAYLIST_GRID_GAP,
): MusicPlaylistGridLayout {
  const safeWidth = Math.max(0, containerWidth);
  const columns = Math.max(1, Math.floor((safeWidth + gap) / (minimumCardWidth + gap)));
  const cardWidth = columns === 1 ? safeWidth : (safeWidth - gap * (columns - 1)) / columns;
  const rowHeights: number[] = [];
  for (let index = 0; index < heights.length; index += 1) {
    const row = Math.floor(index / columns);
    rowHeights[row] = Math.max(rowHeights[row] ?? 0, Math.max(0, heights[index] ?? 0));
  }
  const rowTops: number[] = [];
  let height = 0;
  for (const rowHeight of rowHeights) {
    rowTops.push(height);
    height += rowHeight + gap;
  }
  return {
    positions: heights.map((_, index) => ({
      left: (index % columns) * (cardWidth + gap),
      top: rowTops[Math.floor(index / columns)] ?? 0,
      width: cardWidth,
    })),
    height: Math.max(0, height - (rowHeights.length > 0 ? gap : 0)),
    columns,
  };
}

/** Finds the grid slot nearest to the dragged playlist card's top-left position. */
export function musicPlaylistGridInsertion(
  slots: readonly MusicPlaylistGridSlot[],
  targetLeft: number,
  targetTop: number,
  preferredIndex: number,
): MusicPlaylistGridInsertion {
  let best: MusicPlaylistGridInsertion = {
    index: Math.max(0, preferredIndex),
    distanceSquared: Number.POSITIVE_INFINITY,
  };
  for (const slot of slots) {
    const distanceSquared = (slot.left - targetLeft) ** 2 + (slot.top - targetTop) ** 2;
    const equallyClose = Math.abs(distanceSquared - best.distanceSquared) < 0.01;
    if (distanceSquared < best.distanceSquared || (equallyClose && slot.index === preferredIndex)) {
      best = { index: slot.index, distanceSquared };
    }
  }
  return best;
}
