/**
 * Shared hover-submenu behavior, so every nested menu, picker, and flyout opens, holds, and closes alike.
 * docs/architecture/ui-foundations.md owns the rules.
 */

/** Delay before a hovered row opens its submenu, so a pointer crossing several rows does not open each one. */
export const SUBMENU_OPEN_DELAY_MS = 100;
/** Grace period before a hover-opened submenu closes, so the pointer can travel from its row into it. */
export const SUBMENU_CLOSE_DELAY_MS = 250;
/** Aim corridor toward a submenu, in pixels; the lower edge is wider because submenus usually extend downward. */
export const SUBMENU_AIM_TOLERANCES = {
  tolerance: 12,
  topTolerance: 8,
  bottomTolerance: 32,
  minTowardDistance: 3,
} as const satisfies Pick<MenuAimInput, "tolerance" | "topTolerance" | "bottomTolerance" | "minTowardDistance">;

/** Runs `action` after `delayMs` and returns a function that cancels it. Injectable so tests control time. */
export type HoverTimerScheduler = (action: () => void, delayMs: number) => () => void;

const scheduleBrowserTimer: HoverTimerScheduler = (action, delayMs) => {
  const handle = setTimeout(action, delayMs);
  return () => clearTimeout(handle);
};

/** A single pending hover action, such as closing a submenu or switching to the row under a resting pointer. */
export interface HoverDelay {
  /** Replaces any pending action with `action`, run after `delayMs` (the submenu close delay by default). */
  schedule: (action: () => void, delayMs?: number) => void;
  cancel: () => void;
}

export function createHoverDelay(scheduler: HoverTimerScheduler = scheduleBrowserTimer): HoverDelay {
  let cancelPending: (() => void) | null = null;
  return {
    schedule(action, delayMs = SUBMENU_CLOSE_DELAY_MS) {
      cancelPending?.();
      cancelPending = scheduler(() => {
        cancelPending = null;
        action();
      }, delayMs);
    },
    cancel() {
      cancelPending?.();
      cancelPending = null;
    },
  };
}

/**
 * Hover intent for a row of triggers that share one panel, such as breadcrumb navigators. Resting on a trigger opens
 * its panel after the open delay, and moving to a sibling trigger while a panel shows switches at once. A hover-opened
 * panel closes after the close delay once the pointer leaves both the triggers and the panel. A click, a press inside
 * the panel, or typing in it pins the panel until it is dismissed.
 */
export interface HoverMenu {
  /** The pointer entered a trigger; `open` shows that trigger's panel. Touch pointers are ignored, since a tap clicks. */
  enterTrigger: (event: Pick<PointerEvent, "pointerType">, open: () => void) => void;
  /** The pointer left a trigger or the panel. */
  leave: () => void;
  /** The pointer entered the panel. */
  enterPanel: () => void;
  /** Keeps the panel open until it is dismissed. */
  pin: () => void;
  /**
   * Call first when a trigger is clicked. Returns true when the click only pinned the panel that hover had opened for
   * this trigger, so the caller should not toggle it closed. Any other click pins whatever the caller opens next.
   */
  claimClick: (triggerPanelOpen: boolean) => boolean;
  /** Drops any pending open or close, for example when the panel closes another way. */
  cancel: () => void;
}

export function createHoverMenu(options: {
  isOpen: () => boolean;
  close: () => void;
  scheduler?: HoverTimerScheduler;
}): HoverMenu {
  const delay = createHoverDelay(options.scheduler);
  let pinned = false;

  return {
    enterTrigger(event, open) {
      if (event.pointerType === "touch") return;
      if (options.isOpen()) {
        delay.cancel();
        open();
        return;
      }
      delay.schedule(() => {
        pinned = false;
        open();
      }, SUBMENU_OPEN_DELAY_MS);
    },
    leave() {
      if (!options.isOpen() || pinned) {
        delay.cancel();
        return;
      }
      delay.schedule(options.close, SUBMENU_CLOSE_DELAY_MS);
    },
    enterPanel() {
      delay.cancel();
    },
    pin() {
      delay.cancel();
      pinned = true;
    },
    claimClick(triggerPanelOpen) {
      delay.cancel();
      const hoverOpened = triggerPanelOpen && !pinned;
      pinned = true;
      return hoverOpened;
    },
    cancel() {
      delay.cancel();
    },
  };
}

export interface MenuAimPoint {
  x: number;
  y: number;
}

export interface MenuAimRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export type MenuAimSide = "bottom" | "left" | "right" | "top";

export interface MenuAimInput {
  origin: MenuAimPoint | null;
  point: MenuAimPoint;
  submenu: MenuAimRect;
  side: MenuAimSide;
  tolerance?: number;
  topTolerance?: number;
  bottomTolerance?: number;
  minTowardDistance?: number;
}

function finitePoint(point: MenuAimPoint | null): point is MenuAimPoint {
  return point !== null && Number.isFinite(point.x) && Number.isFinite(point.y);
}

function finiteRect(rect: MenuAimRect): boolean {
  return Number.isFinite(rect.left)
    && Number.isFinite(rect.right)
    && Number.isFinite(rect.top)
    && Number.isFinite(rect.bottom)
    && rect.right >= rect.left
    && rect.bottom >= rect.top;
}

function signedArea(a: MenuAimPoint, b: MenuAimPoint, c: MenuAimPoint): number {
  return (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
}

function pointInTriangle(
  point: MenuAimPoint,
  a: MenuAimPoint,
  b: MenuAimPoint,
  c: MenuAimPoint,
): boolean {
  const epsilon = 0.001;
  const ab = signedArea(a, b, point);
  const bc = signedArea(b, c, point);
  const ca = signedArea(c, a, point);
  const hasNegative = ab < -epsilon || bc < -epsilon || ca < -epsilon;
  const hasPositive = ab > epsilon || bc > epsilon || ca > epsilon;
  return !(hasNegative && hasPositive);
}

export function isPointerAimingAtSubmenu(input: MenuAimInput): boolean {
  const origin = input.origin;
  const point = input.point;
  const submenu = input.submenu;
  if (!finitePoint(origin) || !finitePoint(point) || !finiteRect(submenu)) return false;
  if (
    point.x >= submenu.left
    && point.x <= submenu.right
    && point.y >= submenu.top
    && point.y <= submenu.bottom
  ) return true;

  const tolerance = Math.max(0, input.tolerance ?? 10);
  const topTolerance = Math.max(0, input.topTolerance ?? tolerance);
  const bottomTolerance = Math.max(0, input.bottomTolerance ?? tolerance);
  const minTowardDistance = Math.max(0, input.minTowardDistance ?? 4);
  if (input.side === "left" || input.side === "right") {
    const targetX = input.side === "right"
      ? submenu.left + tolerance
      : submenu.right - tolerance;
    const movedTowardTarget = input.side === "right"
      ? point.x >= origin.x + minTowardDistance
      : point.x <= origin.x - minTowardDistance;
    if (!movedTowardTarget) return false;

    return pointInTriangle(
      point,
      origin,
      { x: targetX, y: submenu.top - topTolerance },
      { x: targetX, y: submenu.bottom + bottomTolerance },
    );
  }

  const targetY = input.side === "bottom"
    ? submenu.top + tolerance
    : submenu.bottom - tolerance;
  const movedTowardTarget = input.side === "bottom"
    ? point.y >= origin.y + minTowardDistance
    : point.y <= origin.y - minTowardDistance;
  if (!movedTowardTarget) return false;

  return pointInTriangle(
    point,
    origin,
    { x: submenu.left - tolerance, y: targetY },
    { x: submenu.right + tolerance, y: targetY },
  );
}
