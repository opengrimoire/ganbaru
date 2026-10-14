import { movedToIndex, sameStarredSiblings } from "./sidebar-order";
import { createSortableDrag } from "./sortable-drag.svelte";

export interface SidebarOrderDragOptions {
  /** Ids of the listed items, in rendered order. */
  getIds: () => readonly string[];
  /** Whether items can be moved now, which is only while the custom order is in effect. */
  enabled: () => boolean;
  /** Whether an item is starred; starred items stay ahead of the rest and move only among themselves. */
  isStarred?: (id: string) => boolean;
  /** Kind of an item when a list mixes kinds that keep separate blocks, such as folders before notes; items move only within their kind. */
  kindOf?: (id: string) => string;
  /** Saves the listed ids in their new order. */
  save: (orderedIds: string[]) => Promise<void>;
  /** Describes the list in the error logged when a save fails. */
  description: string;
}

/**
 * Drag to reorder the items of a sidebar or picker list in its custom order, plus one-step moves for menus. A mouse or
 * pen press on an item's handle lifts it once the pointer travels; touch keeps scrolling the list and uses the moves.
 * A new move waits until the previous order is saved, so a late response never overwrites a newer move.
 */
export function createSidebarOrderDrag(options: SidebarOrderDragOptions) {
  let saving = false;
  const isStarred = (id: string): boolean => options.isStarred?.(id) ?? false;
  const kindOf = (id: string): string => options.kindOf?.(id) ?? "";
  const siblingsOf = (id: string): string[] =>
    sameStarredSiblings(options.getIds(), id, isStarred).filter((other) => kindOf(other) === kindOf(id));

  async function save(orderedIds: string[]): Promise<void> {
    saving = true;
    try {
      await options.save(orderedIds);
    } catch (error: unknown) {
      console.error(`Failed to reorder ${options.description}`, error);
    } finally {
      saving = false;
    }
  }

  const drag = createSortableDrag({
    getIds: siblingsOf,
    canStart: () => options.enabled() && !saving,
    commit: (id, targetIndex) => save(movedToIndex(siblingsOf(id), id, targetIndex)),
  });

  /** Starts tracking a press on the handle of `id`, ignoring touch so the list still scrolls. */
  function pointerDown(event: PointerEvent, id: string): void {
    if (event.pointerType === "touch") return;
    drag.pointerDown(event, id);
  }

  /** Whether `id` has a sibling to swap with one place up (`-1`) or down (`1`). */
  function canMove(id: string, step: -1 | 1): boolean {
    const siblings = siblingsOf(id);
    const target = siblings.indexOf(id) + step;
    return siblings.includes(id) && target >= 0 && target < siblings.length;
  }

  /** Moves `id` one place up (`-1`) or down (`1`) among its siblings. */
  function move(id: string, step: -1 | 1): void {
    if (!options.enabled() || saving || !canMove(id, step)) return;
    const siblings = siblingsOf(id);
    void save(movedToIndex(siblings, id, siblings.indexOf(id) + step));
  }

  return {
    get draggedId() { return drag.draggedId; },
    get enabled() { return options.enabled(); },
    item: drag.item,
    pointerDown,
    canMove,
    move,
    reset: drag.reset,
  };
}

export type SidebarOrderDrag = ReturnType<typeof createSidebarOrderDrag>;
