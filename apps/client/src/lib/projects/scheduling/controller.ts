import {
  CalendarCommitFailure, commitCalendarEdit, previewCalendarEdit,
  type CalendarCommitReceipt, type CalendarCommitRequest, type CalendarPreviewRequest,
  type CalendarTaskScheduleIntent, type ScheduledTaskIdentity,
} from "$lib/api/calendar-edit";
import { activeVaultIdentity } from "$lib/vault/active-vault";

interface PendingSchedule {
  key: string;
  request: CalendarCommitRequest;
  identities: ScheduledTaskIdentity[];
}

/** Retain one immutable scheduling request across lost replies and view remounts. */
export class ProjectSchedulingController {
  private pending: PendingSchedule | null = null;
  private accepted: { key: string; receipt: CalendarCommitReceipt; vaultId: string } | null = null;
  private running = false;

  constructor(
    private readonly vault = activeVaultIdentity,
    private readonly preview = previewCalendarEdit,
    private readonly commit = commitCalendarEdit,
    private readonly commandId: () => string = () => crypto.randomUUID(),
  ) {}

  get recoverable(): boolean { return this.pending !== null || this.accepted !== null; }

  /** An accepted command survives a failed cache refresh without another write. */
  async schedule(intent: CalendarTaskScheduleIntent, window: CalendarPreviewRequest["window"]): Promise<CalendarCommitReceipt> {
    const vaultId = this.vault();
    if (!vaultId) throw new Error("Scheduling requires an active vault");
    const key = JSON.stringify([vaultId, intent]);
    if (this.accepted?.key === key) return structuredClone(this.accepted.receipt);
    if (this.accepted) throw new Error("Refresh the accepted schedule before scheduling another selection");
    if (this.pending) throw new Error("The previous schedule is unresolved; retry it first");
    if (this.running) throw new Error("Project scheduling is already in progress");
    this.running = true;
    try {
      const edit = structuredClone(intent);
      const preview = await this.preview({ commandId: this.commandId(), edit, window });
      if (this.vault() !== vaultId || preview.vaultId !== vaultId || !preview.scheduledTasks) {
        throw new Error("Scheduling vault changed during review");
      }
      this.pending = { key, identities: structuredClone(preview.scheduledTasks), request: {
        vaultId, vaultGeneration: preview.vaultGeneration, commandId: preview.commandId,
        reviewRevision: preview.reviewRevision, edit,
      } };
      return await this.send();
    } finally { this.running = false; }
  }

  /** Retry the original immutable request even if tasks or form inputs changed. */
  async retry(): Promise<CalendarCommitReceipt> {
    if (this.running) throw new Error("Project scheduling is already in progress");
    if (this.accepted) {
      if (this.vault() !== this.accepted.vaultId) throw new Error("Return to the original scheduling vault before retrying");
      return structuredClone(this.accepted.receipt);
    }
    this.running = true;
    try { return await this.send(); }
    finally { this.running = false; }
  }

  private async send(): Promise<CalendarCommitReceipt> {
    const pending = this.pending;
    if (!pending || this.vault() !== pending.request.vaultId) throw new Error("Return to the original scheduling vault before retrying");
    try {
      const receipt = await this.commit(structuredClone(pending.request));
      if (!receipt.changed || receipt.editedId !== pending.identities[0]?.eventId
        || JSON.stringify(receipt.scheduledTasks) !== JSON.stringify(pending.identities)) {
        throw new Error("Scheduling receipt does not match its reviewed selection");
      }
      this.accepted = { key: pending.key, receipt: structuredClone(receipt), vaultId: pending.request.vaultId };
      this.pending = null;
      return receipt;
    } catch (error) {
      if (error instanceof CalendarCommitFailure && error.outcome === "rejected") this.pending = null;
      throw error;
    }
  }

  /** Acknowledgment follows cache refresh; accepted results are otherwise retained. */
  acknowledge(commandId: string): void {
    if (this.accepted?.receipt.commandId === commandId) this.accepted = null;
  }
}

let controller: ProjectSchedulingController | undefined;

/** One retained batch prevents unbounded unresolved commands across Project views. */
export function getProjectSchedulingController(): ProjectSchedulingController {
  return controller ??= new ProjectSchedulingController();
}
