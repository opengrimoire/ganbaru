import {
  moveProjectSettingsEntryToIndex,
  projectSettingsDraggedEntryDropTarget,
  projectSettingsDragOverPlan,
  projectSettingsDropMarkerVisible,
  projectSettingsDropPosition,
  projectSettingsStartDrag,
  type ProjectSettingsDropPosition,
} from "./reorder";

type ReorderEntry = { id: string };

export interface ProjectSettingsMoveControllerOptions<T extends ReorderEntry> {
  getEntries: () => readonly T[];
  /** The entries `entry` moves among, when that is a subset such as the options of one field. */
  getEntriesForEntry?: (entry: T) => readonly T[];
  moveEntry: (entry: T, direction: -1 | 1) => Promise<void>;
  setError: (error: string | null) => void;
  reorderFailedMessage: () => string;
  saveFailedMessage: (message: string) => string;
}

export interface ProjectSettingsReorderControllerOptions<T extends ReorderEntry>
  extends ProjectSettingsMoveControllerOptions<T> {
  dataType: string;
  canDrop?: (draggedEntry: T, targetEntry: T) => boolean;
}

/**
 * Owns the serialized move lifecycle for one settings collection: one move at a time, reported through `pending`, with
 * failures shown through `setError`.
 */
export function createProjectSettingsMoveController<T extends ReorderEntry>(
  options: ProjectSettingsMoveControllerOptions<T>,
) {
  let pending = $state(false);

  /** Return the entries `entryId` moves among, in their current order. */
  function siblings(entryId: string): readonly T[] {
    const entry = options.getEntries().find((candidate) => candidate.id === entryId);
    return entry && options.getEntriesForEntry ? options.getEntriesForEntry(entry) : options.getEntries();
  }

  async function moveToIndex(entryId: string, targetIndex: number): Promise<void> {
    pending = true;
    options.setError(null);
    try {
      const entry = options.getEntries().find((candidate) => candidate.id === entryId);
      const getEntries = entry && options.getEntriesForEntry
        ? () => options.getEntriesForEntry?.(entry) ?? options.getEntries()
        : options.getEntries;
      const moved = await moveProjectSettingsEntryToIndex({
        entryId,
        targetIndex,
        getEntries,
        moveEntry: options.moveEntry,
      });
      if (!moved) options.setError(options.reorderFailedMessage());
    } catch (error) {
      options.setError(
        options.saveFailedMessage(error instanceof Error ? error.message : String(error)),
      );
    } finally {
      pending = false;
    }
  }

  return {
    get pending() { return pending; },
    siblings,
    moveToIndex,
  };
}

/**
 * Owns the transient native drag state and serialized move lifecycle for one settings collection.
 */
export function createProjectSettingsReorderController<T extends ReorderEntry>(
  options: ProjectSettingsReorderControllerOptions<T>,
) {
  const mover = createProjectSettingsMoveController(options);
  let draggedId = $state<string | null>(null);
  let overId = $state<string | null>(null);
  let position = $state<ProjectSettingsDropPosition | null>(null);

  function clear(): void {
    draggedId = null;
    overId = null;
    position = null;
  }

  function start(event: DragEvent, entry: T): void {
    draggedId = entry.id;
    overId = null;
    position = null;
    projectSettingsStartDrag(event, options.dataType, entry.id);
  }

  function dragOver(event: DragEvent, targetEntry: T, target: HTMLElement): void {
    const draggedEntry = options.getEntries().find((entry) => entry.id === draggedId);
    const bounds = target.getBoundingClientRect();
    const plan = projectSettingsDragOverPlan({
      draggedId,
      targetId: targetEntry.id,
      reorderPending: mover.pending,
      position: projectSettingsDropPosition(event.clientY, bounds.top, bounds.height),
      dropAllowed: draggedEntry && options.canDrop
        ? options.canDrop(draggedEntry, targetEntry)
        : undefined,
    });
    if (!plan) return;
    if (plan.preventDefault) {
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    }
    overId = plan.overId;
    position = plan.position;
  }

  function markerVisible(targetId: string, markerPosition: ProjectSettingsDropPosition): boolean {
    return projectSettingsDropMarkerVisible({
      draggedId,
      targetId,
      overId,
      currentPosition: position,
      markerPosition,
    });
  }

  async function moveToIndex(entryId: string, targetIndex: number): Promise<void> {
    try {
      await mover.moveToIndex(entryId, targetIndex);
    } finally {
      clear();
    }
  }

  async function drop(event: DragEvent, targetEntry: T): Promise<void> {
    event.preventDefault();
    const allEntries = options.getEntries();
    const draggedEntry = allEntries.find((entry) => entry.id === draggedId);
    if (draggedEntry && options.canDrop && !options.canDrop(draggedEntry, targetEntry)) {
      clear();
      return;
    }
    const target = projectSettingsDraggedEntryDropTarget({
      event,
      dataType: options.dataType,
      activeDraggedId: draggedId,
      entries: draggedEntry ? mover.siblings(draggedEntry.id) : allEntries,
      targetId: targetEntry.id,
      position: position ?? "before",
    });
    if (!target) {
      clear();
      return;
    }
    await moveToIndex(target.entryId, target.targetIndex);
  }

  return {
    get draggedId() { return draggedId; },
    get pending() { return mover.pending; },
    clear,
    start,
    dragOver,
    markerVisible,
    drop,
    moveToIndex,
  };
}
