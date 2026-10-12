import { hasOnlyShortcutModifier, hasShortcutModifier } from "$lib/keyboard-shortcuts";

export type EventPanelKeyboardAction = "confirm-delete" | "save" | "delete" | null;

export function isEventPanelDeleteActionTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && target.closest("[data-event-panel-delete-action]") !== null;
}

export function canRunEventPanelSave(input: {
  parked: boolean;
  controlsDisabled: boolean;
  pomodoroReadOnlyInteractive: boolean;
  savePending: boolean;
}): boolean {
  return !input.parked
    && (!input.controlsDisabled || input.pomodoroReadOnlyInteractive)
    && !input.savePending;
}

export function eventPanelKeyboardAction(
  event: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey">,
  state: { parked: boolean; deleteArmed: boolean },
): EventPanelKeyboardAction {
  if (state.parked) return null;
  if (state.deleteArmed && event.key === "Enter"
    && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
    return "confirm-delete";
  }
  if (event.key === "Enter" && hasShortcutModifier(event)) return "save";
  if ((event.key === "d" || event.key === "D") && hasOnlyShortcutModifier(event)) return "delete";
  return null;
}

export interface EventPanelActionsControllerOptions {
  parked: () => boolean;
  canDelete: () => boolean;
  hasDeleteTarget: () => boolean;
  deleteEndsEvent: () => boolean;
  inlineEndEventConfirm: () => boolean;
  skipInlineDeleteConfirm: () => boolean;
  save: () => void;
  delete: () => void;
  endEvent: () => void;
}

/** Owns EventPanel global shortcuts and two-step destructive action state. */
export class EventPanelActionsController {
  deleteArmed = $state(false);

  constructor(private readonly options: EventPanelActionsControllerOptions) {}

  reset(): void {
    this.deleteArmed = false;
  }

  handleKeydown(event: KeyboardEvent): void {
    const action = eventPanelKeyboardAction(event, {
      parked: this.options.parked(),
      deleteArmed: this.deleteArmed,
    });
    if (!action) return;
    event.preventDefault();
    if (action === "confirm-delete") this.confirmArmedDelete();
    else if (action === "save") this.options.save();
    else this.armOrConfirmDelete();
  }

  confirmArmedDelete(): boolean {
    if (!this.deleteArmed) return false;
    this.deleteArmed = false;
    if (this.options.deleteEndsEvent()) this.options.endEvent();
    else this.options.delete();
    return true;
  }

  armOrConfirmDelete(): void {
    if (!this.options.hasDeleteTarget() || !this.options.canDelete()) return;
    if (this.options.deleteEndsEvent()) {
      if (!this.options.inlineEndEventConfirm()) {
        this.options.endEvent();
        return;
      }
      if (!this.confirmArmedDelete()) this.deleteArmed = true;
      return;
    }
    if (this.options.skipInlineDeleteConfirm()) {
      this.deleteArmed = false;
      this.options.delete();
      return;
    }
    if (!this.confirmArmedDelete()) this.deleteArmed = true;
  }

  disarmOutsideConfirm(clickedConfirm: boolean): void {
    if (this.deleteArmed && !clickedConfirm) this.deleteArmed = false;
  }
}
