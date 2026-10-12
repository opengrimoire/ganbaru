/**
 * Build the checkerboard background used behind transparent colors.
 *
 * The tile contains a 2 by 2 checker. A 12px tile renders 6px cells.
 */
export function checkerboardBackground(tileSizePx: number): string {
  const tile = Number.isFinite(tileSizePx) && tileSizePx > 0 ? tileSizePx : 12;
  return (
    "conic-gradient(" +
    "var(--editor-chrome-checker-a) 25%, " +
    "var(--editor-chrome-checker-b) 25% 50%, " +
    "var(--editor-chrome-checker-a) 50% 75%, " +
    "var(--editor-chrome-checker-b) 75%" +
    `) 0 0/${tile}px ${tile}px`
  );
}

/**
 * Build a checkerboard background that fits a target cell count in a square.
 */
export function checkerboardBackgroundForCells(
  squareSizePx: number,
  cellsPerSide: number,
): string {
  const size =
    Number.isFinite(squareSizePx) && squareSizePx > 0 ? squareSizePx : 24;
  const cells =
    Number.isFinite(cellsPerSide) && cellsPerSide > 0 ? cellsPerSide : 3;
  return checkerboardBackground((size / cells) * 2);
}
