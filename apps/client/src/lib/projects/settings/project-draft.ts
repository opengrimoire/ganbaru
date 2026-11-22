import type { EventColor } from "$lib/calendar/types";
import { DEFAULT_POMODORO_PRESET_KEY, type PomodoroPresetKey } from "$lib/pomodoro/rhythm";
import {
  PROJECT_DEFAULT_CUSTOM_POMODORO,
  projectCustomPomodoroFromDefaults,
  type ProjectDefaultIdleSettingsSource,
  type ProjectDefaultPomodoroMode,
} from "$lib/projects/default-pomodoro";
import {
  PROJECT_MAX_DURATION_MINUTES,
  type ProjectDefaultEventTimeMode,
} from "./duration";
import {
  DEFAULT_PROJECT_ICON,
  type Project,
  type ProjectLifecycleChange,
  type ProjectLifecycleStatus,
  type ProjectUpdate,
} from "$lib/projects/types";
import {
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  type FocusIdleThresholdMinutes,
} from "$lib/stores/preference-options";
import { projectHasLockedSystemIdentity } from "$lib/projects/system-defaults";

/** `group_unavailable` means the chosen group disappeared; no group at all is a valid choice. */
export type ProjectDraftError =
  | "name_required"
  | "group_unavailable"
  | "invalid_duration";

export type ProjectDraftResult<T> =
  | { ok: true; value: T }
  | { ok: false; error: ProjectDraftError };

export interface ProjectDraft {
  /** The chosen group, or `null` to keep the project outside every group. */
  groupId: string | null;
  name: string;
  icon: string;
  status: ProjectLifecycleStatus;
  /** The status the draft was opened with, which a status change expects the project still has. */
  openedStatus: ProjectLifecycleStatus;
  color: EventColor | undefined;
  defaultEventName: string;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: string;
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey: PomodoroPresetKey;
  defaultPomodoroFocusMinutes: number;
  defaultPomodoroShortBreakMinutes: number;
  defaultPomodoroLongBreakMinutes: number;
  defaultPomodoroLongBreakAfterFocusCount: number;
  defaultIdleSettingsSource: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled: boolean;
  defaultIdleThresholdMinutes: FocusIdleThresholdMinutes;
  focusPlaylistId: string;
  breakPlaylistId: string;
}

export interface ProjectDraftUpdateInput {
  project: Project;
  draft: ProjectDraft;
  visibleGroupIds: ReadonlySet<string>;
}

export function emptyProjectDraft(): ProjectDraft {
  return {
    groupId: null,
    name: "",
    icon: DEFAULT_PROJECT_ICON,
    status: "active",
    openedStatus: "active",
    color: undefined,
    defaultEventName: "",
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: "60",
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: DEFAULT_POMODORO_PRESET_KEY,
    defaultPomodoroFocusMinutes: PROJECT_DEFAULT_CUSTOM_POMODORO.focusDurationMinutes,
    defaultPomodoroShortBreakMinutes: PROJECT_DEFAULT_CUSTOM_POMODORO.shortBreakMinutes,
    defaultPomodoroLongBreakMinutes: PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakMinutes,
    defaultPomodoroLongBreakAfterFocusCount: PROJECT_DEFAULT_CUSTOM_POMODORO.longBreakAfterFocusCount,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
    focusPlaylistId: "",
    breakPlaylistId: "",
  };
}

export function projectDraftFromProject(project: Project): ProjectDraft {
  const customPomodoro = projectCustomPomodoroFromDefaults(project);
  return {
    groupId: project.groupId,
    name: project.name,
    icon: project.icon,
    status: project.status,
    openedStatus: project.status,
    color: project.color,
    defaultEventName: project.defaultEventName ?? "",
    defaultEventTimeMode: project.defaultEventTimeMode,
    defaultEventDurationMinutes: String(project.defaultEventDurationMinutes ?? ""),
    defaultPomodoroMode: project.defaultPomodoroMode,
    defaultPomodoroPresetKey: project.defaultPomodoroPresetKey ?? DEFAULT_POMODORO_PRESET_KEY,
    defaultPomodoroFocusMinutes: customPomodoro.focusDurationMinutes,
    defaultPomodoroShortBreakMinutes: customPomodoro.shortBreakMinutes,
    defaultPomodoroLongBreakMinutes: customPomodoro.longBreakMinutes,
    defaultPomodoroLongBreakAfterFocusCount: customPomodoro.longBreakAfterFocusCount,
    defaultIdleSettingsSource: project.defaultIdleSettingsSource,
    defaultIdlePauseEnabled: project.defaultIdlePauseEnabled,
    defaultIdleThresholdMinutes: project.defaultIdleThresholdMinutes,
    focusPlaylistId: project.focusPlaylistId ?? "",
    breakPlaylistId: project.breakPlaylistId ?? "",
  };
}

export function projectDraftDirty(
  project: Project,
  draft: ProjectDraft,
): boolean {
  const identityDirty = !projectHasLockedSystemIdentity(project)
    && (draft.name !== project.name || draft.groupId !== project.groupId);
  return identityDirty
    || draft.icon !== project.icon
    || draft.status !== draft.openedStatus
    || draft.color !== project.color
    || draft.defaultEventName !== (project.defaultEventName ?? "")
    || draft.defaultEventTimeMode !== project.defaultEventTimeMode
    || draft.defaultEventDurationMinutes !== String(project.defaultEventDurationMinutes ?? "")
    || projectSettingsProjectPomodoroDraftDirty(project, draft)
    || projectSettingsProjectIdleDraftDirty(project, draft)
    || draft.focusPlaylistId !== (project.focusPlaylistId ?? "")
    || draft.breakPlaylistId !== (project.breakPlaylistId ?? "");
}

/**
 * The lifecycle move a saved draft asks for, or `null` when its status is the one it was opened
 * with. The move expects the opened status, so a status another window changed meanwhile is
 * refused instead of overwritten.
 */
export function projectLifecycleChangeFromDraft(projectId: string, draft: ProjectDraft): ProjectLifecycleChange | null {
  if (draft.status === draft.openedStatus) return null;
  return { projectId, expected: draft.openedStatus, next: draft.status };
}

export function projectUpdateFromDraft(
  input: ProjectDraftUpdateInput,
): ProjectDraftResult<ProjectUpdate> {
  const identityLocked = projectHasLockedSystemIdentity(input.project);
  const name = identityLocked ? input.project.name : input.draft.name.trim();
  const groupId = identityLocked ? input.project.groupId : input.draft.groupId;
  if (!name) return { ok: false, error: "name_required" };
  if (groupId !== null && !input.visibleGroupIds.has(groupId)) return { ok: false, error: "group_unavailable" };
  const duration = projectSettingsProjectDurationFromDraft(input.draft);
  if (!duration.ok) return duration;
  const defaultPomodoroCustom = projectSettingsProjectPomodoroCustomDraft(input.draft);
  return {
    ok: true,
    value: {
      id: input.project.id,
      groupId,
      name,
      icon: input.draft.icon,
      color: input.draft.color ?? null,
      placement: groupId === input.project.groupId ? undefined : { kind: "end" },
      defaultEventName: normalizeOptionalText(input.draft.defaultEventName),
      defaultEventTimeMode: input.draft.defaultEventTimeMode,
      defaultEventDurationMinutes: duration.value,
      defaultPomodoroMode: input.draft.defaultPomodoroMode,
      defaultPomodoroPresetKey: input.draft.defaultPomodoroMode === "preset"
        ? input.draft.defaultPomodoroPresetKey
        : null,
      defaultPomodoroFocusMinutes: input.draft.defaultPomodoroMode === "custom"
        ? defaultPomodoroCustom.focusDurationMinutes
        : null,
      defaultPomodoroShortBreakMinutes: input.draft.defaultPomodoroMode === "custom"
        ? defaultPomodoroCustom.shortBreakMinutes
        : null,
      defaultPomodoroLongBreakMinutes: input.draft.defaultPomodoroMode === "custom"
        ? defaultPomodoroCustom.longBreakMinutes
        : null,
      defaultPomodoroLongBreakAfterFocusCount: input.draft.defaultPomodoroMode === "custom"
        ? defaultPomodoroCustom.longBreakAfterFocusCount
        : null,
      defaultIdleSettingsSource: input.draft.defaultIdleSettingsSource,
      defaultIdlePauseEnabled: input.draft.defaultIdlePauseEnabled,
      defaultIdleThresholdMinutes: input.draft.defaultIdleThresholdMinutes,
      focusPlaylistId: normalizeOptionalIdentifier(input.draft.focusPlaylistId),
      breakPlaylistId: normalizeOptionalIdentifier(input.draft.breakPlaylistId),
      workEnvironmentId: input.project.workEnvironmentId ?? null,
      blockerRulesetId: input.project.blockerRulesetId ?? null,
    },
  };
}

function projectSettingsProjectPomodoroDraftDirty(
  project: Project,
  draft: ProjectDraft,
): boolean {
  if (draft.defaultPomodoroMode !== project.defaultPomodoroMode) return true;
  if (draft.defaultPomodoroMode === "preset") {
    return draft.defaultPomodoroPresetKey !== (project.defaultPomodoroPresetKey ?? DEFAULT_POMODORO_PRESET_KEY);
  }
  if (draft.defaultPomodoroMode === "custom") {
    const customPomodoro = projectCustomPomodoroFromDefaults(project);
    return draft.defaultPomodoroFocusMinutes !== customPomodoro.focusDurationMinutes
      || draft.defaultPomodoroShortBreakMinutes !== customPomodoro.shortBreakMinutes
      || draft.defaultPomodoroLongBreakMinutes !== customPomodoro.longBreakMinutes
      || draft.defaultPomodoroLongBreakAfterFocusCount !== customPomodoro.longBreakAfterFocusCount;
  }
  return false;
}

function projectSettingsProjectIdleDraftDirty(
  project: Project,
  draft: ProjectDraft,
): boolean {
  return draft.defaultIdleSettingsSource !== project.defaultIdleSettingsSource
    || draft.defaultIdlePauseEnabled !== project.defaultIdlePauseEnabled
    || draft.defaultIdleThresholdMinutes !== project.defaultIdleThresholdMinutes;
}

function projectSettingsProjectPomodoroCustomDraft(draft: ProjectDraft) {
  return {
    focusDurationMinutes: draft.defaultPomodoroFocusMinutes,
    shortBreakMinutes: draft.defaultPomodoroShortBreakMinutes,
    longBreakMinutes: draft.defaultPomodoroLongBreakMinutes,
    longBreakAfterFocusCount: draft.defaultPomodoroLongBreakAfterFocusCount,
  };
}

function projectSettingsProjectDurationFromDraft(
  draft: ProjectDraft,
): ProjectDraftResult<number | null> {
  if (draft.defaultEventTimeMode === "all_day") return { ok: true, value: null };
  if (!draft.defaultEventDurationMinutes.trim()) return { ok: true, value: null };
  const parsed = Number(draft.defaultEventDurationMinutes.trim());
  if (
    !Number.isInteger(parsed)
    || parsed <= 0
    || parsed > PROJECT_MAX_DURATION_MINUTES
  ) {
    return { ok: false, error: "invalid_duration" };
  }
  return { ok: true, value: parsed };
}

function normalizeOptionalIdentifier(value: string): string | null {
  const trimmed = value.trim();
  return trimmed || null;
}

function normalizeOptionalText(value: string): string | null {
  const trimmed = value.trim();
  return trimmed || null;
}
