import { PALETTE_SIZE, type EventColor } from "$lib/calendar/types";
import { movedToIndex } from "$lib/utils/sidebar-order";
import type { QuickNoteTagReorderRequest } from "./types";

const QUICK_NOTE_VIEW_SHORTCUT_COUNT = 10;

/** Returns the visible numeric shortcut for a Quick notes view position. */
export function quickNoteViewShortcut(index: number): string | null {
  if (!Number.isInteger(index) || index < 0 || index >= QUICK_NOTE_VIEW_SHORTCUT_COUNT) return null;
  return index === QUICK_NOTE_VIEW_SHORTCUT_COUNT - 1 ? "0" : String(index + 1);
}

/** Resolves a numeric key to its zero-based Quick notes view position. */
export function quickNoteViewIndexForKey(key: string): number | null {
  if (key === "0") return QUICK_NOTE_VIEW_SHORTCUT_COUNT - 1;
  if (!/^[1-9]$/u.test(key)) return null;
  return Number(key) - 1;
}

function compareCodeUnits(left: string, right: string): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

/**
 * Orders tags like the native list: by order key, then id. Order keys are ASCII, so code unit
 * comparison matches the database's byte order.
 */
export function compareQuickNoteTags(
  left: { orderKey: string; id: string },
  right: { orderKey: string; id: string },
): number {
  return compareCodeUnits(left.orderKey, right.orderKey) || compareCodeUnits(left.id, right.id);
}

/**
 * Picks a palette slot for a new tag at random, avoiding the default note color and the colors other tags use, so a
 * new tag stands apart from untagged notes. Once every other slot is taken, only the default color is avoided.
 */
export function pickQuickNoteTagColor(
  usedColors: readonly EventColor[],
  defaultColor: EventColor,
  random: () => number = Math.random,
): EventColor {
  const used = new Set(usedColors);
  const others = paletteSlots().filter((color) => color !== defaultColor);
  const unused = others.filter((color) => !used.has(color));
  const pool = unused.length > 0 ? unused : others;
  return pool[Math.min(pool.length - 1, Math.max(0, Math.floor(random() * pool.length)))] ?? defaultColor;
}

function paletteSlots(): EventColor[] {
  return Array.from({ length: PALETTE_SIZE }, (_, index) => index);
}

/**
 * Builds the native request that moves `id` to `targetIndex` in `orderedIds`, naming its new neighbors, or returns
 * null when `id` is missing or would not move.
 */
export function quickNoteTagMoveRequest(
  orderedIds: readonly string[],
  id: string,
  targetIndex: number,
): QuickNoteTagReorderRequest | null {
  const sourceIndex = orderedIds.indexOf(id);
  if (sourceIndex < 0) return null;
  const next = movedToIndex(orderedIds, id, targetIndex);
  const index = next.indexOf(id);
  if (index === sourceIndex) return null;
  return { id, previousId: next[index - 1] ?? null, nextId: next[index + 1] ?? null };
}

/**
 * Counts how many leading tags fit in a row `available` pixels wide, with `gap` between items. When some do not fit,
 * the row also keeps room for the button that lists the rest, `moreWidth` wide.
 */
export function quickNoteTagVisibleCount(
  widths: readonly number[],
  available: number,
  gap: number,
  moreWidth: number,
): number {
  if (rowWidth(widths, gap) <= available) return widths.length;
  let count = 0;
  let used = moreWidth;
  for (const width of widths) {
    used += width + gap;
    if (used > available) break;
    count += 1;
  }
  return count;
}

function rowWidth(widths: readonly number[], gap: number): number {
  return widths.reduce((total, width) => total + width, 0) + Math.max(0, widths.length - 1) * gap;
}

/**
 * Classes of a Quick notes view chip, which paints the `--tag-bg` and `--tag-fg` colors it is given. Only the selected
 * chip shows its full color; the others show a faint tint of theirs, so the selection reads without a ring.
 */
export function quickNoteChipClass(mobileLayout: boolean, selected: boolean): string {
  const size = mobileLayout ? "min-h-12 max-w-40 rounded-xl px-4 text-sm" : "h-7 max-w-32 rounded-md px-2.5 text-xs";
  const tone = selected
    ? "bg-(--tag-bg) text-(--tag-fg) hover:brightness-95"
    : "bg-(--tag-bg)/30 text-foreground hover:bg-(--tag-bg)/50";
  return `flex shrink-0 items-center transition-[filter,background-color,color] ${tone} ${size}`;
}

/**
 * Steps from the current Quick notes view to the next or previous one among All (null) and the tags, wrapping at
 * the ends like workspace tabs. `currentTagId` is null for All and undefined outside them, as in Archive or Trash,
 * where stepping forward lands on All and stepping back on the last view.
 */
export function quickNoteRelativeViewTagId(
  tagIds: readonly string[],
  currentTagId: string | null | undefined,
  direction: -1 | 1,
): string | null {
  const views: (string | null)[] = [null, ...tagIds];
  const current = currentTagId === undefined ? -1 : views.indexOf(currentTagId);
  const next = current < 0
    ? (direction === 1 ? 0 : views.length - 1)
    : (current + direction + views.length) % views.length;
  return views[next] ?? null;
}
