import { createSortableDrag } from "$lib/utils/sortable-drag.svelte";
import { createProjectSettingsMoveController, type ProjectSettingsMoveControllerOptions } from "./reorder-controller.svelte";

/**
 * Pointer reordering for one settings collection. Dragging an entry's handle lifts the row and slides its siblings
 * aside; the drop then moves the entry through the collection's draft moves, one at a time.
 */
export function createProjectSettingsSortableController<T extends { id: string }>(
  options: ProjectSettingsMoveControllerOptions<T>,
) {
  const mover = createProjectSettingsMoveController(options);
  const drag = createSortableDrag({
    getIds: (id) => mover.siblings(id).map((entry) => entry.id),
    canStart: () => !mover.pending,
    commit: mover.moveToIndex,
  });

  return {
    get pending() { return mover.pending; },
    get draggedId() { return drag.draggedId; },
    item: drag.item,
    pointerDown: drag.pointerDown,
    reset: drag.reset,
  };
}

export type ProjectSettingsSortableController = ReturnType<typeof createProjectSettingsSortableController>;
