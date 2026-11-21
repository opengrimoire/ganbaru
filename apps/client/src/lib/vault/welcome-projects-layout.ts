/**
 * Layout math of the welcome Projects scene: how far the task list scrolls sideways, where an open cell menu
 * sits, where the project settings rest for each control, and where the color palette opens, all from
 * measured boxes in logical canvas pixels.
 */
import { PROJECTS_SETTINGS_TARGETS, type ProjectsSettingsTarget } from "./welcome-projects-scene";

/** Where the Defaults heading rests once the settings scroll to it, below the top of their viewport. */
const DEFAULTS_SCROLL_PAD = 4;
/** Room kept above or below a settings control once the settings scroll it into view. */
const CONTROL_SCROLL_PAD = 8;
/** Room kept so a popover that just fits by measure still fits after rounding. */
const FIT_SLACK_PX = 1;

/** A cell menu's edge margin and its gap from the cell, as the list's cell menus place themselves. */
const CELL_MENU_MARGIN = 8;
const CELL_MENU_GAP = 6;
/** Room kept beside a cell once the list scrolls it into view. */
const CELL_SCROLL_PAD = 12;
/** The color palette's edge margin and its padding, whose half is its gap from the trigger, as the picker places it. */
const PALETTE_EDGE = 8;
const PALETTE_PADDING_REM = 0.625;

/** A trigger, a viewport, or a section measured within its positioned parent or the canvas, in logical pixels. */
export interface Box {
  top: number;
  left: number;
  width: number;
  height: number;
}

/** Where the list's parts sit, measured once the added task's row lays itself out. Cells are in canvas coordinates, before any scroll. */
export interface ListLayout {
  viewportWidth: number;
  contentWidth: number;
  status: Box;
  /** The first subtask's status cell, once that subtask is in the list. */
  subtaskStatus: Box | null;
  priority: Box;
  menuWidth: number;
  statusMenuHeight: number;
  priorityMenuHeight: number;
}

/**
 * Where the settings' parts sit, measured once the panel lays itself out for the canvas. The viewport is within the
 * panel, and the Defaults heading and the controls are within the settings' content, before any scroll.
 */
export interface SettingsLayout {
  viewport: Box;
  contentHeight: number;
  defaultsTop: number;
  controls: Record<ProjectsSettingsTarget, Box>;
  paletteWidth: number;
  paletteHeight: number;
  durationMenuHeight: number;
  pomodoroMenuHeight: number;
  durationMenuWidth: number;
  pomodoroMenuWidth: number;
  playlistMenuHeight: number;
  rem: number;
}

/** A placed popover's top left corner on the canvas. */
export interface CanvasPlace {
  left: number;
  top: number;
}

/**
 * How far the list has scrolled sideways: until the added task's status is in view, then on until its priority is.
 * `listScroll` holds each scroll's eased progress from 0 to 1.
 */
export function projectsListScrollPx(listLayout: ListLayout, listScroll: { status: number; priority: number }): number {
  const { viewportWidth } = listLayout;
  const maxScroll = Math.max(0, listLayout.contentWidth - viewportWidth);
  const target = (cell: Box) => Math.min(maxScroll, Math.max(0, cell.left + cell.width + CELL_SCROLL_PAD - viewportWidth));
  const statusTarget = target(listLayout.status);
  const priorityTarget = Math.max(statusTarget, target(listLayout.priority));
  return statusTarget * listScroll.status + (priorityTarget - statusTarget) * listScroll.priority;
}

/**
 * An open cell menu's place, below its cell when it fits there and above it otherwise, as the list's cell menus place
 * themselves. The cell is in canvas coordinates before the list's `scrollX` and `scrollY`.
 */
export function projectsCellMenuPlace(input: {
  cell: Box;
  menuWidth: number;
  menuHeight: number;
  scrollX: number;
  scrollY: number;
  canvasW: number;
  canvasH: number;
}): CanvasPlace {
  const { cell, menuWidth, menuHeight, canvasW, canvasH } = input;
  const cellLeft = cell.left - input.scrollX;
  const cellTop = cell.top - input.scrollY;
  const left = Math.max(CELL_MENU_MARGIN, Math.min(cellLeft, canvasW - menuWidth - CELL_MENU_MARGIN));
  const belowTop = cellTop + cell.height + CELL_MENU_GAP;
  const aboveBottom = cellTop - CELL_MENU_GAP;
  const belowSpace = Math.max(0, canvasH - CELL_MENU_MARGIN - belowTop);
  const aboveSpace = Math.max(0, aboveBottom - CELL_MENU_MARGIN);
  const below = belowSpace >= menuHeight || belowSpace >= aboveSpace;
  const top = Math.max(CELL_MENU_MARGIN, Math.min(below ? belowTop : aboveBottom - menuHeight, canvasH - CELL_MENU_MARGIN - menuHeight));
  return { left, top };
}

/**
 * Where the settings rest for each control: the Defaults heading at the top for the event name, then for every other
 * control the least scroll from the one before that brings it into view. The color also keeps room for its palette
 * below or above it on the canvas, so the palette never covers its own trigger on a short canvas. `panelTop` is the
 * settings panel's top on the canvas.
 */
export function projectsSettingsOffsets(
  layout: SettingsLayout,
  panelTop: number,
  canvasH: number,
): Record<ProjectsSettingsTarget, number> {
  const viewportHeight = layout.viewport.height;
  const maxScroll = Math.max(0, layout.contentHeight - viewportHeight);
  const clamp = (value: number) => Math.min(maxScroll, Math.max(0, value));
  const base = panelTop + layout.viewport.top;
  const paletteGap = (PALETTE_PADDING_REM / 2) * layout.rem;
  const offsets = {} as Record<ProjectsSettingsTarget, number>;
  let previous = 0;
  for (const target of PROJECTS_SETTINGS_TARGETS) {
    const box = layout.controls[target];
    let offset: number;
    if (target === "name") {
      offset = clamp(layout.defaultsTop - DEFAULTS_SCROLL_PAD);
    } else {
      const pad = CONTROL_SCROLL_PAD;
      let low = clamp(box.top + box.height + pad - viewportHeight);
      let high = Math.max(low, clamp(box.top - pad));
      if (target === "color") {
        const belowLow = base + box.top + box.height + paletteGap + layout.paletteHeight + PALETTE_EDGE - canvasH + FIT_SLACK_PX;
        const aboveHigh = base + box.top - paletteGap - layout.paletteHeight - PALETTE_EDGE - FIT_SLACK_PX;
        const nearest = (from: number, to: number) => Math.min(to, Math.max(from, previous));
        const below = belowLow <= high ? { low: Math.max(low, belowLow), high } : null;
        const above = aboveHigh >= low ? { low, high: Math.min(high, aboveHigh) } : null;
        const fits = [below, above].filter((range): range is { low: number; high: number } => range !== null && range.low <= range.high);
        if (fits.length > 0) {
          const best = fits.reduce((chosen, range) =>
            Math.abs(nearest(range.low, range.high) - previous) < Math.abs(nearest(chosen.low, chosen.high) - previous) ? range : chosen);
          low = best.low;
          high = best.high;
        }
      }
      offset = Math.min(high, Math.max(low, previous));
    }
    offsets[target] = offset;
    previous = offset;
  }
  return offsets;
}

/**
 * The color palette's place: below its trigger when it fits, above it otherwise, and kept on the canvas. The trigger
 * is on the canvas at the settings' current scroll.
 */
export function projectsPalettePlace(
  trigger: { top: number; left: number; bottom: number },
  layout: Pick<SettingsLayout, "paletteWidth" | "paletteHeight" | "rem">,
  canvasW: number,
  canvasH: number,
): CanvasPlace {
  const { paletteWidth, paletteHeight } = layout;
  const offset = (PALETTE_PADDING_REM / 2) * layout.rem;
  const maxLeft = Math.max(PALETTE_EDGE, canvasW - paletteWidth - PALETTE_EDGE);
  const left = Math.min(Math.max(PALETTE_EDGE, trigger.left), maxLeft);
  const belowTop = trigger.bottom + offset;
  const aboveTop = trigger.top - paletteHeight - offset;
  const top = belowTop + paletteHeight + PALETTE_EDGE <= canvasH
    ? belowTop
    : aboveTop >= PALETTE_EDGE
      ? aboveTop
      : Math.max(PALETTE_EDGE, canvasH - paletteHeight - PALETTE_EDGE);
  return { left, top };
}
