/**
 * What the desktop welcome scenes' pointer hovers. A scene names the parts of its drawing that react to a hover with
 * `data-scene-hover`, and the parts drawn over others, such as an open menu, with `data-scene-cover`. The pointer then
 * hovers every named part under it, as a real one does, except what an open cover hides.
 */

import type { WelcomeCursorPoint } from "./welcome-cursor";

/** The hover names under nothing, kept as one value so a scene can tell an unchanged hover apart cheaply. */
export const WELCOME_NO_HOVER: readonly string[] = Object.freeze([]);

/** A box in client pixels. */
interface ClientBox {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

function contains(box: ClientBox, x: number, y: number): boolean {
  return x >= box.left && x < box.right && y >= box.top && y < box.bottom;
}

function drawn(element: Element): boolean {
  const rect = element.getBoundingClientRect();
  return (rect.width > 0 || rect.height > 0) && getComputedStyle(element).visibility !== "hidden";
}

/**
 * The `data-scene-hover` names under `point`, in document order, in a drawing `canvas` laid out `canvasWidth` logical
 * pixels wide and possibly zoomed. Hidden parts never hover. While an open cover is under the point, only the parts
 * inside that cover hover, so a menu drawn over a row keeps the row from hovering.
 */
export function welcomeHoverAt(canvas: HTMLElement, canvasWidth: number, point: WelcomeCursorPoint | null): readonly string[] {
  if (!point || canvasWidth <= 0) return WELCOME_NO_HOVER;
  const canvasRect = canvas.getBoundingClientRect();
  if (canvasRect.width === 0) return WELCOME_NO_HOVER;
  const scale = canvasRect.width / canvasWidth;
  const x = canvasRect.left + point.x * scale;
  const y = canvasRect.top + point.y * scale;
  if (!contains(canvasRect, x, y)) return WELCOME_NO_HOVER;
  const covers = Array.from(canvas.querySelectorAll("[data-scene-cover]")).filter(
    (cover) => drawn(cover) && contains(cover.getBoundingClientRect(), x, y),
  );
  const cover = covers.at(-1);
  const names: string[] = [];
  for (const element of canvas.querySelectorAll<HTMLElement>("[data-scene-hover]")) {
    if (cover && !cover.contains(element)) continue;
    if (!contains(element.getBoundingClientRect(), x, y) || !drawn(element)) continue;
    const name = element.dataset.sceneHover;
    if (name) names.push(name);
  }
  return names.length === 0 ? WELCOME_NO_HOVER : names;
}

/** Whether two hovers name the same parts, so a scene only redraws when what the pointer hovers changes. */
export function welcomeHoverSame(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((name, index) => name === b[index]);
}
