export type ProjectListDropPosition = "before" | "after";
export type ProjectListSortDirection = "asc" | "desc";

const DEFAULT_DRAG_THRESHOLD_PX = 4;
const DEFAULT_DRAG_HOLD_MS = 120;

export interface ProjectListDragTask {
  id: string;
  sectionSortOrder: number;
}

export interface ProjectListDragSection {
  id: string;
  sortOrder: number;
}

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
  orderedTasks: readonly ProjectListDragTask[];
  draggedTaskId: string;
  overTaskId?: string;
  position?: ProjectListDropPosition;
  sortDirection: ProjectListSortDirection;
}

export interface ProjectListSectionDropInput {
  orderedSections: readonly ProjectListDragSection[];
  draggedSectionId: string;
  overSectionId?: string;
  position?: ProjectListDropPosition;
}

const SORT_STEP = 1000;

function midpoint(a: number, b: number): number {
  return a + (b - a) / 2;
}

function orderBeforeFirst(first: ProjectListDragTask, direction: ProjectListSortDirection): number {
  if (direction === "desc") return first.sectionSortOrder + SORT_STEP;
  return first.sectionSortOrder > 0 ? first.sectionSortOrder / 2 : 0;
}

function orderAfterLast(last: ProjectListDragTask, direction: ProjectListSortDirection): number {
  if (direction === "desc") return last.sectionSortOrder > 0 ? last.sectionSortOrder / 2 : 0;
  return last.sectionSortOrder + SORT_STEP;
}

function orderBetween(
  before: ProjectListDragTask,
  after: ProjectListDragTask,
  direction: ProjectListSortDirection,
): number {
  return direction === "desc"
    ? midpoint(after.sectionSortOrder, before.sectionSortOrder)
    : midpoint(before.sectionSortOrder, after.sectionSortOrder);
}

export function projectListDropSortOrder(input: ProjectListDropInput): number {
  const candidates = input.orderedTasks.filter((task) => task.id !== input.draggedTaskId);
  const overIndex = input.overTaskId
    ? candidates.findIndex((task) => task.id === input.overTaskId)
    : -1;
  const insertIndex = overIndex >= 0
    ? overIndex + (input.position === "after" ? 1 : 0)
    : candidates.length;

  const before = candidates[insertIndex - 1];
  const after = candidates[insertIndex];
  if (before && after) return orderBetween(before, after, input.sortDirection);
  if (after) return orderBeforeFirst(after, input.sortDirection);
  if (before) return orderAfterLast(before, input.sortDirection);
  return SORT_STEP;
}

export function projectListSectionDropSortOrder(input: ProjectListSectionDropInput): number {
  const candidates = input.orderedSections.filter((section) => section.id !== input.draggedSectionId);
  const overIndex = input.overSectionId
    ? candidates.findIndex((section) => section.id === input.overSectionId)
    : -1;
  const insertIndex = overIndex >= 0
    ? overIndex + (input.position === "after" ? 1 : 0)
    : candidates.length;

  const before = candidates[insertIndex - 1];
  const after = candidates[insertIndex];
  if (before && after) return midpoint(before.sortOrder, after.sortOrder);
  if (after) return after.sortOrder > 0 ? after.sortOrder / 2 : 0;
  if (before) return before.sortOrder + SORT_STEP;
  return SORT_STEP;
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
