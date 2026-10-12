import type {
  CalendarEvent,
  EventColor,
  PomodoroConfig,
  RecurrenceConfig,
  RecurringScope,
} from "$lib/calendar/types";
import { recurrenceConfigsEqual } from "$lib/calendar/rrule";
import { parseCalendarDate } from "$lib/calendar/utils";
import {
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  clampFocusIdleThresholdMinutes,
} from "$lib/stores/preference-options";
import {
  clonePomodoroConfig,
  createPresetPomodoroConfig,
} from "$lib/pomodoro/rhythm";
import {
  hasMeetingState,
  hasNonDefaultGuestPermissions,
} from "$lib/calendar/meeting-state";

export type PanelAnchor = { x: number; y: number; width: number; height: number };

export type EditSessionState =
  | { mode: "closed" }
  | { mode: "create"; sessionKey: number; start: string; end: string; anchor: PanelAnchor }
  | {
      mode: "edit";
      sessionKey: number;
      originalEvent: CalendarEvent;
      instanceEvent: CalendarEvent;
      templateId: string;
      detailsLoaded: boolean;
      anchor: PanelAnchor;
    };

export interface CreatePreview {
  dateStr: string;
  startMinute: number;
  endMinute: number;
  title?: string;
  color?: EventColor;
  recurrence?: RecurrenceConfig;
  allDay?: boolean;
  endDateStr?: string;
}

export interface FocusIdleEventDefaults {
  pauseWhenIdle: boolean;
  thresholdMinutes: number;
}

const STATIC_FOCUS_IDLE_DEFAULTS: FocusIdleEventDefaults = {
  pauseWhenIdle: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  thresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
};

function normalizeFocusIdleDefaults(
  defaults?: Partial<FocusIdleEventDefaults>,
): FocusIdleEventDefaults {
  return {
    pauseWhenIdle: defaults?.pauseWhenIdle ?? STATIC_FOCUS_IDLE_DEFAULTS.pauseWhenIdle,
    thresholdMinutes: clampFocusIdleThresholdMinutes(
      defaults?.thresholdMinutes ?? STATIC_FOCUS_IDLE_DEFAULTS.thresholdMinutes,
    ),
  };
}

/**
 * Deep-equal comparison for a single field of a CalendarEvent patch.
 * Treats missing and explicit undefined as equivalent.
 */
export function fieldEqual(a: unknown, b: unknown): boolean {
  if (a === undefined || b === undefined) return a === b;
  if (a === null || b === null) return a === b;
  if (typeof a !== "object" || typeof b !== "object") return a === b;
  return JSON.stringify(a) === JSON.stringify(b);
}

function normalizeNotifications(notifications?: number[]): number[] | undefined {
  if (!notifications || notifications.length === 0) return undefined;
  return [...new Set(notifications)].sort((a, b) => a - b);
}

function normalizePomodoroConfig(
  config: PomodoroConfig | undefined,
  focusIdleDefaults?: Partial<FocusIdleEventDefaults>,
): PomodoroConfig | undefined {
  if (!config) return undefined;
  const { thresholdMinutes } = normalizeFocusIdleDefaults(focusIdleDefaults);
  return {
    ...clonePomodoroConfig(config),
    idleTimeoutMinutes: config.idleTimeoutMinutes !== null ? thresholdMinutes : null,
  };
}

/**
 * Build the normalized baseline shape used by EventPanel on first paint.
 * Keeping this in the session avoids a parent-state callback during panel
 * mount, which otherwise makes event-panel opens wait for a second Svelte
 * flush before they can paint.
 */
export function buildEditPanelInitialChanges(
  event: CalendarEvent,
  focusIdleDefaults?: Partial<FocusIdleEventDefaults>,
): Partial<CalendarEvent> {
  const meetingEnabled = hasMeetingState(event);
  return {
    title: event.title.trim(),
    start: event.start,
    end: event.end,
    color: event.color,
    linkedTaskIds: event.linkedTaskIds ? [...event.linkedTaskIds] : [],
    description: event.description ?? "",
    recurrence: event.recurrence,
    notifications: normalizeNotifications(event.notifications),
    pomodoroConfig: event.allDay
      ? undefined
      : normalizePomodoroConfig(event.pomodoroConfig, focusIdleDefaults),
    allDay: event.allDay || undefined,
    meetingEnabled: meetingEnabled || undefined,
    location: meetingEnabled && event.location ? event.location : undefined,
    url: meetingEnabled && event.url ? event.url : undefined,
    transparency: event.transparency !== "opaque" ? event.transparency : undefined,
    status: event.status !== "confirmed" ? event.status : undefined,
    visibility: event.visibility !== "public" ? event.visibility : undefined,
    attendees: meetingEnabled && event.attendees && event.attendees.length > 0
      ? [...event.attendees]
      : undefined,
    localParticipationStatus: meetingEnabled ? event.localParticipationStatus : undefined,
    guestPermissions: meetingEnabled && hasNonDefaultGuestPermissions(event.guestPermissions)
      ? event.guestPermissions
      : undefined,
  };
}

export function buildCreatePanelInitialChanges(
  start: string,
  end: string,
  allDay?: boolean,
  focusIdleDefaults?: Partial<FocusIdleEventDefaults>,
  initialChanges?: Partial<CalendarEvent>,
): Partial<CalendarEvent> {
  const { pauseWhenIdle, thresholdMinutes } = normalizeFocusIdleDefaults(focusIdleDefaults);
  const base: Partial<CalendarEvent> = {
    title: "",
    start,
    end,
    color: undefined,
    linkedTaskIds: [],
    description: "",
    recurrence: undefined,
    notifications: [0],
    pomodoroConfig: allDay
      ? undefined
      : createPresetPomodoroConfig("adaptive", pauseWhenIdle ? thresholdMinutes : null),
    allDay: allDay || undefined,
    meetingEnabled: undefined,
    location: undefined,
    url: undefined,
    transparency: undefined,
    status: undefined,
    visibility: "private",
    attendees: undefined,
  };
  return {
    ...base,
    ...initialChanges,
    start: initialChanges?.start ?? start,
    end: initialChanges?.end ?? end,
    allDay: initialChanges?.allDay ?? (allDay || undefined),
  };
}

export function minuteOffsetFromDateStart(dateStr: string, value: string): number {
  const base = parseCalendarDate(`${dateStr} 00:00`).getTime();
  const target = parseCalendarDate(value).getTime();
  const offset = (target - base) / 60000;
  return Number.isFinite(offset) ? Math.round(offset) : 0;
}

/**
 * Returns true if `changes` represents any meaningful deviation from `baseline`.
 * Uses merge semantics: the "final" event is `{ ...baseline, ...changes }`,
 * so a field absent from `changes` takes the baseline value and never
 * counts as dirty. Only keys actually present in `changes` are compared,
 * which makes partial patches (e.g. a drag that emits only `start`/`end`)
 * behave correctly without requiring the panel's initial sync to have
 * already populated the baseline.
 */
export function isDirtyDiff(
  changes: Partial<CalendarEvent>,
  baseline: Partial<CalendarEvent>,
): boolean {
  for (const key of Object.keys(changes)) {
    const a = (changes as Record<string, unknown>)[key];
    const b = (baseline as Record<string, unknown>)[key];
    if (key === "recurrence") {
      if (!recurrenceConfigsEqual(a as RecurrenceConfig | undefined, b as RecurrenceConfig | undefined)) {
        return true;
      }
      continue;
    }
    if (!fieldEqual(a, b)) return true;
  }
  return false;
}

let nextSessionKey = 0;

export function createEditSession(
  getFocusIdleDefaults: () => Partial<FocusIdleEventDefaults> = () => STATIC_FOCUS_IDLE_DEFAULTS,
) {
  let state = $state<EditSessionState>({ mode: "closed" });
  let changes = $state<Partial<CalendarEvent>>({});
  let baseline = $state<Partial<CalendarEvent>>({});
  let scope = $state<RecurringScope>("this");
  let createPreview = $state<CreatePreview | null>(null);

  const dirty = $derived(isDirtyDiff(changes, baseline));

  function applyChanges(data: Partial<CalendarEvent>) {
    changes = { ...changes, ...data };

    // Keep createPreview in sync when in create mode
    if (state.mode === "create" && createPreview) {
      const updated = { ...createPreview };
      if (data.title !== undefined) updated.title = data.title;
      if (data.color !== undefined) updated.color = data.color;
      if (data.recurrence !== undefined) updated.recurrence = data.recurrence;
      if (data.start) {
        const dateStr = data.start.split(" ")[0];
        updated.dateStr = dateStr;
        updated.startMinute = minuteOffsetFromDateStart(dateStr, data.start);
      }
      if (data.end) {
        updated.endMinute = minuteOffsetFromDateStart(updated.dateStr, data.end);
      }
      createPreview = updated;
    }

    // Mirror start and end into create-mode state only when they change, since
    // reassigning state resets the panel anchor effect.
    if (state.mode === "create" && (data.start || data.end)) {
      state = {
        ...state,
        start: data.start ? String(data.start) : state.start,
        end: data.end ? String(data.end) : state.end,
      };
    }
  }

  return {
    get state() { return state; },
    get changes() { return changes; },
    get baseline() { return baseline; },
    get scope() { return scope; },
    get dirty() { return dirty; },
    get createPreview() { return createPreview; },

    openEdit(
      event: CalendarEvent,
      anchor: PanelAnchor,
      instanceEvent?: CalendarEvent,
      detailsLoaded = false,
      initialChanges?: Partial<CalendarEvent>,
    ) {
      const panelInitialChanges = initialChanges
        ?? buildEditPanelInitialChanges(event, getFocusIdleDefaults());
      const templateId = event.recurringParentId ?? event.id;
      state = {
        mode: "edit",
        sessionKey: ++nextSessionKey,
        originalEvent: { ...event },
        instanceEvent: instanceEvent ? { ...instanceEvent } : { ...event },
        templateId,
        detailsLoaded,
        anchor,
      };
      changes = { ...panelInitialChanges };
      baseline = { ...panelInitialChanges };
      scope = "this";
      createPreview = null;
    },

    openCreate(
      start: string,
      end: string,
      anchor: PanelAnchor,
      allDay?: boolean,
      initialChanges?: Partial<CalendarEvent>,
    ) {
      const normalizedStart = initialChanges?.start ?? start;
      const normalizedEnd = initialChanges?.end ?? end;
      state = {
        mode: "create",
        sessionKey: ++nextSessionKey,
        start: normalizedStart,
        end: normalizedEnd,
        anchor,
      };
      scope = "this";

      // Seed the full panel baseline before mount. This keeps create preview
      // data available on the first frame and avoids a parent callback from
      // EventPanel during the opening flush.
      const initial = buildCreatePanelInitialChanges(
        normalizedStart,
        normalizedEnd,
        initialChanges?.allDay ?? allDay,
        getFocusIdleDefaults(),
        initialChanges,
      );
      changes = { ...initial };
      baseline = { ...initial };

      const dateStr = normalizedStart.split(" ")[0];
      const endDateStr = normalizedEnd.split(" ")[0];
      createPreview = {
        dateStr,
        startMinute: minuteOffsetFromDateStart(dateStr, normalizedStart),
        endMinute: minuteOffsetFromDateStart(dateStr, normalizedEnd),
        title: typeof initial.title === "string" ? initial.title : undefined,
        color: initial.color,
        allDay: initial.allDay,
        endDateStr: initial.allDay ? endDateStr : undefined,
      };
    },

    updateChanges(data: Partial<CalendarEvent>) {
      applyChanges(data);
    },

    /**
     * Merge panel-initial field values into both `changes` and `baseline`.
     * Both sides receive the same values, so dirty stays false until the user
     * edits a field, and reverting a field to its baseline value lets the
     * panel close without a discard prompt.
     *
     * The panel calls this when deferred event details arrive. Keys absent
     * from `data`, such as start and end already moved by a drag, keep their
     * current values on both sides.
     */
    setInitialChanges(data: Partial<CalendarEvent>) {
      applyChanges(data);
      baseline = { ...baseline, ...data };
    },

    updateScope(newScope: RecurringScope) {
      scope = newScope;
    },

    close() {
      state = { mode: "closed" };
      changes = {};
      baseline = {};
      scope = "this";
      createPreview = null;
    },
  };
}
