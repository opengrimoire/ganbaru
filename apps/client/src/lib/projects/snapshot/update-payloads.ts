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
  ProjectTag,
  ProjectTagUpdate,
  ProjectSection,
  ProjectSectionUpdate,
  ProjectTask,
  ProjectTaskUpdate,
  ProjectUpdate,
} from "$lib/projects/types";

export type ProjectTaskUpdatePatch = Partial<ProjectTask> & { changeReason?: string | null };

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
    sectionSortOrder: merged.sectionSortOrder,
    statusSortOrder: merged.statusSortOrder,
    estimateMinutes: merged.estimateMinutes ?? null,
    dueDate: merged.dueDate ?? null,
    dueTime: merged.dueDate ? merged.dueTime ?? null : null,
    startDate: merged.startDate ?? null,
    startTime: merged.startDate ? merged.startTime ?? null : null,
    targetEndDate: merged.targetEndDate ?? null,
    archivedAt: merged.archivedAt ?? null,
    blockerReason: merged.blockerReason ?? null,
    milestone: merged.milestone,
    changeReason: patch.changeReason ?? null,
  };
}

export function checklistItemUpdatePayload(
  item: ProjectChecklistItem,
  patch: Partial<Pick<ProjectChecklistItem, "title" | "completedAt" | "sortOrder">>,
): ProjectChecklistItemUpdate {
  const merged = { ...item, ...patch };
  return {
    id: merged.id,
    title: merged.title,
    completed: Boolean(merged.completedAt),
    sortOrder: merged.sortOrder,
  };
}

export function tagUpdatePayload(
  tag: ProjectTag,
  patch: Partial<Pick<ProjectTag, "name" | "color" | "sortOrder">>,
): ProjectTagUpdate {
  const merged = { ...tag, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    color: merged.color ?? null,
    sortOrder: merged.sortOrder,
  };
}

export function customFieldUpdatePayload(
  field: ProjectCustomField,
  patch: Partial<Pick<ProjectCustomField, "name" | "sortOrder" | "icon">>,
): ProjectCustomFieldUpdate {
  const merged = { ...field, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    sortOrder: merged.sortOrder,
    icon: merged.icon,
  };
}

export function customFieldOptionUpdatePayload(
  option: ProjectCustomFieldOption,
  patch: Partial<Pick<ProjectCustomFieldOption, "name" | "sortOrder">>,
): ProjectCustomFieldOptionUpdate {
  const merged = { ...option, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    sortOrder: merged.sortOrder,
  };
}

export function groupUpdatePayload(
  group: ProjectGroup,
  patch: Partial<Pick<ProjectGroup, "name" | "icon" | "color" | "sortOrder" | "collapsed">>,
): ProjectGroupUpdate {
  const merged = { ...group, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    icon: merged.icon,
    color: merged.color ?? null,
    sortOrder: merged.sortOrder,
    collapsed: merged.collapsed,
  };
}

export function projectUpdatePayload(project: Project, patch: Partial<Project>): ProjectUpdate {
  const merged = { ...project, ...patch };
  return {
    id: merged.id,
    groupId: merged.groupId,
    name: merged.name,
    icon: merged.icon,
    color: merged.color ?? null,
    sortOrder: merged.sortOrder,
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
  patch: Partial<Pick<ProjectSection, "name" | "sortOrder" | "collapsed" | "hiddenAt" | "archivedAt">>,
): ProjectSectionUpdate {
  const merged = { ...section, ...patch };
  return {
    id: merged.id,
    name: merged.name,
    sortOrder: merged.sortOrder,
    collapsed: merged.collapsed,
    hiddenAt: merged.hiddenAt ?? null,
    archivedAt: merged.archivedAt ?? null,
  };
}
