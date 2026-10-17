export interface MusicFrequencyTooltipPosition {
  top: number;
  left: number;
  placement: "above" | "below";
}

interface MusicFrequencyTooltipPositionInput {
  anchorTop: number;
  anchorBottom: number;
  anchorLeft: number;
  tooltipWidth: number;
  tooltipHeight: number;
  viewportWidth: number;
  viewportHeight: number;
}

/** Places a frequency hint above its trigger when it fits in the viewport. */
export function pickMusicFrequencyTooltipPosition({
  anchorTop,
  anchorBottom,
  anchorLeft,
  tooltipWidth,
  tooltipHeight,
  viewportWidth,
  viewportHeight,
}: MusicFrequencyTooltipPositionInput): MusicFrequencyTooltipPosition {
  const inset = 8;
  const gap = 8;
  const placement = anchorTop - tooltipHeight - gap >= inset ? "above" : "below";
  const preferredTop = placement === "above" ? anchorTop - tooltipHeight - gap : anchorBottom + gap;
  return {
    top: Math.min(Math.max(inset, preferredTop), Math.max(inset, viewportHeight - tooltipHeight - inset)),
    left: Math.min(Math.max(inset, anchorLeft), Math.max(inset, viewportWidth - tooltipWidth - inset)),
    placement,
  };
}
