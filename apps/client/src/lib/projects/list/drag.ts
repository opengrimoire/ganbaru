import { dropPlacement } from "$lib/projects/order";
import type { ProjectOrderPlacement } from "$lib/projects/types";

export type ProjectListDropPosition = "before" | "after";
export type ProjectListSortDirection = "asc" | "desc";

const DEFAULT_DRAG_THRESHOLD_PX = 4;
const DEFAULT_DRAG_HOLD_MS = 120;

export interface ProjectListPointerDragGesture {
  itemId: string;
  pointerId: number;
  startX: number;
  startY: number;
  startedAt: number;
}

export interface ProjectListDraggableTask {
  id: string;
  projectId: string;
  archivedAt?: string;
  parentTaskId?: string;
}

export interface ProjectListDraggableSection {
  id: string;
  projectId: string;
  archivedAt?: string;
  hiddenAt?: string;
}

export interface ProjectListDropInput {
  /** Tasks of the destination section in their visible order. */
  orderedTasks: readonly { id: string }[];
  draggedTaskId: string;
  overTaskId?: string;
  position?: ProjectListDropPosition;
  sortDirection: ProjectListSortDirection;
}

export interface ProjectListSectionDropInput {
  /** Sections of the project in their visible order. */
  orderedSections: readonly { id: string }[];
  draggedSectionId: string;
  overSectionId?: string;
  position?: ProjectListDropPosition;
}

/** The section placement for a task drop, or null when the task would keep its place in the section. */
export function projectListDropPlacement(input: ProjectListDropInput): ProjectOrderPlacement | null {
  return dropPlacement({
    visibleIds: input.orderedTasks.map((task) => task.id),
    draggedId: input.draggedTaskId,
    overId: input.overTaskId,
    position: input.position,
    direction: input.sortDirection,
  });
}

/** The placement for a section drop, or null when the section would keep its place. */
export function projectListSectionDropPlacement(input: ProjectListSectionDropInput): ProjectOrderPlacement | null {
  return dropPlacement({
    visibleIds: input.orderedSections.map((section) => section.id),
    draggedId: input.draggedSectionId,
    overId: input.overSectionId,
    position: input.position,
    direction: "asc",
  });
}

export function projectListPointerDragGestureReady(input: {
  gesture: ProjectListPointerDragGesture | null;
  itemId: string;
  clientX: number;
  clientY: number;
  now: number;
  thresholdPx?: number;
  holdMs?: number;
}): boolean {
  const gesture = input.gesture;
  if (!gesture || gesture.itemId !== input.itemId) return false;
  const thresholdPx = input.thresholdPx ?? DEFAULT_DRAG_THRESHOLD_PX;
  const holdMs = input.holdMs ?? DEFAULT_DRAG_HOLD_MS;
  const distance = Math.hypot(input.clientX - gesture.startX, input.clientY - gesture.startY);
  const elapsed = input.now - gesture.startedAt;
  return distance >= thresholdPx
    || (elapsed >= holdMs && distance >= 1);
}

export function projectListTaskDragAllowed(input: {
  dragEnabled: boolean;
  task: ProjectListDraggableTask;
  dropPendingTaskId: string | null;
}): boolean {
  return input.dragEnabled
    && !input.task.archivedAt
    && !input.task.parentTaskId
    && input.dropPendingTaskId === null;
}

export function projectListSectionDragAllowed(input: {
  dragEnabled: boolean;
  section: ProjectListDraggableSection;
  dropPendingSectionId: string | null;
}): boolean {
  return input.dragEnabled
    && !input.section.archivedAt
    && !input.section.hiddenAt
    && input.dropPendingSectionId === null;
}

export function projectListTaskDropAllowed(input: {
  dragEnabled: boolean;
  task: ProjectListDraggableTask | undefined;
  targetSection: ProjectListDraggableSection;
}): boolean {
  return input.dragEnabled
    && !!input.task
    && !input.task.archivedAt
    && !input.task.parentTaskId
    && !input.targetSection.archivedAt
    && !input.targetSection.hiddenAt
    && input.task.projectId === input.targetSection.projectId;
}

export function projectListSectionDropAllowed(input: {
  dragEnabled: boolean;
  draggedSection: ProjectListDraggableSection | undefined;
  targetSection: ProjectListDraggableSection;
}): boolean {
  return input.dragEnabled
    && !!input.draggedSection
    && !input.draggedSection.archivedAt
    && !input.draggedSection.hiddenAt
    && !input.targetSection.archivedAt
    && !input.targetSection.hiddenAt
    && input.draggedSection.projectId === input.targetSection.projectId;
}

export function projectListDropPositionFromPoint(
  clientY: number,
  rect: Pick<DOMRect, "top" | "height">,
): ProjectListDropPosition {
  return clientY >= rect.top + rect.height / 2 ? "after" : "before";
}
