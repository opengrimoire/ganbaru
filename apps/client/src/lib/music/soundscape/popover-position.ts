import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
import {
  pickSelectPopoverGeometry,
  type SelectPopoverGeometry,
  type SelectPopoverRect,
} from "$lib/utils/select-popover-position";

const VIEWPORT_INSET = 8;
const TRIGGER_GAP = 6;
export const SOUNDSCAPE_POPOVER_WIDTH = FLOATING_WIDTH.sm;

/** Center the player panel over its trigger while keeping it within the viewport. */
export function centeredSoundscapePanelLeft(triggerLeft: number, triggerWidth: number, panelWidth: number, viewportWidth: number): number {
  const centeredLeft = triggerLeft + (triggerWidth - panelWidth) / 2;
  return Math.max(VIEWPORT_INSET, Math.min(centeredLeft, viewportWidth - VIEWPORT_INSET - panelWidth));
}

/** Center a soundscape flyout over its button pair and prefer the space above. */
export function pickSoundscapePopoverGeometry(
  triggerRect: SelectPopoverRect,
  boundaryRect: SelectPopoverRect,
  contentWidth: number,
  contentHeight: number,
): SelectPopoverGeometry {
  const fallback = pickSelectPopoverGeometry({
    triggerRect,
    boundaryRect,
    contentWidth,
    contentHeight,
    horizontalAlign: "end",
    gap: TRIGGER_GAP,
    inset: VIEWPORT_INSET,
  });
  const width = Math.min(contentWidth, Math.max(0, boundaryRect.width - VIEWPORT_INSET * 2));
  const center = (triggerRect.left + triggerRect.right) / 2;
  const left = Math.min(
    Math.max(center - width / 2, boundaryRect.left + VIEWPORT_INSET),
    boundaryRect.right - VIEWPORT_INSET - width,
  );
  const aboveSpace = triggerRect.top - TRIGGER_GAP - boundaryRect.top - VIEWPORT_INSET;
  const centered = {
    ...fallback,
    left,
    width,
    minWidth: width,
    maxWidth: width,
  };
  if (aboveSpace < contentHeight) return centered;
  return {
    ...centered,
    top: triggerRect.top - TRIGGER_GAP - contentHeight,
    maxHeight: aboveSpace,
    placement: "above",
  };
}
