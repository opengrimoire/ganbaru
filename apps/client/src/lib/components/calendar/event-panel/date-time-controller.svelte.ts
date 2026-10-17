import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { selectDateRangeEnd, selectDateRangeStart } from "$lib/calendar/date-range-selection";
import { hasOnlyShortcutModifier, hasShortcutModifier } from "$lib/keyboard-shortcuts";
import { tick } from "svelte";
import type { TimePickerInputNavigation } from "$lib/components/ui/TimePicker.svelte";
import {
  commitTimeDraft,
  displayTimeDraft,
  restoreTimeDraft,
  sanitizeTimeDraftInput,
} from "$lib/calendar/event-panel-utils";
import { isPanelArrowKey } from "./arrow-nav";

export type EventPanelTimeTarget = "start" | "end";

export interface EventPanelDateTimeDraft {
  allDay: boolean;
  startDate: string;
  startTime: string;
  endDate: string;
  endTime: string;
}

export interface EventPanelDateTimeControllerOptions {
  draft: EventPanelDateTimeDraft;
  controlsDisabled: () => boolean;
  lockStartControls: () => boolean;
  timeFormat: () => CalendarTimeFormat;
  emitChange: () => void;
  now?: () => Date;
}

function nextLocalDate(date: string): string {
  const [year, month, day] = date.split("-").map(Number);
  const next = new Date(year, month - 1, day + 1);
  return `${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(next.getDate()).padStart(2, "0")}`;
}

/** Owns the event panel date range, time input, and picker interaction state. */
export class EventPanelDateTimeController {
  startDatePickerOpen = $state(false);
  endDatePickerOpen = $state(false);
  timePickerTarget = $state<EventPanelTimeTarget | null>(null);
  timePickerKeyboardOpen = $state(false);
  startDateButton = $state<HTMLButtonElement>();
  endDateButton = $state<HTMLButtonElement>();
  startTimeInput = $state<HTMLInputElement>();
  endTimeInput = $state<HTMLInputElement>();
  startTimeDraft = $state("");
  endTimeDraft = $state("");
  startTimeDraftEdited = $state(false);
  endTimeDraftEdited = $state(false);
  timeInputEditTarget = $state<EventPanelTimeTarget | null>(null);
  timePickerInputNavigation = $state<TimePickerInputNavigation | null>(null);

  private timePickerInputNavigationSequence = 0;
  private stashedStartTime = "";
  private stashedEndTime = "";

  constructor(private readonly options: EventPanelDateTimeControllerOptions) {}

  get draft(): EventPanelDateTimeDraft {
    return this.options.draft;
  }

  get isEditing(): boolean {
    return this.timeInputEditTarget !== null
      || this.startTimeDraftEdited
      || this.endTimeDraftEdited;
  }

  resetInteraction(): void {
    this.startDatePickerOpen = false;
    this.endDatePickerOpen = false;
    this.timePickerTarget = null;
    this.timePickerKeyboardOpen = false;
    this.timePickerInputNavigation = null;
  }

  resetAllDayStash(): void {
    this.stashedStartTime = "";
    this.stashedEndTime = "";
  }

  async focusDateButton(target: EventPanelTimeTarget): Promise<void> {
    await tick();
    (target === "start" ? this.startDateButton : this.endDateButton)?.focus();
  }

  cancelDatePicker(target: EventPanelTimeTarget, source?: "keyboard" | "pointer"): void {
    if (target === "start") this.startDatePickerOpen = false;
    else this.endDatePickerOpen = false;
    if (source === "keyboard") void this.focusDateButton(target);
  }

  selectDate(target: EventPanelTimeTarget, date: string, source?: "keyboard" | "pointer"): void {
    if (this.options.controlsDisabled() || (target === "start" && this.options.lockStartControls())) return;
    const next = target === "start"
      ? selectDateRangeStart({
          selectedDate: date,
          startDate: this.draft.startDate,
          endDate: this.draft.endDate,
          fillMissingEndDate: true,
        })
      : selectDateRangeEnd({
          selectedDate: date,
          startDate: this.draft.startDate,
          endDate: this.draft.endDate,
          fillMissingStartDate: true,
        });
    this.draft.startDate = next.startDate ?? date;
    this.draft.endDate = next.endDate ?? date;
    this.cancelDatePicker(target, source);
    this.options.emitChange();
  }

  toggleDatePicker(target: EventPanelTimeTarget, source: "keyboard" | "pointer" = "pointer"): void {
    if (this.options.controlsDisabled() || (target === "start" && this.options.lockStartControls())) return;
    this.closeTimePicker();
    if (target === "start") {
      this.endDatePickerOpen = false;
      this.startDatePickerOpen = source === "keyboard" ? true : !this.startDatePickerOpen;
    } else {
      this.startDatePickerOpen = false;
      this.endDatePickerOpen = source === "keyboard" ? true : !this.endDatePickerOpen;
    }
  }

  handleDateButtonKeydown(event: KeyboardEvent, target: EventPanelTimeTarget): void {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key === " ") {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    event.stopPropagation();
    this.toggleDatePicker(target, "keyboard");
  }

  async focusTimeInput(target: EventPanelTimeTarget, moveCaretToEnd = false): Promise<void> {
    await tick();
    const input = target === "start" ? this.startTimeInput : this.endTimeInput;
    input?.focus();
    if (moveCaretToEnd && input) input.setSelectionRange(input.value.length, input.value.length);
  }

  closeTimePicker(source?: "keyboard" | "pointer"): void {
    const target = this.timePickerTarget;
    this.timePickerTarget = null;
    this.timePickerKeyboardOpen = false;
    if (source === "keyboard" && target) void this.focusTimeInput(target);
  }

  openTimePicker(target: EventPanelTimeTarget, source: "keyboard" | "pointer" = "pointer"): void {
    if (this.options.controlsDisabled() || (target === "start" && this.options.lockStartControls())) return;
    this.startDatePickerOpen = false;
    this.endDatePickerOpen = false;
    this.timePickerKeyboardOpen = source === "keyboard";
    this.timePickerInputNavigation = null;
    this.timePickerTarget = target;
  }

  private setTimeDraft(target: EventPanelTimeTarget, value: string): void {
    if (target === "start") this.startTimeDraft = value;
    else this.endTimeDraft = value;
  }

  private setTimeDraftEdited(target: EventPanelTimeTarget, value: boolean): void {
    if (target === "start") this.startTimeDraftEdited = value;
    else this.endTimeDraftEdited = value;
  }

  isTimeDraftEdited(target: EventPanelTimeTarget): boolean {
    return target === "start" ? this.startTimeDraftEdited : this.endTimeDraftEdited;
  }

  isTimeInputEditing(target: EventPanelTimeTarget): boolean {
    return this.timeInputEditTarget === target;
  }

  timeInputDisplayValue(target: EventPanelTimeTarget): string {
    const draft = target === "start" ? this.startTimeDraft : this.endTimeDraft;
    const canonical = target === "start" ? this.draft.startTime : this.draft.endTime;
    if (this.isTimeInputEditing(target) || this.isTimeDraftEdited(target)) return draft;
    return displayCanonicalTime(canonical || draft, this.options.timeFormat());
  }

  timeInputMirrorValue(target: EventPanelTimeTarget): string {
    return this.timeInputDisplayValue(target)
      || (this.options.timeFormat() === "12h" ? "h:mmam" : "HH:MM");
  }

  timePickerWidth(isEnd: boolean): string {
    if (this.options.timeFormat() === "24h") return isEnd ? "124px" : "80px";
    return isEnd ? "148px" : "98px";
  }

  enterTimeInputEditMode(target: EventPanelTimeTarget): void {
    if (this.timeInputEditTarget !== target && !this.isTimeDraftEdited(target)) {
      this.setTimeDraft(target, restoreTimeDraft(
        target === "start" ? this.draft.startTime : this.draft.endTime,
        this.options.timeFormat(),
      ));
    }
    this.timeInputEditTarget = target;
  }

  leaveTimeInputEditMode(target?: EventPanelTimeTarget): void {
    if (!target || this.timeInputEditTarget === target) this.timeInputEditTarget = null;
  }

  handleTimeDraftInput(event: Event & { currentTarget: HTMLInputElement }, target: EventPanelTimeTarget): void {
    if (target === "start" && this.options.lockStartControls()) return;
    const inputType = "inputType" in event && typeof event.inputType === "string" ? event.inputType : "";
    const formatShortCompact = inputType !== "insertText" && inputType !== "deleteContentBackward";
    this.setTimeDraft(target, displayTimeDraft(event.currentTarget.value, formatShortCompact, this.options.timeFormat()));
    this.setTimeDraftEdited(target, true);
    this.enterTimeInputEditMode(target);
    if (this.timePickerTarget === target) this.closeTimePicker();
  }

  handleTimeBeforeInput(event: InputEvent): void {
    if (event.inputType !== "insertText") return;
    const text = event.data ?? "";
    const allowMeridiem = this.options.timeFormat() === "12h";
    const sanitized = sanitizeTimeDraftInput(text, allowMeridiem);
    if (allowMeridiem ? sanitized.length > 0 : sanitized === text) return;
    event.preventDefault();
  }

  syncTimeDrafts(): void {
    this.startTimeDraft = this.draft.startTime;
    this.endTimeDraft = this.draft.endTime;
    this.startTimeDraftEdited = false;
    this.endTimeDraftEdited = false;
    this.leaveTimeInputEditMode();
  }

  commitTimeInput(target: EventPanelTimeTarget): boolean {
    if (target === "start" && this.options.lockStartControls()) {
      this.restoreTimeInput("start");
      return false;
    }
    const previous = target === "start" ? this.draft.startTime : this.draft.endTime;
    const draft = target === "start" ? this.startTimeDraft : this.endTimeDraft;
    const result = commitTimeDraft(draft, previous, this.options.timeFormat());
    this.setTimeDraft(target, result.value);
    this.setTimeDraftEdited(target, false);
    this.leaveTimeInputEditMode(target);
    if (!result.committed) return false;
    if (target === "start") this.draft.startTime = result.value;
    else this.draft.endTime = result.value;
    this.syncEndDateFromTimes();
    this.options.emitChange();
    return true;
  }

  get hasSaveableTimeDraft(): boolean {
    return this.endTimeDraftEdited || (!this.options.lockStartControls() && this.startTimeDraftEdited);
  }

  commitSaveableTimeDrafts(): boolean {
    let committed = false;
    if (!this.options.lockStartControls() && this.startTimeDraftEdited) {
      committed = this.commitTimeInput("start") || committed;
    }
    if (this.endTimeDraftEdited) committed = this.commitTimeInput("end") || committed;
    return committed;
  }

  restoreTimeInput(target: EventPanelTimeTarget): void {
    this.setTimeDraft(target, restoreTimeDraft(
      target === "start" ? this.draft.startTime : this.draft.endTime,
      this.options.timeFormat(),
    ));
    this.setTimeDraftEdited(target, false);
    this.leaveTimeInputEditMode(target);
  }

  selectTime(time: string, source?: "keyboard" | "pointer"): void {
    const target = this.timePickerTarget;
    if (target === "start" && this.options.lockStartControls()) {
      this.closeTimePicker(source);
      return;
    }
    if (target === "start") this.draft.startTime = time;
    else if (target === "end") this.draft.endTime = time;
    if (target) {
      this.setTimeDraft(target, time);
      this.setTimeDraftEdited(target, false);
      this.leaveTimeInputEditMode(target);
      this.syncEndDateFromTimes();
    }
    this.closeTimePicker(source);
    this.options.emitChange();
  }

  beginTimeTypingFromPicker(digit: string): void {
    const target = this.timePickerTarget;
    if (!target || this.options.controlsDisabled() || this.draft.allDay
      || (target === "start" && this.options.lockStartControls())) return;
    this.closeTimePicker();
    this.setTimeDraft(target, displayTimeDraft(digit, false, this.options.timeFormat()));
    this.setTimeDraftEdited(target, true);
    this.enterTimeInputEditMode(target);
    void this.focusTimeInput(target, true);
  }

  handleTimeInputClick(target: EventPanelTimeTarget): void {
    if (this.options.controlsDisabled() || (target === "start" && this.options.lockStartControls())) return;
    const input = target === "start" ? this.startTimeInput : this.endTimeInput;
    const selectAll = this.timeInputEditTarget !== target || document.activeElement !== input;
    this.openTimePicker(target, "pointer");
    this.enterTimeInputEditMode(target);
    if (selectAll) input?.select();
  }

  private beginTimeTypingFromNavigation(target: EventPanelTimeTarget, text: string): void {
    if (target === "start" && this.options.lockStartControls()) return;
    const draft = displayTimeDraft(text, false, this.options.timeFormat());
    if (!draft) return;
    if (this.timePickerTarget === target) this.closeTimePicker();
    this.setTimeDraft(target, draft);
    this.setTimeDraftEdited(target, true);
    this.enterTimeInputEditMode(target);
    void this.focusTimeInput(target, true);
  }

  private moveOpenPicker(target: EventPanelTimeTarget, key: "ArrowUp" | "ArrowDown"): boolean {
    if (target === "start" && this.options.lockStartControls()) return false;
    if (this.timePickerTarget !== target) return false;
    this.timePickerKeyboardOpen = true;
    this.timePickerInputNavigationSequence += 1;
    this.timePickerInputNavigation = { key, sequence: this.timePickerInputNavigationSequence };
    return true;
  }

  handleTimeInputKeydown(event: KeyboardEvent, target: EventPanelTimeTarget): void {
    if (event.key === "Enter" && hasShortcutModifier(event)) return;
    if ((event.key === "d" || event.key === "D") && hasOnlyShortcutModifier(event)) return;
    if (!event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        const commitOnly = this.isTimeDraftEdited(target);
        this.commitTimeInput(target);
        if (commitOnly) {
          if (this.timePickerTarget === target) this.closeTimePicker("keyboard");
          this.leaveTimeInputEditMode(target);
        } else {
          this.openTimePicker(target, "keyboard");
        }
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        this.restoreTimeInput(target);
        if (this.timePickerTarget) this.closeTimePicker("keyboard");
        return;
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        if (!this.isTimeInputEditing(target)) return;
        if (this.timePickerTarget === target) this.closeTimePicker();
        event.stopPropagation();
        return;
      }
      if ((event.key === "ArrowUp" || event.key === "ArrowDown") && this.moveOpenPicker(target, event.key)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (isPanelArrowKey(event.key)) return;
      if (/^[\d:]$/.test(event.key) && !this.isTimeInputEditing(target)) {
        event.preventDefault();
        event.stopPropagation();
        this.beginTimeTypingFromNavigation(target, event.key);
        return;
      }
    }
    event.stopPropagation();
  }

  syncEndDateFromTimes(): void {
    if (!this.draft.startDate) return;
    this.draft.endDate = this.draft.endTime < this.draft.startTime
      ? nextLocalDate(this.draft.startDate)
      : this.draft.startDate;
  }

  applyAllDayDefault(): void {
    this.stashedStartTime = this.draft.startTime;
    this.stashedEndTime = this.draft.endTime;
    this.draft.allDay = true;
    this.draft.endDate = this.draft.startDate;
    this.draft.startTime = "00:00";
    this.draft.endTime = "00:00";
    this.syncTimeDrafts();
  }

  toggleAllDay(): void {
    if (this.options.controlsDisabled() || this.options.lockStartControls()) return;
    this.closeTimePicker();
    this.draft.allDay = !this.draft.allDay;
    if (this.draft.allDay) {
      this.stashedStartTime = this.draft.startTime;
      this.stashedEndTime = this.draft.endTime;
      this.draft.startTime = "00:00";
      this.draft.endTime = "00:00";
      this.syncTimeDrafts();
    } else if (this.stashedStartTime && this.stashedStartTime !== "00:00") {
      this.draft.startTime = this.stashedStartTime;
      this.draft.endTime = this.stashedEndTime;
      this.resetAllDayStash();
      this.syncEndDateFromTimes();
      this.syncTimeDrafts();
    } else {
      const now = new Date((this.options.now ?? (() => new Date()))());
      now.setMinutes(Math.ceil(now.getMinutes() / 15) * 15, 0, 0);
      this.draft.startTime = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
      const end = new Date(now.getTime() + 3_600_000);
      this.draft.endTime = `${String(end.getHours()).padStart(2, "0")}:${String(end.getMinutes()).padStart(2, "0")}`;
      this.resetAllDayStash();
      this.syncEndDateFromTimes();
      this.syncTimeDrafts();
    }
    this.options.emitChange();
  }
}

function displayCanonicalTime(value: string, timeFormat: CalendarTimeFormat): string {
  if (!value) return "";
  const [hourText, minuteText] = value.split(":");
  const hour = Number(hourText);
  if (timeFormat === "24h" || !Number.isFinite(hour)) return value;
  const period = hour < 12 ? "am" : "pm";
  const displayHour = hour % 12 || 12;
  return `${displayHour}:${minuteText}${period}`;
}
