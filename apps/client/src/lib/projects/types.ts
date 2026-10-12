import type { EventColor } from "$lib/calendar/types";
import type {
  ProjectDefaultIdleSettingsSource,
  ProjectDefaultPomodoroMode,
} from "./default-pomodoro";
import type { PomodoroPresetKey } from "$lib/pomodoro/rhythm";
import {
  DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  type FocusIdleThresholdMinutes,
} from "$lib/stores/preference-options";
import type { MusicContextAssignmentDraft } from "$lib/music/context-assignment";
import type { ProjectDefaultEventTimeMode } from "$lib/projects/settings/duration";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import type { NotesHistoryRetentionDays } from "$lib/notes/history/retention";

/** One step toward the start (-1) or the end (1) of a manual order. */
export type MoveDirection = -1 | 1;

export const PROJECT_VIEW_IDS = ["dashboard", "list", "kanban", "calendar", "gantt"] as const;
export type ProjectViewId = (typeof PROJECT_VIEW_IDS)[number];

export type ProjectPriority = string;
export const PROJECT_TAG_DEFAULT_COLOR: EventColor = 13;

export type ProjectTaskStatusFilter = "all" | "open" | "blocked" | "done";
export type ProjectTaskDueFilter = "all" | "overdue" | "today" | "week" | "none" | "range";
export type ProjectTaskScheduleFilter = "all" | "scheduled" | "unscheduled";
export type ProjectTaskDependencyFilter = "all" | "linked" | "blocked_by" | "blocking" | "none";
export type ProjectTaskTagFilter = string | "all" | "none";
export type ProjectCustomFieldReference = `custom:${string}`;
export const PROJECT_TASK_SORT_MODES = [
  "manual",
  "title",
  "status",
  "section",
  "priority",
  "due",
  "start",
  "scheduled",
  "created",
  "updated",
  "estimate",
] as const;
export type ProjectCoreTaskSortMode = (typeof PROJECT_TASK_SORT_MODES)[number];
export type ProjectTaskSortMode = ProjectCoreTaskSortMode | ProjectCustomFieldReference;
export type ProjectTaskSortDirection = "asc" | "desc";
export const PROJECT_TASK_LIST_COLUMNS = [
  "status",
  "start",
  "due",
  "priority",
  "assignee",
  "reviewer",
  "estimate",
  "scheduled",
  "dependencies",
] as const;
export type ProjectCoreTaskListColumn = (typeof PROJECT_TASK_LIST_COLUMNS)[number];
export type ProjectTaskListColumn = ProjectCoreTaskListColumn | ProjectCustomFieldReference;
export const PROJECT_TASK_GROUP_MODES = ["section", "status", "priority", "due", "scheduled"] as const;
export type ProjectTaskGroupMode = (typeof PROJECT_TASK_GROUP_MODES)[number];
export type ProjectCustomFieldFilter =
  | { fieldId: string; mode: "filled" | "empty" }
  | { fieldId: string; mode: "checkbox"; checked: boolean }
  | { fieldId: string; mode: "option"; optionId: string };

export const PROJECT_TASK_TYPES = ["task", "milestone", "bug", "habit"] as const;
export type ProjectTaskType = (typeof PROJECT_TASK_TYPES)[number];
export const PROJECT_CUSTOM_FIELD_TYPES = [
  "text",
  "number",
  "select",
  "multi_select",
  "status",
  "date",
  "person",
  "files",
  "checkbox",
  "url",
  "phone",
  "email",
] as const;
export type ProjectCustomFieldType = (typeof PROJECT_CUSTOM_FIELD_TYPES)[number];

export type ProjectStatusCategory = "not_started" | "active" | "blocked" | "done";
export const PROJECT_LIFECYCLE_STATUSES = ["active", "hidden", "archived"] as const;
export type ProjectLifecycleStatus = (typeof PROJECT_LIFECYCLE_STATUSES)[number];
export const PROJECT_TEMPLATE_IDS = ["blank", "software", "course", "routine", "reading", "chores"] as const;
export type ProjectTemplateId = (typeof PROJECT_TEMPLATE_IDS)[number];

export interface ProjectChatWorkingFolderOption {
  id: string;
  displayName: string;
}

export interface ProjectChatIntegration {
  listWorkingFolders: (projectId: string) => readonly ProjectChatWorkingFolderOption[];
  ensureLoaded: () => Promise<void>;
  openProject: (projectId: string, workingFolderId?: string) => Promise<void>;
}

export interface ProjectGroup {
  id: string;
  name: string;
  icon: string;
  color?: EventColor;
  sortOrder: number;
  collapsed: boolean;
  hiddenAt?: string;
  archivedAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface Project {
  id: string;
  groupId: string;
  name: string;
  icon: string;
  color?: EventColor;
  sortOrder: number;
  status: ProjectLifecycleStatus;
  defaultEventName: string | null;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: number | null;
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey?: PomodoroPresetKey;
  defaultPomodoroFocusMinutes?: number;
  defaultPomodoroShortBreakMinutes?: number;
  defaultPomodoroLongBreakMinutes?: number;
  defaultPomodoroLongBreakAfterFocusCount?: number;
  defaultIdleSettingsSource: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled: boolean;
  defaultIdleThresholdMinutes: FocusIdleThresholdMinutes;
  focusPlaylistId?: string;
  breakPlaylistId?: string;
  workEnvironmentId?: string;
  blockerRulesetId?: string;
  notesDefaultOpenMode?: NotesPageOpenMode;
  notesHistoryRetentionDays?: NotesHistoryRetentionDays;
  createdAt: string;
  updatedAt: string;
}

export const PROJECT_TEMPLATE_DEFAULTS = {
  blank: {
    icon: "lucide:folder",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
  software: {
    icon: "lucide:folder",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
  course: {
    icon: "lucide:graduation-cap",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
  routine: {
    icon: "lucide:repeat",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
  reading: {
    icon: "lucide:book-open",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
  chores: {
    icon: "lucide:shopping-cart",
    color: null,
    defaultEventName: null,
    defaultEventTimeMode: "timed",
    defaultEventDurationMinutes: null,
    defaultPomodoroMode: "preset",
    defaultPomodoroPresetKey: "adaptive",
    defaultPomodoroFocusMinutes: null,
    defaultPomodoroShortBreakMinutes: null,
    defaultPomodoroLongBreakMinutes: null,
    defaultPomodoroLongBreakAfterFocusCount: null,
    defaultIdleSettingsSource: "global",
    defaultIdlePauseEnabled: DEFAULT_FOCUS_IDLE_PAUSE_ON_EVENT_CREATE,
    defaultIdleThresholdMinutes: DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  },
} satisfies Record<ProjectTemplateId, {
  icon: string;
  color: EventColor | null;
  defaultEventName: string | null;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: number | null;
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey: PomodoroPresetKey | null;
  defaultPomodoroFocusMinutes: number | null;
  defaultPomodoroShortBreakMinutes: number | null;
  defaultPomodoroLongBreakMinutes: number | null;
  defaultPomodoroLongBreakAfterFocusCount: number | null;
  defaultIdleSettingsSource: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled: boolean;
  defaultIdleThresholdMinutes: FocusIdleThresholdMinutes;
}>;

export interface ProjectSection {
  id: string;
  projectId: string;
  name: string;
  sortOrder: number;
  collapsed: boolean;
  hiddenAt?: string;
  archivedAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectStatus {
  id: string;
  projectId: string;
  name: string;
  category: ProjectStatusCategory;
  color: EventColor;
  sortOrder: number;
  terminal: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectPriorityConfig {
  id: string;
  projectId: string;
  name: string;
  color: EventColor;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectTask {
  /** Native monotonic row version. Local drafts can exist before persistence. */
  revision?: number;
  id: string;
  projectId: string;
  sectionId: string;
  statusId: string;
  parentTaskId?: string;
  title: string;
  description: string;
  priority: ProjectPriority;
  taskType: ProjectTaskType;
  sectionSortOrder: number;
  statusSortOrder: number;
  estimateMinutes?: number;
  dueDate?: string;
  dueTime?: string;
  dueReminderMinutes?: number;
  startDate?: string;
  startTime?: string;
  startReminderMinutes?: number;
  targetEndDate?: string;
  completedAt?: string;
  archivedAt?: string;
  blockerReason?: string;
  milestone: boolean;
  createdAt: string;
  updatedAt: string;
  detailLoaded?: boolean;
  summaryBlocked?: boolean;
}

export interface ProjectTaskViewRequest {
  projectId: string;
  view: ProjectViewId;
  pageSize: number;
  cursor?: string;
  columnCursors: Record<string, string>;
  showArchived: boolean;
  visibleSectionIds: string[];
  search: string;
  statusFilter: ProjectTaskStatusFilter;
  sectionFilter: string;
  priorityFilter: string;
  dueFilter: ProjectTaskDueFilter;
  dueRangeStart: string;
  dueRangeEnd: string;
  today: string;
  weekEnd: string;
  scheduleFilter: ProjectTaskScheduleFilter;
  dependencyFilter: ProjectTaskDependencyFilter;
  tagFilter: ProjectTaskTagFilter;
  customFieldFilters: ProjectCustomFieldFilter[];
  sortMode: ProjectTaskSortMode;
  sortDirection: ProjectTaskSortDirection;
  candidateEventIds: string[];
}

export interface ProjectTaskColumnCount {
  statusId: string;
  count: number;
  nextCursor?: string;
}

export interface ProjectDashboardTaskAggregates {
  total: number;
  completed: number;
  openEstimateMinutes: number;
  blocked: number;
  overdue: number;
  unscheduledDue: number;
  missingEstimate: number;
  statusCounts: Record<string, number>;
}

export interface ProjectTaskViewPage {
  projectId: string;
  view: ProjectViewId;
  tasks: ProjectTask[];
  totalCount: number;
  matchedCount: number;
  archivedCount: number;
  nextCursor?: string;
  columnCounts: ProjectTaskColumnCount[];
  aggregates?: ProjectDashboardTaskAggregates;
  columnCalculations?: ProjectTaskColumnCalculations[];
  matchedEventIds: string[];
  taskTagLinks: ProjectTaskTagLink[];
  customFieldValues: ProjectCustomFieldValue[];
  customFieldOptionValues: ProjectCustomFieldOptionValue[];
  dependencies: ProjectTaskDependency[];
  eventLinks: ProjectTaskEventLink[];
  tags: ProjectTag[];
  customFields: ProjectCustomField[];
  customFieldOptions: ProjectCustomFieldOption[];
}

/** Reductions over the complete filtered task set, independent of visible pages. */
export interface ProjectTaskColumnCalculations {
  column: "name" | ProjectTaskListColumn;
  total: number;
  filled: number;
  sum?: number;
  average?: number;
  minimum?: number;
  maximum?: number;
}

export interface ProjectTaskDetailData {
  task: ProjectTask;
  relatedTasks: ProjectTask[];
  checklistItems: ProjectChecklistItem[];
  tags: ProjectTag[];
  taskTagLinks: ProjectTaskTagLink[];
  customFields: ProjectCustomField[];
  customFieldOptions: ProjectCustomFieldOption[];
  customFieldValues: ProjectCustomFieldValue[];
  customFieldOptionValues: ProjectCustomFieldOptionValue[];
  dependencies: ProjectTaskDependency[];
  eventLinks: ProjectTaskEventLink[];
  taskChangeEvents: ProjectTaskChangeEvent[];
}

export interface ProjectChecklistItem {
  id: string;
  taskId: string;
  title: string;
  completedAt?: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectTag {
  id: string;
  projectId: string;
  name: string;
  color?: EventColor;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectTaskTagLink {
  taskId: string;
  tagId: string;
  createdAt: string;
}

export interface ProjectCustomField {
  /** Canonical revision is available after native persistence. */
  revision?: number;
  id: string;
  projectId: string;
  name: string;
  fieldType: ProjectCustomFieldType;
  sortOrder: number;
  /** Chosen `lucide:` icon value; null shows the field type icon. */
  icon: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectCustomFieldOption {
  /** Canonical revision is available after native persistence. */
  revision?: number;
  id: string;
  fieldId: string;
  name: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectCustomFieldValue {
  taskId: string;
  fieldId: string;
  textValue?: string;
  numberValue?: number;
  dateValue?: string;
  checkboxValue?: boolean;
  updatedAt: string;
}

export interface ProjectCustomFieldOptionValue {
  taskId: string;
  fieldId: string;
  optionId: string;
  createdAt: string;
}

export interface ProjectTaskDependency {
  id: string;
  blockingTaskId: string;
  blockedTaskId: string;
  dependencyType: "blocks";
  createdAt: string;
}

export interface ProjectTaskEventLink {
  taskId: string;
  eventId: string;
  linkKind: "scheduled" | "reference";
  createdAt: string;
}

export type ProjectTaskChangeEventType =
  | "created"
  | "updated"
  | "status_changed"
  | "scheduled"
  | "completed"
  | "reopened"
  | "archived"
  | "event_unlinked"
  | "dependency_added"
  | "dependency_removed";

export interface ProjectTaskChangeEvent {
  id: string;
  taskId: string;
  eventType: ProjectTaskChangeEventType;
  fieldName?: string;
  oldValue?: string;
  newValue?: string;
  reason?: string;
  occurredAt: string;
}

export interface ProjectLinkableEventTask {
  taskId: string;
  title: string;
  archivedAt?: string;
}

export interface ProjectLinkableEvent {
  id: string;
  projectId: string;
  title: string;
  start: string;
  end: string;
  timezone: string;
  calendarId: string;
  color?: EventColor;
  allDay: boolean;
  status: string;
  linkedTasks: ProjectLinkableEventTask[];
}

export interface ProjectViewPreference {
  projectId: string;
  viewId: ProjectViewId;
  preferenceKey: string;
  preferenceValue: string;
  updatedAt: string;
}

export interface ProjectViewPreferenceUpsert {
  projectId: string;
  viewId: ProjectViewId;
  preferenceKey: string;
  preferenceValue: string;
}

export interface ProjectCustomEmoji {
  id: string;
  name: string;
  assetPath: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

export interface ProjectCustomEmojiCreate {
  id: string;
  name: string;
  assetPath: string;
  sortOrder: number;
}

export interface ProjectSavedTaskView {
  id: string;
  projectId: string;
  name: string;
  viewId: ProjectViewId;
  search: string;
  statusFilter: ProjectTaskStatusFilter;
  sectionFilter: string | "all";
  priorityFilter: ProjectPriority | "all";
  dueFilter: ProjectTaskDueFilter;
  dueRangeStart: string;
  dueRangeEnd: string;
  scheduleFilter: ProjectTaskScheduleFilter;
  dependencyFilter: ProjectTaskDependencyFilter;
  tagFilter: ProjectTaskTagFilter;
  customFieldFilters: ProjectCustomFieldFilter[];
  sortMode: ProjectTaskSortMode;
  sortDirection: ProjectTaskSortDirection;
  groupBy: ProjectTaskGroupMode;
  collapsedSectionIds: string[];
  showArchivedTasks: boolean;
  visibleColumns: ProjectTaskListColumn[];
  updatedAt: string;
}

export interface ProjectTaskEventLinkCreate {
  taskId: string;
  eventId: string;
  linkKind: ProjectTaskEventLink["linkKind"];
}

export interface ProjectTaskDependencyCreate {
  id: string;
  blockingTaskId: string;
  blockedTaskId: string;
  dependencyType: ProjectTaskDependency["dependencyType"];
}

export interface ProjectChecklistItemCreate {
  id: string;
  taskId: string;
  title: string;
  sortOrder: number;
}

export interface ProjectChecklistItemUpdate {
  id: string;
  title: string;
  completed: boolean;
  sortOrder: number;
}

export interface ProjectTagCreate {
  id: string;
  projectId: string;
  name: string;
  color: EventColor | null;
  sortOrder: number;
}

export interface ProjectTagUpdate {
  id: string;
  name: string;
  color: EventColor | null;
  sortOrder: number;
}

export interface ProjectTaskTagLinkCreate {
  taskId: string;
  tagId: string;
}

export interface ProjectCustomFieldCreate {
  id: string;
  projectId: string;
  name: string;
  fieldType: ProjectCustomFieldType;
  sortOrder: number;
  icon?: string | null;
  duplicateSourceId?: string;
}

/** Full replacement of a field's editable definition; a null icon resets it to the type icon. */
export interface ProjectCustomFieldUpdate {
  id: string;
  name: string;
  sortOrder: number;
  icon: string | null;
}

export interface ProjectCustomFieldOptionCreate {
  id: string;
  fieldId: string;
  name: string;
  sortOrder: number;
}

export interface ProjectCustomFieldOptionUpdate {
  id: string;
  name: string;
  sortOrder: number;
}

export interface ProjectCustomFieldValueUpdate {
  taskId: string;
  fieldId: string;
  textValue: string | null;
  numberValue: number | null;
  dateValue: string | null;
  checkboxValue: boolean | null;
  optionIds: string[];
}

export interface ProjectsSnapshot {
  groups: ProjectGroup[];
  projects: Project[];
  sections: ProjectSection[];
  statuses: ProjectStatus[];
  priorities: ProjectPriorityConfig[];
  tasks: ProjectTask[];
  checklistItems: ProjectChecklistItem[];
  tags: ProjectTag[];
  taskTagLinks: ProjectTaskTagLink[];
  customFields: ProjectCustomField[];
  customFieldOptions: ProjectCustomFieldOption[];
  customFieldValues: ProjectCustomFieldValue[];
  customFieldOptionValues: ProjectCustomFieldOptionValue[];
  dependencies: ProjectTaskDependency[];
  eventLinks: ProjectTaskEventLink[];
  taskChangeEvents: ProjectTaskChangeEvent[];
  viewPreferences: ProjectViewPreference[];
  customEmojis: ProjectCustomEmoji[];
}

export type ProjectMutationRemoval =
  | { kind: "group"; id: string }
  | { kind: "project"; id: string }
  | { kind: "status"; id: string }
  | { kind: "priority"; id: string }
  | { kind: "checklist_item"; id: string }
  | { kind: "tag"; id: string }
  | { kind: "task_tag_link"; taskId: string; tagId: string }
  | { kind: "custom_field"; id: string }
  | { kind: "custom_field_option"; id: string }
  | { kind: "custom_field_value"; taskId: string; fieldId: string }
  | { kind: "dependency"; id: string }
  | { kind: "event_link"; taskId: string; eventId: string }
  | {
      kind: "view_preference";
      projectId: string;
      viewId: string;
      preferenceKey: string;
    }
  | { kind: "custom_emoji"; id: string };

export interface ProjectMutation {
  changed: ProjectsSnapshot;
  removals: ProjectMutationRemoval[];
  calendarEventProjectAssignments: Array<{ eventId: string; projectId: string }>;
}

export interface ProjectsWorkspaceSnapshot {
  resolvedProjectId: string | null;
  activeView: ProjectViewId;
  snapshot: ProjectsSnapshot;
}

export type ProjectOptionalDataKind =
  | "relationships"
  | "custom_fields"
  | "history"
  | "checklist"
  | "saved_views"
  | "custom_emojis";

export interface ProjectsOptionalData {
  kind: ProjectOptionalDataKind;
  projectId: string | null;
  checklistItems: ProjectChecklistItem[];
  tags: ProjectTag[];
  taskTagLinks: ProjectTaskTagLink[];
  customFields: ProjectCustomField[];
  customFieldOptions: ProjectCustomFieldOption[];
  customFieldValues: ProjectCustomFieldValue[];
  customFieldOptionValues: ProjectCustomFieldOptionValue[];
  dependencies: ProjectTaskDependency[];
  eventLinks: ProjectTaskEventLink[];
  taskChangeEvents: ProjectTaskChangeEvent[];
  viewPreferences: ProjectViewPreference[];
  customEmojis: ProjectCustomEmoji[];
}

export interface ProjectGroupCreate {
  id: string;
  name: string;
  icon: string;
  color: EventColor | null;
  sortOrder: number;
}

export interface ProjectGroupUpdate {
  id: string;
  name: string;
  icon: string;
  color: EventColor | null;
  sortOrder: number;
  collapsed: boolean;
}

export interface ProjectCreate {
  id: string;
  groupId: string;
  templateId: ProjectTemplateId;
  name: string;
  icon: string;
  color: EventColor | null;
  sortOrder: number;
  defaultEventName: string | null;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: number | null;
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey: PomodoroPresetKey | null;
  defaultPomodoroFocusMinutes: number | null;
  defaultPomodoroShortBreakMinutes: number | null;
  defaultPomodoroLongBreakMinutes: number | null;
  defaultPomodoroLongBreakAfterFocusCount: number | null;
  defaultIdleSettingsSource: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled: boolean;
  defaultIdleThresholdMinutes: FocusIdleThresholdMinutes;
}

export interface ProjectUpdate {
  id: string;
  groupId: string;
  name: string;
  icon: string;
  color: EventColor | null;
  sortOrder: number;
  status: ProjectLifecycleStatus;
  defaultEventName: string | null;
  defaultEventTimeMode: ProjectDefaultEventTimeMode;
  defaultEventDurationMinutes: number | null;
  defaultPomodoroMode: ProjectDefaultPomodoroMode;
  defaultPomodoroPresetKey: PomodoroPresetKey | null;
  defaultPomodoroFocusMinutes: number | null;
  defaultPomodoroShortBreakMinutes: number | null;
  defaultPomodoroLongBreakMinutes: number | null;
  defaultPomodoroLongBreakAfterFocusCount: number | null;
  defaultIdleSettingsSource: ProjectDefaultIdleSettingsSource;
  defaultIdlePauseEnabled: boolean;
  defaultIdleThresholdMinutes: FocusIdleThresholdMinutes;
  focusPlaylistId: string | null;
  breakPlaylistId: string | null;
  workEnvironmentId: string | null;
  blockerRulesetId: string | null;
  musicAssignments?: MusicContextAssignmentDraft[];
  musicAssignmentsUpdatedAt?: number;
}

export interface ProjectSectionCreate {
  id: string;
  projectId: string;
  name: string;
  sortOrder: number;
}

export interface ProjectSectionUpdate {
  id: string;
  name: string;
  sortOrder: number;
  collapsed: boolean;
  hiddenAt: string | null;
  archivedAt: string | null;
}

export interface ProjectStatusCreate {
  id: string;
  projectId: string;
  name: string;
  category: ProjectStatusCategory;
  color: EventColor;
  sortOrder: number;
  terminal: boolean;
}

export interface ProjectStatusUpdate {
  id: string;
  name: string;
  category: ProjectStatusCategory;
  color: EventColor;
  sortOrder: number;
  terminal: boolean;
}

export interface ProjectPriorityCreate {
  id: string;
  projectId: string;
  name: string;
  color: EventColor;
  sortOrder: number;
}

export interface ProjectPriorityUpdate {
  id: string;
  projectId: string;
  name: string;
  color: EventColor;
  sortOrder: number;
}

export interface ProjectTaskCreate {
  id: string;
  projectId: string;
  sectionId: string;
  statusId: string;
  parentTaskId: string | null;
  title: string;
}

export interface ProjectTaskUpdate {
  id: string;
  sectionId: string;
  statusId: string;
  parentTaskId: string | null;
  title: string;
  description: string;
  priority: ProjectPriority;
  taskType: ProjectTaskType;
  sectionSortOrder: number;
  statusSortOrder: number;
  estimateMinutes: number | null;
  dueDate: string | null;
  dueTime: string | null;
  dueReminderMinutes: number | null;
  startDate: string | null;
  startTime: string | null;
  startReminderMinutes: number | null;
  targetEndDate: string | null;
  archivedAt: string | null;
  blockerReason: string | null;
  milestone: boolean;
  changeReason: string | null;
}
