import type { CalendarEvent, RecurringScope } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import type { PanelSaveData } from "$lib/components/calendar/event-panel/payloads";
import type { CalendarEditIntent } from "$lib/api/calendar-edit";
import type { CalendarViewCommitService } from "./commit-service";
import type { createCalendarViewToastController } from "./toasts.svelte";

type ToastController = ReturnType<typeof createCalendarViewToastController>;

export interface CalendarViewSaveControllerOptions {
  toasts: ToastController;
  commitService: Pick<CalendarViewCommitService, "persist">;
  getSessionState: () => EditSessionState;
  canEnablePomodoro: (state: Extract<EditSessionState, { mode: "edit" }>) => boolean;
  isSelectedEndable: (state: Extract<EditSessionState, { mode: "edit" }>) => boolean;
  wouldSaveStopSession: (data: PanelSaveData, scope?: RecurringScope) => boolean;
  endWouldStopProductivity: (state: Extract<EditSessionState, { mode: "edit" }>) => boolean;
  confirmSaveStop: (action: () => Promise<void>) => void;
  confirmEndStop: (action: () => Promise<void>) => void;
  buildFreeze: (data: PanelSaveData, scope?: RecurringScope) => CalendarEvent[];
  setDisplayState: (state: {
    suppressGlow: boolean;
    suppressPreview: boolean;
    frozenEvents: CalendarEvent[] | null;
  }) => void;
  refreshWindow: () => Promise<void>;
  closeSession: () => void;
  afterRender: () => Promise<void>;
  savePendingLabel: () => string;
  saveSuccessLabel: () => string;
  saveErrorLabel: (error: unknown) => string;
  logError: (
    context: "enable-active-pomodoro" | "panel-save",
    error: unknown,
    data: PanelSaveData,
    scope?: RecurringScope,
  ) => void;
}

/** Confirm user intent, then await one native Calendar and Focus transaction. */
export class CalendarViewSaveController {
  endingActiveEvent = $state(false);
  saving = $state(false);

  constructor(private readonly options: CalendarViewSaveControllerOptions) {}

  /** Report completion explicitly so failed or deferred saves retain the editor draft. */
  async save(data: PanelSaveData, scope?: RecurringScope): Promise<boolean> {
    const state = this.options.getSessionState();
    if (state.mode === "closed" || this.saving) return false;
    const action = state.mode === "edit" && this.options.canEnablePomodoro(state)
      && data.pomodoroConfig ? "enable_focus" : "save";
    const execute = () => this.execute(data, scope, action, state.sessionKey);
    if (this.options.wouldSaveStopSession(data, scope)) {
      this.options.confirmSaveStop(async () => { await execute(); });
      return false;
    }
    return execute();
  }

  /** Confirm an eligible current-event action; Rust chooses and commits its cutoff. */
  async end(data: PanelSaveData, scope?: RecurringScope): Promise<void> {
    const state = this.options.getSessionState();
    if (state.mode !== "edit" || this.saving || !this.options.isSelectedEndable(state)) return;
    const execute = async () => { await this.execute(data, scope, "end_now", state.sessionKey); };
    if (this.options.endWouldStopProductivity(state)) {
      this.options.confirmEndStop(execute);
      return;
    }
    await execute();
  }

  private sameSession(sessionKey: number): boolean {
    const state = this.options.getSessionState();
    return state.mode !== "closed" && state.sessionKey === sessionKey;
  }

  private async execute(
    data: PanelSaveData, scope: RecurringScope | undefined,
    action: NonNullable<CalendarEditIntent["action"]>, sessionKey: number,
  ): Promise<boolean> {
    if (this.saving || !this.sameSession(sessionKey)) return false;
    this.saving = true;
    this.endingActiveEvent = action === "end_now";
    let toastId: string | undefined;
    try {
      this.options.setDisplayState({
        suppressGlow: true, suppressPreview: true,
        frozenEvents: this.options.buildFreeze(data, scope),
      });
      toastId = this.options.toasts.showSavePendingToast(this.options.savePendingLabel());
      const result = await this.options.commitService.persist(data, scope, { action });
      if (!result.saveRefreshedVisibleWindow) await this.options.refreshWindow();
      if (this.sameSession(sessionKey)) this.options.closeSession();
      this.options.toasts.showSaveSuccessToast(toastId, this.options.saveSuccessLabel());
      await this.options.afterRender();
      return true;
    } catch (error) {
      this.options.logError(action === "enable_focus" ? "enable-active-pomodoro" : "panel-save", error, data, scope);
      toastId ??= this.options.toasts.showSavePendingToast(this.options.savePendingLabel());
      this.options.toasts.showSaveErrorToast(toastId, this.options.saveErrorLabel(error));
      return false;
    } finally {
      if (toastId && this.options.toasts.saveToast?.id === toastId
        && this.options.toasts.saveToast.pending) {
        this.options.toasts.dismissSaveToastIfCurrent(toastId);
      }
      this.saving = false;
      this.endingActiveEvent = false;
      this.options.setDisplayState({ suppressGlow: false, suppressPreview: false, frozenEvents: null });
    }
  }
}
