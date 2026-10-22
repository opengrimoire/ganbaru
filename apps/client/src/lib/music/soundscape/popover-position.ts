const VIEWPORT_INSET = 8;

/** Center the player panel over its trigger while keeping it within the viewport. */
export function centeredSoundscapePanelLeft(triggerLeft: number, triggerWidth: number, panelWidth: number, viewportWidth: number): number {
  const centeredLeft = triggerLeft + (triggerWidth - panelWidth) / 2;
  return Math.max(VIEWPORT_INSET, Math.min(centeredLeft, viewportWidth - VIEWPORT_INSET - panelWidth));
}
