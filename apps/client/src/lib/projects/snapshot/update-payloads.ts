import type {
  Project,
  ProjectChecklistItem,
  ProjectChecklistItemUpdate,
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldOptionUpdate,
  ProjectCustomFieldUpdate,
  ProjectGroup,
  ProjectGroupUpdate,
  ProjectOrderPlacement,
  ProjectTag,
  ProjectTagUpdate,
  ProjectSection,
  ProjectSectionUpdate,
  ProjectTask,
  ProjectTaskUpdate,
  ProjectUpdate,
} from "$lib/projects/types";
import { offeredDateReminderMinutes } from "$lib/utils/date-reminders";

/** Task fields to change, a change reason, and optional moves within the section or status order. */
export type ProjectTaskUpdatePatch = Partial<ProjectTask> & {
  changeReason?: string | null;
  sectionPlacement?: ProjectOrderPlacement;
  statusPlacement?: ProjectOrderPlacement;
};

/** An optional move of a row within its ordered list; omitted, the row keeps its place. */
export interface PlacementPatch {
  placement?: ProjectOrderPlacement;
}

function placementField(placement: ProjectOrderPlacement | undefined): { placement?: ProjectOrderPlacement } {
  return placement ? { placement } : {};
}

/** A task date's reminder: none without the date, and an offset its time still offers. */
function taskDateReminderMinutes(
  date: string | undefined,
  time: string | undefined,
  minutes: number | undefined,
): number | null {
  if (!date || minutes === undefined) return null;
  return offeredDateReminderMinutes(minutes, Boolean(time));
}

export function taskUpdatePayload(task: ProjectTask, patch: ProjectTaskUpdatePatch): ProjectTaskUpdate {
  const merged = { ...task, ...patch };
  return {
    id: merged.id,
    sectionId: merged.sectionId,
    statusId: merged.statusId,
    parentTaskId: merged.parentTaskId ?? null,
    title: merged.title,
    description: merged.description,
    priority: merged.priority,
    taskType: merged.taskType,
    ...(patch.sectionPlacement ? { sectionPlacement: patch.sectionPlacement } : {}),
    ...(patch.statusPlacement ? { statusPlacement: patch.statusPlacement } : {}),
    estimateMinutes: merged.estimateMinutes ?? null,
    dueDate: merged.dueDate ?? null,
    dueTime: merged.dueDate ? merged.dueTime ?? null : null,
    dueReminderMinutes: taskDateReminderMinutes(merged.dueDate, merged.dueTime, merged.dueReminderMinutes),
    startDate: merged.startDate ?? null,
    startTime: merged.startDate ? merged.startTime ?? null : null,
    startReminderMinutes: taskDateReminderMinutes(merged.startDate, merged.startTime, merged.startReminderMinutes),
    targetEndDate: merged.targetEndDate ?? null,
    archivedAt: merged.archivedAt ?? null,
    blockerReason: merged.blockerReason ?? null,
    milestone: merged.milestone,
    changeReason: patch.changeReason ?? null,
    expectedRevision: task.revision ?? null,
  };
}

export function checklistItemUpdatePayload(
  item: ProjectChecklistItem,
  patch: Partial<Pick<ProjectChecklistItem, "title" | "completedAt">> & PlacementPatch,
): ProjectChecklistItemUpdate {
  const merged = { ...item, ...patch };
  return {
    id: merged.id,
    title: merged.title,
    completed: Boolean(merged.completedAt),
    ...placementField(patch.placement),
  };
}

export function tagUpdatePayload(
  tag: ProjectTag,
  patch: Partial<Pick<ProjectTag, "name" | "color">> & PlacementPatch,
): ProjectTagUpdate {
  const merged = { ...tag, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    color: merged.color ?? null,
    ...placementField(patch.placement),
  };
}

export function customFieldUpdatePayload(
  field: ProjectCustomField,
  patch: Partial<Pick<ProjectCustomField, "name" | "icon">> & PlacementPatch,
): ProjectCustomFieldUpdate {
  const merged = { ...field, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    ...placementField(patch.placement),
    icon: merged.icon,
  };
}

export function customFieldOptionUpdatePayload(
  option: ProjectCustomFieldOption,
  patch: Partial<Pick<ProjectCustomFieldOption, "name">> & PlacementPatch,
): ProjectCustomFieldOptionUpdate {
  const merged = { ...option, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    ...placementField(patch.placement),
  };
}

export function groupUpdatePayload(
  group: ProjectGroup,
  patch: Partial<Pick<ProjectGroup, "name" | "icon" | "color" | "collapsed">> & PlacementPatch,
): ProjectGroupUpdate {
  const merged = { ...group, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    icon: merged.icon,
    color: merged.color ?? null,
    ...placementField(patch.placement),
    collapsed: merged.collapsed,
  };
}

export function projectUpdatePayload(project: Project, patch: Partial<Project> & PlacementPatch): ProjectUpdate {
  const merged = { ...project, ...patch };
  return {
    id: merged.id,
    groupId: merged.groupId,
    name: merged.name,
    icon: merged.icon,
    color: merged.color ?? null,
    ...placementField(patch.placement),
    status: merged.status,
    defaultEventName: merged.defaultEventName,
    defaultEventTimeMode: merged.defaultEventTimeMode,
    defaultEventDurationMinutes: merged.defaultEventDurationMinutes,
    defaultPomodoroMode: merged.defaultPomodoroMode,
    defaultPomodoroPresetKey: merged.defaultPomodoroPresetKey ?? null,
    defaultPomodoroFocusMinutes: merged.defaultPomodoroFocusMinutes ?? null,
    defaultPomodoroShortBreakMinutes: merged.defaultPomodoroShortBreakMinutes ?? null,
    defaultPomodoroLongBreakMinutes: merged.defaultPomodoroLongBreakMinutes ?? null,
    defaultPomodoroLongBreakAfterFocusCount: merged.defaultPomodoroLongBreakAfterFocusCount ?? null,
    defaultIdleSettingsSource: merged.defaultIdleSettingsSource,
    defaultIdlePauseEnabled: merged.defaultIdlePauseEnabled,
    defaultIdleThresholdMinutes: merged.defaultIdleThresholdMinutes,
    focusPlaylistId: merged.focusPlaylistId ?? null,
    breakPlaylistId: merged.breakPlaylistId ?? null,
    workEnvironmentId: merged.workEnvironmentId ?? null,
    blockerRulesetId: merged.blockerRulesetId ?? null,
  };
}

export function sectionUpdatePayload(
  section: ProjectSection,
  patch: Partial<Pick<ProjectSection, "name" | "collapsed" | "hiddenAt" | "archivedAt">> & PlacementPatch,
): ProjectSectionUpdate {
  const merged = { ...section, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    ...placementField(patch.placement),
    collapsed: merged.collapsed,
    hiddenAt: merged.hiddenAt ?? null,
    archivedAt: merged.archivedAt ?? null,
  };
}
