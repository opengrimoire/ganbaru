import { dropPlacement } from "./order";
import type { ProjectOrderPlacement } from "./types";

export type ProjectKanbanDropPosition = "before" | "after";
export type ProjectKanbanSortDirection = "asc" | "desc";

export interface ProjectKanbanDropInput {
  /** Tasks of the destination status column in their visible order. */
  orderedTasks: readonly { id: string }[];
  draggedTaskId: string;
  overTaskId?: string;
  position?: ProjectKanbanDropPosition;
  sortDirection: ProjectKanbanSortDirection;
}

/** The status placement for a Kanban drop, or null when the task would keep its place in the column. */
export function projectKanbanDropPlacement(input: ProjectKanbanDropInput): ProjectOrderPlacement | null {
  return dropPlacement({
    visibleIds: input.orderedTasks.map((task) => task.id),
    draggedId: input.draggedTaskId,
    overId: input.overTaskId,
    position: input.position,
    direction: input.sortDirection,
  });
}
