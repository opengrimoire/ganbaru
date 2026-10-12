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
