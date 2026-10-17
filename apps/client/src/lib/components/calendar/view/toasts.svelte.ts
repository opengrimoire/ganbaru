export interface DeleteUndoToast {
  id: string;
  pending: boolean;
  restore?: () => Promise<void>;
  label: string;
}

export interface SaveToast {
  id: string;
  pending: boolean;
  message: string;
  variant: "success" | "error";
}

const DELETE_UNDO_TIMEOUT_MS = 5_000;
const SAVE_SUCCESS_TOAST_TIMEOUT_MS = 3_000;
const SAVE_ERROR_TOAST_TIMEOUT_MS = 8_000;

export function createCalendarViewToastController() {
  let deleteUndoToast: DeleteUndoToast | null = $state(null);
  let deleteUndoTimer: ReturnType<typeof setTimeout> | undefined;
  let saveToast: SaveToast | null = $state(null);
  let saveToastTimer: ReturnType<typeof setTimeout> | undefined;

  function clearDeleteUndoTimer(): void {
    if (deleteUndoTimer) {
      clearTimeout(deleteUndoTimer);
      deleteUndoTimer = undefined;
    }
  }

  function dismissDeleteUndoToast(): void {
    clearDeleteUndoTimer();
    deleteUndoToast = null;
  }

  function showDeletePendingToast(label: string): string {
    clearDeleteUndoTimer();
    const id = crypto.randomUUID();
    deleteUndoToast = { id, pending: true, label };
    return id;
  }

  function dismissDeleteToastIfCurrent(id: string): void {
    if (deleteUndoToast?.id === id) dismissDeleteUndoToast();
  }

  function dismissDeleteToastIfPending(id: string): void {
    if (deleteUndoToast?.id === id && deleteUndoToast.pending) dismissDeleteUndoToast();
  }

  function showDeleteUndoToast(
    id: string,
    label: string,
    restore?: () => Promise<void>,
    lifetimeMs = DELETE_UNDO_TIMEOUT_MS,
  ): void {
    clearDeleteUndoTimer();
    if (deleteUndoToast?.id !== id) return;
    deleteUndoToast = { id, pending: false, restore, label };
    deleteUndoTimer = setTimeout(() => {
      if (deleteUndoToast?.id === id) deleteUndoToast = null;
      deleteUndoTimer = undefined;
    }, restore ? Math.max(0, Math.min(DELETE_UNDO_TIMEOUT_MS, lifetimeMs)) : SAVE_SUCCESS_TOAST_TIMEOUT_MS);
  }

  function clearSaveToastTimer(): void {
    if (saveToastTimer) {
      clearTimeout(saveToastTimer);
      saveToastTimer = undefined;
    }
  }

  function dismissSaveToast(): void {
    clearSaveToastTimer();
    saveToast = null;
  }

  function showSavePendingToast(message: string): string {
    clearSaveToastTimer();
    const id = crypto.randomUUID();
    saveToast = { id, pending: true, message, variant: "success" };
    return id;
  }

  function dismissSaveToastIfCurrent(id: string): void {
    if (saveToast?.id === id) dismissSaveToast();
  }

  function showSaveSuccessToast(id: string, message: string): void {
    clearSaveToastTimer();
    if (saveToast?.id !== id) return;
    saveToast = { id, pending: false, message, variant: "success" };
    saveToastTimer = setTimeout(() => {
      if (saveToast?.id === id) saveToast = null;
      saveToastTimer = undefined;
    }, SAVE_SUCCESS_TOAST_TIMEOUT_MS);
  }

  function showSaveErrorToast(id: string, message: string): void {
    clearSaveToastTimer();
    if (saveToast?.id !== id) return;
    saveToast = { id, pending: false, message, variant: "error" };
    saveToastTimer = setTimeout(() => {
      if (saveToast?.id === id) saveToast = null;
      saveToastTimer = undefined;
    }, SAVE_ERROR_TOAST_TIMEOUT_MS);
  }

  function destroy(): void {
    clearDeleteUndoTimer();
    clearSaveToastTimer();
  }

  return {
    get deleteUndoToast() {
      return deleteUndoToast;
    },
    get saveToast() {
      return saveToast;
    },
    dismissDeleteUndoToast,
    showDeletePendingToast,
    dismissDeleteToastIfCurrent,
    dismissDeleteToastIfPending,
    showDeleteUndoToast,
    dismissSaveToastIfCurrent,
    showSavePendingToast,
    showSaveSuccessToast,
    showSaveErrorToast,
    destroy,
  };
}
