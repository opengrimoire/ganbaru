import type {
  AttendeeStatus,
  CalendarEvent,
  EventAttendee,
  EventColor,
  EventOrganizer,
  EventStatus,
  EventTransparency,
  EventVisibility,
  GeoCoordinates,
  RecurrenceConfig,
  RecurringScope,
} from "$lib/calendar/types";
import type { Project } from "$lib/projects/types";
import type { CalendarTimeFormat } from "$lib/stores/preference-options";
import { effectiveProjectDefaultEventName } from "$lib/projects/system-defaults";
import {
  projectDefaultIdleTimeoutMinutes,
  projectDefaultPomodoroConfig,
} from "$lib/projects/default-pomodoro";
import {
  projectAllDayDefaultForSelection,
  projectDefaultEventTitleForSelection,
  projectDurationDefaultForSelection,
} from "$lib/calendar/event-panel-utils";
import {
  buildEventPanelChangesPayload,
  buildEventPanelHeavyInitPayload,
  buildEventPanelPomodoroConfig,
  buildEventPanelSaveData,
  collectEventPanelNotifications,
  type EventPanelPayloadInput,
  type PanelSaveData,
} from "./payloads";
import { hasMeetingState } from "$lib/calendar/meeting-state";
import {
  COUNT_PRESET_RHYTHMS,
  type SequencePomodoroRhythmStep,
} from "$lib/pomodoro/rhythm";
import { EventPanelDateTimeController } from "./date-time-controller.svelte";

type PomodoroPreset = "adaptive" | "creative" | "balanced" | "deep" | "extended" | "custom";

export interface EventPanelSessionProjects {
  projectById(id: string | undefined): Project | undefined;
}

export interface EventPanelSessionOptions {
  projects: EventPanelSessionProjects;
  controlsDisabled: () => boolean;
  lockStartControls: () => boolean;
  timeFormat: () => CalendarTimeFormat;
  mode: () => "create" | "edit";
  preferences: () => {
    idlePauseEnabled: boolean;
    idleThresholdMinutes: number;
  };
  onChange: () => ((data: Partial<CalendarEvent>) => void) | undefined;
}

export interface EventPanelHeavyLoadToken {
  generation: number;
  openedEventId: string;
  projectId: string | undefined;
}

export function eventPanelHeavyLookupId(event: Pick<CalendarEvent, "id" | "recurringParentId">): string {
  return event.recurringParentId ?? event.id;
}

/** Owns all editable EventPanel draft state and deterministic draft transitions. */
export class EventPanelSessionController {
  title = $state("");
  startTime = $state("");
  endTime = $state("");
  startDate = $state("");
  endDate = $state("");
  color = $state<EventColor>();
  projectId = $state<string>();
  environmentId = $state<string>();
  playlistId = $state<string>();
  description = $state("");
  scope = $state<RecurringScope>("this");
  allDay = $state(false);
  location = $state("");
  eventUrl = $state("");
  transparency = $state<EventTransparency>("opaque");
  eventStatus = $state<EventStatus>("confirmed");
  visibility = $state<EventVisibility>("private");
  meetingEnabled = $state(false);
  attendees = $state<EventAttendee[]>([]);
  localParticipationStatus = $state<AttendeeStatus>();
  guestCanModify = $state(false);
  guestCanInviteOthers = $state(true);
  guestCanSeeOtherGuests = $state(true);
  organizer = $state<EventOrganizer>();
  geo = $state<GeoCoordinates>();
  rdate = $state<string[]>();
  pomodoroEnabled = $state(false);
  pomodoroPreset = $state<PomodoroPreset>("adaptive");
  focusDurationMinutes = $state(40);
  shortBreakMinutes = $state(5);
  longBreakMinutes = $state(10);
  longBreakAfterFocusCount = $state(4);
  customRhythmMode = $state<"simple" | "sequence">("simple");
  sequenceSteps = $state<SequencePomodoroRhythmStep[]>([]);
  idleTimeoutEnabled = $state(true);
  idleTimeoutMinutesDraft = $state(3);
  notificationsEnabled = $state(false);
  selectedNotificationMinutes = $state(new Set<number>());
  customNotifications = $state<{ amount: number; unit: number }[]>([]);
  recurrence = $state<RecurrenceConfig>();
  initialized = $state(false);
  fullEvent = $state<CalendarEvent | null>(null);
  savePending = $state(false);
  lastInitKey = "";
  lastFullKey = "";
  lastHeavyAppliedKey = "";
  private heavyLoadGeneration = 0;

  readonly dateTime: EventPanelDateTimeController;

  constructor(private readonly options: EventPanelSessionOptions) {
    this.idleTimeoutMinutesDraft = options.preferences().idleThresholdMinutes;
    this.dateTime = new EventPanelDateTimeController({
      draft: this,
      controlsDisabled: options.controlsDisabled,
      lockStartControls: options.lockStartControls,
      timeFormat: options.timeFormat,
      emitChange: () => this.emitChange(),
    });
  }

  get timedSectionsVisible(): boolean {
    return !this.allDay;
  }

  get idleTimeoutMinutesForPayload(): number | null {
    return this.idleTimeoutEnabled ? this.idleTimeoutMinutesDraft : null;
  }

  beginInitialization(key: string): boolean {
    if (key === this.lastInitKey) return false;
    this.lastInitKey = key;
    this.lastHeavyAppliedKey = "";
    this.savePending = false;
    this.initialized = false;
    return true;
  }

  applyDefaultIdleTimeoutPreference(): void {
    const defaults = this.options.preferences();
    this.idleTimeoutEnabled = defaults.idlePauseEnabled;
    this.idleTimeoutMinutesDraft = defaults.idleThresholdMinutes;
  }

  globalFocusIdleDefaults(): { idlePauseEnabled: boolean; idleThresholdMinutes: number } {
    return this.options.preferences();
  }

  applyPomodoroConfigDraft(
    config: CalendarEvent["pomodoroConfig"],
    fallbackEnabled: boolean,
  ): void {
    this.pomodoroEnabled = !!config || fallbackEnabled;
    if (!config) {
      this.focusDurationMinutes = 40;
      this.shortBreakMinutes = 5;
      this.longBreakMinutes = 10;
      this.longBreakAfterFocusCount = 4;
      this.customRhythmMode = "simple";
      this.sequenceSteps = [{
        focusDurationMinutes: 40,
        breakPhase: "short_break",
        breakDurationMinutes: 5,
      }];
      this.pomodoroPreset = "adaptive";
      this.applyDefaultIdleTimeoutPreference();
      return;
    }

    this.pomodoroPreset = config.rhythmSource === "preset" && config.presetKey
      ? config.presetKey
      : "custom";
    if (config.rhythm.kind === "count") {
      this.focusDurationMinutes = config.rhythm.focusDurationMinutes;
      this.shortBreakMinutes = config.rhythm.shortBreakMinutes;
      this.longBreakMinutes = config.rhythm.longBreakMinutes;
      this.longBreakAfterFocusCount = config.rhythm.longBreakAfterFocusCount;
      this.customRhythmMode = "simple";
      this.sequenceSteps = [{
        focusDurationMinutes: config.rhythm.focusDurationMinutes,
        breakPhase: "short_break",
        breakDurationMinutes: config.rhythm.shortBreakMinutes,
      }];
    } else {
      const firstStep = config.rhythm.steps[0] ?? {
        focusDurationMinutes: COUNT_PRESET_RHYTHMS.adaptive.focusDurationMinutes,
        breakPhase: "short_break" as const,
        breakDurationMinutes: COUNT_PRESET_RHYTHMS.adaptive.shortBreakMinutes,
      };
      this.focusDurationMinutes = firstStep.focusDurationMinutes;
      this.shortBreakMinutes = firstStep.breakPhase === "short_break"
        ? firstStep.breakDurationMinutes
        : COUNT_PRESET_RHYTHMS.adaptive.shortBreakMinutes;
      this.longBreakMinutes = firstStep.breakPhase === "long_break"
        ? firstStep.breakDurationMinutes
        : COUNT_PRESET_RHYTHMS.adaptive.longBreakMinutes;
      this.longBreakAfterFocusCount = config.rhythm.steps.length;
      this.customRhythmMode = "sequence";
      this.sequenceSteps = config.rhythm.steps.map((step) => ({ ...step }));
    }
    this.idleTimeoutEnabled = config.idleTimeoutMinutes !== null;
    this.idleTimeoutMinutesDraft = config.idleTimeoutMinutes
      ?? this.options.preferences().idleThresholdMinutes;
  }

  initializeEdit(event: CalendarEvent): void {
    this.title = event.title;
    this.assignTimes(event.start, event.end);
    this.color = event.color;
    this.projectId = event.projectId;
    this.environmentId = event.environmentId;
    this.playlistId = event.playlistId;
    this.recurrence = event.recurrence ? { ...event.recurrence } : undefined;
    this.allDay = event.allDay ?? false;
    this.dateTime.resetAllDayStash();
    this.location = event.location ?? "";
    this.transparency = event.transparency ?? "opaque";
    this.eventStatus = event.status ?? "confirmed";
    this.rdate = event.rdate;
    this.description = event.description ?? "";
    this.eventUrl = event.url ?? "";
    this.visibility = event.visibility ?? "public";
    this.organizer = event.organizer;
    this.attendees = event.attendees ? [...event.attendees] : [];
    this.localParticipationStatus = event.localParticipationStatus;
    this.guestCanModify = event.guestPermissions?.canModify ?? false;
    this.guestCanInviteOthers = event.guestPermissions?.canInviteOthers ?? true;
    this.guestCanSeeOtherGuests = event.guestPermissions?.canSeeOtherGuests ?? true;
    this.geo = event.geo;
    this.meetingEnabled = hasMeetingState(event);
    this.applyPomodoroConfigDraft(event.pomodoroConfig, false);
    this.applyNotifications(event.notifications ?? []);
  }

  initializeCreate(
    createData: Partial<CalendarEvent>,
    fallbackStart: string,
    fallbackEnd: string,
    initialAllDay: boolean,
  ): void {
    const initialStart = createData.start ?? fallbackStart;
    const initialEnd = createData.end ?? fallbackEnd;
    this.title = createData.title ?? "";
    this.assignTimes(initialStart, initialEnd);
    this.color = createData.color;
    this.projectId = createData.projectId;
    this.environmentId = createData.environmentId;
    this.playlistId = createData.playlistId;
    this.description = createData.description ?? "";
    this.recurrence = createData.recurrence ? { ...createData.recurrence } : undefined;
    this.applyPomodoroConfigDraft(createData.pomodoroConfig, true);
    this.applyNotifications(createData.notifications ?? [0]);
    this.allDay = createData.allDay ?? initialAllDay;
    this.dateTime.resetAllDayStash();
    this.location = createData.location ?? "";
    this.eventUrl = createData.url ?? "";
    this.transparency = createData.transparency ?? "opaque";
    this.eventStatus = createData.status ?? "confirmed";
    this.visibility = createData.visibility ?? "private";
    this.organizer = undefined;
    this.attendees = createData.attendees ? [...createData.attendees] : [];
    this.localParticipationStatus = createData.localParticipationStatus;
    this.guestCanModify = false;
    this.guestCanInviteOthers = true;
    this.guestCanSeeOtherGuests = true;
    this.geo = undefined;
    this.rdate = undefined;
    this.meetingEnabled = false;
  }

  beginHeavyLoad(openedEventId: string): EventPanelHeavyLoadToken {
    this.heavyLoadGeneration += 1;
    this.lastFullKey = openedEventId;
    this.fullEvent = null;
    return {
      generation: this.heavyLoadGeneration,
      openedEventId,
      projectId: this.projectId,
    };
  }

  cancelHeavyLoad(): void {
    this.heavyLoadGeneration += 1;
    this.lastFullKey = "";
    this.fullEvent = null;
  }

  completeHeavyLoad(token: EventPanelHeavyLoadToken, fullEvent: CalendarEvent): boolean {
    if (token.generation !== this.heavyLoadGeneration
      || token.openedEventId !== this.lastFullKey) return false;
    this.fullEvent = fullEvent;
    this.applyHeavyEvent(fullEvent, this.projectId !== token.projectId);
    return true;
  }

  applyHeavyEvent(fullEvent: CalendarEvent, preserveProjectSelection = false): void {
    this.description = fullEvent.description ?? "";
    if (!preserveProjectSelection) {
      this.projectId = fullEvent.projectId;
      this.environmentId = fullEvent.environmentId;
      this.playlistId = fullEvent.playlistId;
    }
    this.eventUrl = fullEvent.url ?? "";
    this.visibility = fullEvent.visibility ?? "public";
    this.organizer = fullEvent.organizer;
    this.attendees = fullEvent.attendees ? [...fullEvent.attendees] : [];
    this.localParticipationStatus = fullEvent.localParticipationStatus;
    this.guestCanModify = fullEvent.guestPermissions?.canModify ?? false;
    this.guestCanInviteOthers = fullEvent.guestPermissions?.canInviteOthers ?? true;
    this.guestCanSeeOtherGuests = fullEvent.guestPermissions?.canSeeOtherGuests ?? true;
    this.geo = fullEvent.geo;
    this.meetingEnabled = hasMeetingState(fullEvent);
  }

  syncExternalTimes(start: string, end: string): void {
    if (this.dateTime.isEditing) return;
    this.assignTimes(start, end);
  }

  handleProjectSelect(nextProjectId: string | undefined): void {
    this.projectId = nextProjectId;
    const selectedProject = this.options.projects.projectById(nextProjectId);
    this.environmentId = selectedProject?.workEnvironmentId;
    this.playlistId = selectedProject?.focusPlaylistId;
    if (!selectedProject) {
      this.emitChange();
      return;
    }
    this.title = projectDefaultEventTitleForSelection({
      currentTitle: this.title,
      defaultEventName: effectiveProjectDefaultEventName(selectedProject),
    });
    this.color = selectedProject.color;
    if (projectAllDayDefaultForSelection({
      mode: this.options.mode(),
      allDay: this.allDay,
      activeEdit: this.options.mode() === "edit" && this.options.lockStartControls(),
      defaultEventTimeMode: selectedProject.defaultEventTimeMode,
      defaultEventDurationMinutes: selectedProject.defaultEventDurationMinutes,
    })) {
      this.dateTime.applyAllDayDefault();
    }
    if (!this.allDay) {
      const duration = projectDurationDefaultForSelection({
        mode: this.options.mode(),
        allDay: this.allDay,
        activeEdit: this.options.mode() === "edit" && this.options.lockStartControls(),
        defaultEventTimeMode: selectedProject.defaultEventTimeMode,
        defaultEventDurationMinutes: selectedProject.defaultEventDurationMinutes,
      });
      if (duration !== null) this.addDuration(duration);
      const idleTimeoutMinutes = projectDefaultIdleTimeoutMinutes(
        selectedProject,
        this.globalFocusIdleDefaults(),
      );
      this.applyPomodoroConfigDraft(
        projectDefaultPomodoroConfig(selectedProject, idleTimeoutMinutes),
        false,
      );
      this.idleTimeoutEnabled = idleTimeoutMinutes !== null;
      this.idleTimeoutMinutesDraft = idleTimeoutMinutes
        ?? this.options.preferences().idleThresholdMinutes;
    }
    this.emitChange();
  }

  currentPayloadInput(): EventPanelPayloadInput {
    return {
      title: this.title,
      startDate: this.startDate,
      startTime: this.startTime,
      endDate: this.endDate,
      endTime: this.endTime,
      color: this.color,
      projectId: this.projectId,
      linkedTaskIds: [],
      environmentId: this.environmentId,
      playlistId: this.playlistId,
      description: this.description,
      recurrence: this.recurrence,
      notifications: collectEventPanelNotifications({
        enabled: this.notificationsEnabled,
        selected: this.selectedNotificationMinutes,
        custom: this.customNotifications,
      }),
      pomodoroConfig: buildEventPanelPomodoroConfig({
        allDay: this.allDay,
        enabled: this.pomodoroEnabled,
        preset: this.pomodoroPreset,
        customRhythmMode: this.customRhythmMode,
        sequenceSteps: this.sequenceSteps,
        focusDurationMinutes: this.focusDurationMinutes,
        shortBreakMinutes: this.shortBreakMinutes,
        longBreakMinutes: this.longBreakMinutes,
        longBreakAfterFocusCount: this.longBreakAfterFocusCount,
        idleTimeoutMinutes: this.idleTimeoutMinutesForPayload,
      }),
      allDay: this.allDay,
      meetingEnabled: this.meetingEnabled,
      location: this.location,
      eventUrl: this.eventUrl,
      transparency: this.transparency,
      eventStatus: this.eventStatus,
      visibility: this.visibility,
      attendees: this.attendees,
      localParticipationStatus: this.localParticipationStatus,
      guestCanModify: this.guestCanModify,
      guestCanInviteOthers: this.guestCanInviteOthers,
      guestCanSeeOtherGuests: this.guestCanSeeOtherGuests,
    };
  }

  changesPayload(): Partial<CalendarEvent> {
    return buildEventPanelChangesPayload(this.currentPayloadInput());
  }

  heavyPayload(): Partial<CalendarEvent> {
    return buildEventPanelHeavyInitPayload(this.currentPayloadInput());
  }

  saveData(): PanelSaveData {
    return buildEventPanelSaveData(this.currentPayloadInput());
  }

  emitChange(): void {
    if (this.startDate && this.startTime && this.endTime
      && this.endDate === this.startDate && this.endTime < this.startTime) {
      this.dateTime.syncEndDateFromTimes();
    }
    this.options.onChange()?.(this.changesPayload());
  }

  private assignTimes(start: string, end: string): void {
    this.startDate = start.split(" ")[0] ?? "";
    this.startTime = start.split(" ")[1] ?? "";
    this.endDate = end.split(" ")[0] ?? "";
    this.endTime = end.split(" ")[1] ?? "";
    this.dateTime.syncTimeDrafts();
  }

  private applyNotifications(notifications: number[]): void {
    this.notificationsEnabled = notifications.length > 0;
    this.selectedNotificationMinutes = new Set<number>();
    this.customNotifications = [];
    const presets = new Set([0, 5, 10, 30, 60, 1440]);
    for (const minutes of notifications) {
      if (presets.has(minutes)) {
        this.selectedNotificationMinutes.add(minutes);
        continue;
      }
      const unit = [10080, 1440, 60, 1].find((candidate) => minutes > 0 && minutes % candidate === 0) ?? 1;
      if (this.customNotifications.length < 2) {
        this.customNotifications = [...this.customNotifications, { amount: minutes / unit, unit }];
      }
    }
  }

  private addDuration(minutes: number): void {
    const [year, month, day] = this.startDate.split("-").map(Number);
    const [hour, minute] = this.startTime.split(":").map(Number);
    if (![year, month, day, hour, minute].every(Number.isInteger) || minutes <= 0) return;
    const next = new Date(year, month - 1, day, hour, minute + minutes);
    this.endDate = `${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(next.getDate()).padStart(2, "0")}`;
    this.endTime = `${String(next.getHours()).padStart(2, "0")}:${String(next.getMinutes()).padStart(2, "0")}`;
    this.dateTime.syncTimeDrafts();
  }
}
