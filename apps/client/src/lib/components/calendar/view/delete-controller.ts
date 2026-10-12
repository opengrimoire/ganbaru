import {
  dismissCalendarDeleteUndo,
  type CalendarCommitReceipt, type CalendarDeleteOutcome, type CalendarEditPreview,
} from "$lib/api/calendar-edit";
import type { CalendarEvent } from "$lib/calendar/types";
import type { CalendarEditDraft, NativeCalendarEditController } from "./native-edit-controller.svelte";
import type { getCalendar } from "$lib/stores/calendar.svelte";
import type { createCalendarViewToastController } from "./toasts.svelte";
import type { computeViewWindow } from "$lib/calendar/utils";

export interface CalendarViewDeleteControllerOptions {
  calendarStore: Pick<ReturnType<typeof getCalendar>, "acceptNativeEdit" | "refreshWindow">;
  nativeEditor: Pick<NativeCalendarEditController, "commitReviewed" | "undoDeletion" | "pendingKind"
    | "retainedPreview" | "retryPending" | "acceptedResultAgeMs" | "acknowledgeResult" | "acceptsContext">;
  toasts: Pick<ReturnType<typeof createCalendarViewToastController>, "showDeletePendingToast" | "showDeleteUndoToast"
    | "dismissDeleteToastIfPending" | "dismissDeleteUndoToast" | "showSavePendingToast" | "showSaveErrorToast">;
  getWindow: () => ReturnType<typeof computeViewWindow>;
  getEvents: () => CalendarEvent[];
  canPresent: (review?: CalendarEditPreview) => boolean;
  isCurrent: (input: CalendarEditDraft) => boolean;
  setCommitState: (state: { hidden: boolean; suppressPreview: boolean; frozenEvents: CalendarEvent[] | null }) => void;
  closeSession: () => void;
  pendingLabel: (outcome: CalendarDeleteOutcome) => string;
  outcomeLabel: (outcome: CalendarDeleteOutcome) => string;
  errorLabel: (error: unknown) => string;
}

/** Present native deletion and Undo receipts without rebuilding domain snapshots. */
export class CalendarViewDeleteController {
  private undo: { preview: CalendarEditPreview; receipt: CalendarCommitReceipt } | null = null;
  private busy = false;

  constructor(private readonly options: CalendarViewDeleteControllerOptions) {}

  async execute(input: CalendarEditDraft, review: CalendarEditPreview, stopActive: boolean): Promise<void> {
    if (this.busy) return;
    if (!review.deletion || !this.options.isCurrent(input)) throw new Error("Calendar deletion selection changed");
    this.busy = true;
    this.undo = null;
    const outcome = review.deletion.outcome;
    this.options.setCommitState({ hidden: true, suppressPreview: true,
      frozenEvents: [...this.options.getEvents().filter((event) =>
        (event.recurringParentId ?? event.id) !== review.sourceId), ...review.window.windowEvents] });
    const toastId = this.options.toasts.showDeletePendingToast(this.options.pendingLabel(outcome));
    try {
      const receipt = await this.options.nativeEditor.commitReviewed(input, review, stopActive);
      await this.complete(receipt, review, "delete", toastId, input);
    } catch (error) {
      this.report(error);
    } finally {
      this.busy = false;
      this.options.toasts.dismissDeleteToastIfPending(toastId);
      this.options.setCommitState({ hidden: false, suppressPreview: false, frozenEvents: null });
    }
  }

  /** Resolve the original operation after transport loss, refresh failure or remount. */
  async retry(): Promise<void> {
    if (this.busy) return;
    const editor = this.options.nativeEditor;
    const review = editor.retainedPreview;
    const kind = editor.pendingKind;
    if (!review || !review.deletion || (kind !== "delete" && kind !== "undo_delete")) return;
    this.busy = true;
    const toastId = this.options.toasts.showDeletePendingToast(this.options.pendingLabel(review.deletion.outcome));
    try {
      await this.complete(await editor.retryPending(), review, kind, toastId);
    } catch (error) { this.report(error); }
    finally { this.busy = false; this.options.toasts.dismissDeleteToastIfPending(toastId); }
  }

  async undoCurrent(): Promise<void> {
    if (this.busy) return;
    const undo = this.undo;
    if (!undo || !undo.preview.deletion) return;
    this.busy = true;
    this.undo = null;
    this.options.toasts.dismissDeleteUndoToast();
    const toastId = this.options.toasts.showDeletePendingToast(this.options.pendingLabel(undo.preview.deletion.outcome));
    try {
      await this.complete(await this.options.nativeEditor.undoDeletion(undo.preview, undo.receipt),
        undo.preview, "undo_delete", toastId);
    } catch (error) { this.report(error); }
    finally { this.busy = false; this.options.toasts.dismissDeleteToastIfPending(toastId); }
  }

  async dismissUndo(): Promise<void> {
    const undo = this.undo;
    this.undo = null;
    this.options.toasts.dismissDeleteUndoToast();
    if (!undo) return;
    try {
      await dismissCalendarDeleteUndo({ vaultId: undo.preview.vaultId,
        vaultGeneration: undo.preview.vaultGeneration, deleteCommandId: undo.receipt.commandId });
    } catch (error) { this.report(error); }
  }

  private async complete(receipt: CalendarCommitReceipt, review: CalendarEditPreview,
    kind: "delete" | "undo_delete", toastId: string, input?: CalendarEditDraft): Promise<void> {
    // Unmounted views leave acceptance pending for the next mounted projection.
    if (!this.options.canPresent(review)) return;
    this.options.calendarStore.acceptNativeEdit();
    const window = this.options.getWindow();
    await this.options.calendarStore.refreshWindow(window.start, window.end);
    if (!this.options.canPresent(review)) return;
    const remainingMs = Math.floor((receipt.undoAvailableForMs ?? 0) - this.options.nativeEditor.acceptedResultAgeMs);
    if (input && this.options.isCurrent(input)) this.options.closeSession();
    this.options.nativeEditor.acknowledgeResult(receipt.commandId);
    if (kind === "delete" && review.deletion) {
      if (receipt.undoReviewRevision && remainingMs > 0 && this.options.nativeEditor.acceptsContext(review)) this.undo = { preview: review, receipt };
      this.options.toasts.showDeleteUndoToast(toastId, this.options.outcomeLabel(review.deletion.outcome),
        this.undo ? () => this.undoCurrent() : undefined, remainingMs);
    }
  }

  private report(error: unknown): void {
    if (!this.options.canPresent()) return;
    const id = this.options.toasts.showSavePendingToast(this.options.errorLabel(error));
    this.options.toasts.showSaveErrorToast(id, this.options.errorLabel(error));
  }
}
