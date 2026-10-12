export type ProjectSettingsDropPosition = "before" | "after";

export interface ProjectSettingsDropTargetInput {
  sourceIndex: number;
  targetIndex: number;
  position: ProjectSettingsDropPosition;
  itemCount: number;
}

export interface ProjectSettingsDropMarkerInput {
  draggedId: string | null;
  targetId: string;
  overId: string | null;
  currentPosition: ProjectSettingsDropPosition | null;
  markerPosition: ProjectSettingsDropPosition;
}

export interface ProjectSettingsDragOverPlanInput {
  draggedId: string | null;
  targetId: string;
  reorderPending: boolean;
  position: ProjectSettingsDropPosition;
  dropAllowed?: boolean;
}

export interface ProjectSettingsDragOverPlan {
  preventDefault: boolean;
  overId: string | null;
  position: ProjectSettingsDropPosition | null;
}

export interface MoveProjectSettingsEntryInput<T extends { id: string }> {
  entryId: string;
  targetIndex: number;
  getEntries: () => readonly T[];
  moveEntry: (entry: T, direction: -1 | 1) => Promise<void>;
}

export interface ProjectSettingsDragDataTransfer {
  effectAllowed: DataTransfer["effectAllowed"];
  dropEffect: DataTransfer["dropEffect"];
  getData: DataTransfer["getData"];
  setData: DataTransfer["setData"];
}

export interface ProjectSettingsDragEventLike {
  preventDefault: () => void;
  dataTransfer?: ProjectSettingsDragDataTransfer | null;
}

export interface ProjectSettingsEntryDropInput<T extends { id: string }> {
  entries: readonly T[];
  sourceId: string;
  targetId: string;
  position: ProjectSettingsDropPosition;
}

export interface ProjectSettingsDraggedEntryDropInput<T extends { id: string }> {
  event: ProjectSettingsDragEventLike;
  dataType: string;
  activeDraggedId: string | null;
  entries: readonly T[];
  targetId: string;
  position: ProjectSettingsDropPosition;
}

export interface ProjectSettingsDraggedEntryDropTarget {
  entryId: string;
  targetIndex: number;
}

export function projectSettingsStartDrag(
  event: ProjectSettingsDragEventLike,
  dataType: string,
  entryId: string,
): void {
  const transfer = event.dataTransfer;
  if (!transfer) return;
  transfer.effectAllowed = "move";
  transfer.setData(dataType, entryId);
  transfer.setData("text/plain", entryId);
}

export function projectSettingsDraggedEntryId(
  event: ProjectSettingsDragEventLike,
  dataType: string,
  activeDraggedId: string | null,
): string | null {
  return activeDraggedId
    ?? event.dataTransfer?.getData(dataType)
    ?? event.dataTransfer?.getData("text/plain")
    ?? null;
}

export function projectSettingsDragOverPlan(
  input: ProjectSettingsDragOverPlanInput,
): ProjectSettingsDragOverPlan | null {
  if (!input.draggedId || input.reorderPending || input.dropAllowed === false) return null;
  if (input.draggedId === input.targetId) {
    return {
      preventDefault: false,
      overId: null,
      position: null,
    };
  }
  return {
    preventDefault: true,
    overId: input.targetId,
    position: input.position,
  };
}

export function projectSettingsEntryDropTargetIndex<T extends { id: string }>(
  input: ProjectSettingsEntryDropInput<T>,
): number | null {
  if (!input.sourceId || input.sourceId === input.targetId) return null;
  return projectSettingsDropTargetIndex({
    sourceIndex: input.entries.findIndex((entry) => entry.id === input.sourceId),
    targetIndex: input.entries.findIndex((entry) => entry.id === input.targetId),
    position: input.position,
    itemCount: input.entries.length,
  });
}

export function projectSettingsDraggedEntryDropTarget<T extends { id: string }>(
  input: ProjectSettingsDraggedEntryDropInput<T>,
): ProjectSettingsDraggedEntryDropTarget | null {
  const entryId = projectSettingsDraggedEntryId(
    input.event,
    input.dataType,
    input.activeDraggedId,
  );
  if (!entryId) return null;
  const targetIndex = projectSettingsEntryDropTargetIndex({
    entries: input.entries,
    sourceId: entryId,
    targetId: input.targetId,
    position: input.position,
  });
  if (targetIndex === null) return null;
  return { entryId, targetIndex };
}

export function projectSettingsDropPosition(
  clientY: number,
  targetTop: number,
  targetHeight: number,
): ProjectSettingsDropPosition {
  return clientY < targetTop + targetHeight / 2 ? "before" : "after";
}

export function projectSettingsDropTargetIndex(
  input: ProjectSettingsDropTargetInput,
): number | null {
  const { sourceIndex, itemCount } = input;
  let { targetIndex } = input;
  if (itemCount <= 0 || sourceIndex < 0 || targetIndex < 0) return null;

  if (input.position === "after") targetIndex += 1;
  if (sourceIndex < targetIndex) targetIndex -= 1;

  const boundedTargetIndex = Math.max(0, Math.min(itemCount - 1, targetIndex));
  return boundedTargetIndex === sourceIndex ? null : boundedTargetIndex;
}

export function projectSettingsDropMarkerVisible(
  input: ProjectSettingsDropMarkerInput,
): boolean {
  return input.draggedId !== null
    && input.draggedId !== input.targetId
    && input.overId === input.targetId
    && input.currentPosition === input.markerPosition;
}

export async function moveProjectSettingsEntryToIndex<T extends { id: string }>(
  input: MoveProjectSettingsEntryInput<T>,
): Promise<boolean> {
  let currentIndex = input.getEntries().findIndex((entry) => entry.id === input.entryId);
  let remainingMoves = input.getEntries().length;

  while (currentIndex >= 0 && currentIndex !== input.targetIndex && remainingMoves > 0) {
    const direction: -1 | 1 = currentIndex < input.targetIndex ? 1 : -1;
    const entry = input.getEntries()[currentIndex];
    if (!entry) break;
    await input.moveEntry(entry, direction);
    currentIndex = input.getEntries().findIndex((nextEntry) => nextEntry.id === input.entryId);
    remainingMoves -= 1;
  }

  return currentIndex === input.targetIndex;
}
