export interface NotesFloatingPanelRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface NotesFloatingPanelPlacement {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
}

/** Measure unclipped panel content and its borders so fitted panels do not scroll by a few pixels. */
export function notesFloatingPanelContentHeight(
  panel: Pick<HTMLElement, "scrollHeight" | "clientHeight" | "offsetHeight">,
): number {
  return Math.ceil(panel.scrollHeight + Math.max(0, panel.offsetHeight - panel.clientHeight));
}

/** Place a Notes title panel next to its trigger within the visible viewport. */
export function notesFloatingPanelPlacement(
  trigger: NotesFloatingPanelRect,
  viewport: { width: number; height: number },
  preferred: { width: number; height: number; align: "start" | "end" },
): NotesFloatingPanelPlacement {
  const margin = 8;
  const gap = 6;
  const width = Math.max(0, Math.min(preferred.width, viewport.width - margin * 2));
  const preferredLeft = preferred.align === "start" ? trigger.left : trigger.right - width;
  const left = Math.max(margin, Math.min(preferredLeft, viewport.width - width - margin));
  const spaceBelow = Math.max(0, viewport.height - trigger.bottom - gap - margin);
  const spaceAbove = Math.max(0, trigger.top - gap - margin);
  const openAbove = spaceBelow < preferred.height && spaceAbove > spaceBelow;
  const maxHeight = Math.min(preferred.height, openAbove ? spaceAbove : spaceBelow);
  const preferredTop = openAbove ? trigger.top - gap - maxHeight : trigger.bottom + gap;
  const top = Math.max(margin, Math.min(preferredTop, viewport.height - margin - maxHeight));
  return { left, top, width, maxHeight };
}
