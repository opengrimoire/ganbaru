export interface MusicItemMenuLayoutInput {
  anchor: { left: number; right: number; top: number; bottom: number };
  viewportWidth: number;
  viewportHeight: number;
  panelWidth: number;
  panelHeight: number;
  gap?: number;
  margin?: number;
}

export interface MusicItemMenuLayout {
  panelLeft: number;
  panelTop: number;
}

/** Fits a track action panel around its row action button. */
export function projectMusicItemMenuLayout(input: MusicItemMenuLayoutInput): MusicItemMenuLayout {
  const margin = input.margin ?? 6;
  const gap = input.gap ?? 4;
  const viewportWidth = Math.max(0, input.viewportWidth);
  const viewportHeight = Math.max(0, input.viewportHeight);
  const panelLeft = clamp(input.anchor.right - input.panelWidth, margin, viewportWidth - input.panelWidth - margin);
  const spaceBelow = viewportHeight - input.anchor.bottom - margin;
  const panelTop = spaceBelow >= input.panelHeight
    ? input.anchor.bottom + gap
    : clamp(input.anchor.top - input.panelHeight - gap, margin, viewportHeight - input.panelHeight - margin);
  return {
    panelLeft,
    panelTop,
  };
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(Math.max(minimum, maximum), value));
}
