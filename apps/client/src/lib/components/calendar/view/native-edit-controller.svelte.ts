import {
  CalendarCommitFailure, commitCalendarEdit, previewCalendarEdit,
  type CalendarCommitReceipt, type CalendarCommitRequest, type CalendarEditIntent, type CalendarMutationIntent,
  type CalendarEditPreview, type CalendarPreviewRequest,
} from "$lib/api/calendar-edit";
import { activeVaultIdentity } from "$lib/vault/active-vault";

export interface CalendarEditContext {
  vaultId: string;
  vaultGeneration: number;
}

export interface CalendarEditDraft extends CalendarEditContext {
  sessionKey: number;
  edit: CalendarMutationIntent;
  window: CalendarPreviewRequest["window"];
}

const PREVIEW_DELAY_MS = 200;

interface PendingPreview {
  input: CalendarEditDraft;
  key: string;
  generation: number;
  commandId: string;
}

interface PendingCommit {
  intentKey: string;
  request: CalendarCommitRequest;
  preview: CalendarEditPreview;
}

function intentKey(input: CalendarEditDraft): string {
  // View navigation does not alter an already reviewed durable intent.
  return JSON.stringify([input.vaultId, input.sessionKey, input.edit]);
}

/** Coalesce native reviews and retain immutable commit requests after uncertain replies. */
export class NativeCalendarEditController {
  preview = $state.raw<CalendarEditPreview | null>(null);
  loading = $state(false);
  committing = $state(false);
  uncertain = $state(false);
  error = $state<string | null>(null);
  resultPending = $state(false);
  private generation = 0;
  private commandId: string;
  private desired: PendingPreview | null = null;
  private running: Promise<void> | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private failedGeneration: number | null = null;
  private lastError: unknown;
  private pendingCommit: PendingCommit | null = null;
  private accepted: PendingCommit & { receipt: CalendarCommitReceipt; receivedAt: number } | null = null;

  constructor(
    private readonly context: () => CalendarEditContext | null,
    private readonly loadPreview = previewCalendarEdit,
    private readonly writeCommit = commitCalendarEdit,
    private readonly newCommandId: () => string = () => crypto.randomUUID(),
    private readonly vaultIdentity: () => string | null = () => this.context()?.vaultId ?? null,
  ) {
    this.commandId = this.newCommandId();
  }

  /** Replace pending work, with at most one IPC preview and one latest draft retained. */
  update(input: CalendarEditDraft | null): void {
    if (!input) {
      this.reset();
      return;
    }
    const key = JSON.stringify(input);
    if (this.resultPending && this.accepted?.intentKey !== intentKey(input)) {
      throw new CalendarCommitFailure("unknown", "Refresh the accepted Calendar operation before starting another deletion");
    }
    if (key === this.desired?.key) return;
    if (this.desired && (this.desired.input.sessionKey !== input.sessionKey
      || this.desired.input.vaultId !== input.vaultId)) this.commandId = this.newCommandId();
    if ("kind" in input.edit && this.pendingCommit && this.pendingCommit.intentKey !== intentKey(input)
      && this.desired && intentKey(this.desired.input) !== intentKey(input)) {
      // A changed creation draft cannot borrow the identity of an uncertain write.
      this.commandId = this.newCommandId();
    }
    if (this.accepted && this.accepted.intentKey !== intentKey(input)) {
      this.accepted = null;
      this.commandId = this.newCommandId();
    }
    this.desired = { input: structuredClone($state.snapshot(input)), key,
      generation: ++this.generation, commandId: this.commandId };
    this.preview = null;
    this.error = null;
    this.failedGeneration = null;
    this.loading = true;
    this.schedule();
  }

  private isCurrentContext(input: CalendarEditContext): boolean {
    const context = this.context();
    return context?.vaultId === input.vaultId && context.vaultGeneration === input.vaultGeneration
      && this.vaultIdentity() === input.vaultId;
  }

  /** Availability belongs to the current native process generation. */
  acceptsContext(input: CalendarEditContext): boolean {
    return this.isCurrentContext(input);
  }

  /** Read contours only for the exact current draft, including before the next effect flush. */
  previewFor(input: CalendarEditDraft): CalendarEditPreview | null {
    const preview = this.preview;
    return preview && this.desired?.key === JSON.stringify(input) && this.isCurrentContext(input) ? preview : null;
  }

  private clearTimer(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }

  private schedule(): void {
    this.clearTimer();
    if (this.running || !this.desired) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.start();
    }, PREVIEW_DELAY_MS);
  }

  private start(): Promise<void> {
    if (this.running) return this.running;
    const work = this.desired;
    if (!work) return Promise.resolve();
    this.clearTimer();
    this.running = this.read(work).finally(() => {
      this.running = null;
      if (this.desired && this.desired.generation !== work.generation) this.schedule();
    });
    return this.running;
  }

  private async read(work: PendingPreview): Promise<void> {
    try {
      const preview = await this.loadPreview({ commandId: work.commandId,
        edit: work.input.edit, window: work.input.window });
      if (this.desired?.generation !== work.generation || !this.isCurrentContext(work.input)) return;
      if (preview.vaultId !== work.input.vaultId || preview.vaultGeneration !== work.input.vaultGeneration) {
        throw new Error("Calendar review belongs to a previous vault context");
      }
      this.preview = preview;
    } catch (error) {
      if (this.desired?.generation !== work.generation || !this.isCurrentContext(work.input)) return;
      this.lastError = error;
      this.failedGeneration = work.generation;
      this.error = error instanceof Error ? error.message : String(error);
    } finally {
      if (this.desired?.generation === work.generation) this.loading = false;
    }
  }

  /** Wait for this exact draft, never for a newer editor generation. */
  async review(input: CalendarEditDraft): Promise<CalendarEditPreview> {
    this.update(input);
    const work = this.desired;
    if (!work) throw new Error("Calendar editor closed before review");
    this.clearTimer();
    this.failedGeneration = null;
    this.error = null;
    while (this.desired?.generation === work.generation && this.isCurrentContext(input)) {
      if (this.preview) return this.preview;
      await this.start();
      if (this.failedGeneration === work.generation) throw this.lastError;
    }
    throw new Error("Calendar editor or vault changed during review");
  }

  /** Refresh native protection immediately before opening a deletion confirmation. */
  async reviewDeletion(input: CalendarEditDraft): Promise<CalendarEditPreview> {
    if (this.pendingCommit || this.resultPending || this.committing) throw new Error("Resolve the previous Calendar operation first");
    this.desired = null;
    this.preview = null;
    return this.review(input);
  }

  /** Reuse confirmed results if only the subsequent cache refresh failed. */
  async commit(input: CalendarEditDraft): Promise<CalendarCommitReceipt> {
    if (this.committing) throw new Error("Calendar Save is already in progress");
    const key = intentKey(input);
    if (this.accepted?.intentKey === key && this.isCurrentContext(input)) return this.accepted.receipt;
    if (this.pendingCommit && (this.pendingCommit.intentKey !== key
      || this.pendingCommit.request.vaultId !== this.vaultIdentity())) {
      throw new CalendarCommitFailure("unknown", "The previous Save is unresolved; retry its original draft first");
    }
    this.committing = true;
    try {
      if (!this.pendingCommit) {
        const preview = await this.review(input);
        this.pendingCommit = { intentKey: key, preview, request: {
          vaultId: preview.vaultId, vaultGeneration: preview.vaultGeneration,
          commandId: preview.commandId, reviewRevision: preview.reviewRevision,
          edit: structuredClone($state.snapshot(input.edit)),
        } };
      }
      return await this.sendPending();
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      throw error;
    } finally {
      this.committing = false;
    }
  }

  /** Keep the retained semantic action when checking whether a retry may close the panel. */
  get pendingAction(): CalendarEditIntent["action"] {
    const intent = this.pendingCommit?.request.edit;
    return intent && "action" in intent ? intent.action : undefined;
  }

  /** Keep the exact reviewed selection through an explicit stop confirmation. */
  async commitReviewed(input: CalendarEditDraft, preview: CalendarEditPreview, stopActive: boolean): Promise<CalendarCommitReceipt> {
    if (this.committing) throw new Error("Calendar operation is already in progress");
    if (this.pendingCommit || this.resultPending) throw new CalendarCommitFailure("unknown", "Resolve the previous Calendar operation first");
    if (!("kind" in input.edit) || input.edit.kind !== "delete" || !preview.deletion
      || this.previewFor(input) !== preview || !this.isCurrentContext(input)) {
      throw new Error("Calendar deletion selection changed during confirmation");
    }
    this.pendingCommit = { intentKey: intentKey(input), preview, request: {
      vaultId: preview.vaultId, vaultGeneration: preview.vaultGeneration,
      commandId: preview.commandId, reviewRevision: preview.reviewRevision,
      edit: { ...structuredClone($state.snapshot(input.edit)), stopActive },
    } };
    this.committing = true;
    try { return await this.sendPending(); } finally { this.committing = false; }
  }

  /** Submit only the accepted deletion identity and native preimage revision. */
  async undoDeletion(preview: CalendarEditPreview, receipt: CalendarCommitReceipt): Promise<CalendarCommitReceipt> {
    if (this.committing || this.pendingCommit || this.resultPending) throw new Error("Resolve the current Calendar operation before Undo");
    if (!receipt.undoReviewRevision || !this.isCurrentContext(preview)) throw new Error("Calendar Undo belongs to an unavailable vault context");
    const request: CalendarCommitRequest = { vaultId: preview.vaultId, vaultGeneration: preview.vaultGeneration,
      commandId: this.newCommandId(), reviewRevision: receipt.undoReviewRevision,
      edit: { kind: "undo_delete", deleteCommandId: receipt.commandId } };
    this.pendingCommit = { intentKey: JSON.stringify(["undo_delete", request.vaultId, receipt.commandId]), request, preview };
    this.committing = true;
    try { return await this.sendPending(); } finally { this.committing = false; }
  }

  get pendingKind(): "edit" | "create" | "schedule_tasks" | "delete" | "undo_delete" | undefined {
    const intent = this.pendingCommit?.request.edit ?? (this.resultPending ? this.accepted?.request.edit : undefined);
    return intent ? "kind" in intent ? intent.kind : "edit" : undefined;
  }

  get retainedPreview(): CalendarEditPreview | null {
    return this.pendingCommit?.preview ?? (this.resultPending ? this.accepted?.preview ?? null : null);
  }

  get acceptedResultAgeMs(): number {
    return this.accepted ? Math.max(0, performance.now() - this.accepted.receivedAt) : 0;
  }

  /** Acknowledge only after all affected projections have refreshed successfully. */
  acknowledgeResult(commandId: string): void {
    if (this.accepted?.receipt.commandId === commandId) {
      this.resultPending = false;
      this.accepted = null;
      this.commandId = this.newCommandId();
    }
  }

  /** Closing after retry is safe only when the selected editor still has the saved intent. */
  matchesPending(input: CalendarEditDraft): boolean {
    return this.pendingCommit?.intentKey === intentKey(input);
  }

  /** Resolve the retained request even if its original panel has been closed. */
  async retryPending(): Promise<CalendarCommitReceipt> {
    if (this.committing) throw new Error("Calendar Save is already in progress");
    if (this.resultPending && this.accepted?.request.vaultId === this.vaultIdentity()) return this.accepted.receipt;
    if (!this.pendingCommit || this.pendingCommit.request.vaultId !== this.vaultIdentity()) {
      throw new Error("Return to the original Calendar vault before retrying Save");
    }
    this.committing = true;
    try {
      return await this.sendPending();
    } finally {
      this.committing = false;
    }
  }

  private async sendPending(): Promise<CalendarCommitReceipt> {
    const pending = this.pendingCommit;
    if (!pending) throw new Error("Calendar has no retained Save request");
    try {
      const receipt = await this.writeCommit(pending.request);
      this.accepted = { ...pending, receipt, receivedAt: performance.now() };
      this.resultPending = "kind" in pending.request.edit && (pending.request.edit.kind === "delete" || pending.request.edit.kind === "undo_delete");
      this.pendingCommit = null;
      this.uncertain = false;
      this.error = null;
      return receipt;
    } catch (error) {
      if (error instanceof CalendarCommitFailure && error.outcome === "rejected") {
        this.pendingCommit = null;
        this.uncertain = false;
        this.commandId = this.newCommandId();
        this.desired = null;
        this.preview = null;
      } else {
        this.uncertain = true;
      }
      this.error = error instanceof Error ? error.message : String(error);
      throw error;
    }
  }

  /** Cancel presentation work. Accepted writes and uncertain receipts remain recoverable. */
  reset(): void {
    this.clearTimer();
    this.generation += 1;
    this.desired = null;
    this.commandId = this.newCommandId();
    this.preview = null;
    this.loading = false;
    this.error = null;
    if (!this.resultPending) this.accepted = null;
  }
}

let panelController: NativeCalendarEditController | null = null;
let directController: NativeCalendarEditController | null = null;
let deleteController: NativeCalendarEditController | null = null;

/** Keep one controller per kind (panel, direct drag, deletion) so uncertain Saves survive Calendar view remounts. */
export function getNativeCalendarEditController(
  context: () => CalendarEditContext | null,
  kind: "panel" | "direct" | "delete" = "panel",
): NativeCalendarEditController {
  if (kind === "delete") return deleteController ??= new NativeCalendarEditController(context,
    undefined, undefined, undefined, activeVaultIdentity);
  if (kind === "direct") return directController ??= new NativeCalendarEditController(context,
    undefined, undefined, undefined, activeVaultIdentity);
  return panelController ??= new NativeCalendarEditController(context,
    undefined, undefined, undefined, activeVaultIdentity);
}
