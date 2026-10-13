import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import { normalizeEventColor } from "$lib/calendar/utils";
import { FALLBACK_COLOR_INDEX } from "$lib/calendar/types";
import { localTimezone } from "$lib/stores/calendar/event-payloads";
import { toCalendarDate } from "$lib/calendar/db-rows";
import {
  isNotesPageOpenMode,
  type NotesPageOpenMode,
} from "$lib/notes/pages/open-mode";
import {
  isNotesHistoryRetentionDays,
  type NotesHistoryRetentionDays,
} from "$lib/notes/history/retention";
import {
  DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES,
  FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS,
  type FocusIdleThresholdMinutes,
} from "$lib/stores/preference-options";
import type {
  Project,
  ProjectChecklistItem,
  ProjectChecklistItemCreate,
  ProjectChecklistItemUpdate,
  ProjectCreate,
  ProjectCustomEmoji,
  ProjectCustomEmojiCreate,
  ProjectCustomField,
  ProjectCustomFieldCreate,
  ProjectCustomFieldOption,
  ProjectCustomFieldOptionCreate,
  ProjectCustomFieldOptionUpdate,
  ProjectCustomFieldOptionValue,
  ProjectCustomFieldUpdate,
  ProjectCustomFieldValue,
  ProjectCustomFieldValueUpdate,
  ProjectGroup,
  ProjectGroupCreate,
  ProjectGroupUpdate,
  ProjectTag,
  ProjectTagCreate,
  ProjectTagUpdate,
  ProjectLinkableEvent,
  ProjectMutation,
  ProjectMutationRemoval,
  ProjectOptionalDataKind,
  ProjectPriorityConfig,
  ProjectPriorityCreate,
  ProjectPriorityUpdate,
  ProjectSection,
  ProjectSectionCreate,
  ProjectSectionUpdate,
  ProjectStatus,
  ProjectStatusCreate,
  ProjectStatusUpdate,
  ProjectUpdate,
  ProjectsSnapshot,
  ProjectsOptionalData,
  ProjectsWorkspaceSnapshot,
  ProjectTask,
  ProjectTaskChangeEvent,
  ProjectTaskCreate,
  ProjectTaskDependency,
  ProjectTaskDependencyCreate,
  ProjectTaskEventLink,
  ProjectTaskEventLinkCreate,
  ProjectTaskTagLink,
  ProjectTaskTagLinkCreate,
  ProjectTaskUpdate,
  ProjectTaskViewRequest,
  ProjectTaskViewPage,
  ProjectTaskListColumn,
  ProjectTaskDetailData,
  ProjectViewPreference,
  ProjectViewPreferenceUpsert,
  ProjectViewId,
} from "$lib/projects/types";
import { translate } from "$lib/i18n/translator.svelte";
import { PROJECT_CUSTOM_FIELD_TYPES } from "$lib/projects/types";
import {
  systemProjectGroupName,
  systemProjectName,
} from "$lib/projects/system-defaults";

interface ProjectGroupRow {
  id: string;
  name: string;
  icon: string;
  color: number | null;
  sort_order: number;
  collapsed: number;
  hidden_at: string | null;
  archived_at: string | null;
  created_at: string;
  updated_at: string;
}

interface ProjectRow {
  id: string;
  group_id: string;
  name: string;
  icon: string;
  color: number | null;
  sort_order: number;
  status: Project["status"];
  default_event_name: string | null;
  default_event_time_mode: Project["defaultEventTimeMode"];
  default_event_duration_minutes: number | null;
  default_pomodoro_mode: Project["defaultPomodoroMode"];
  default_pomodoro_preset_key: NonNullable<Project["defaultPomodoroPresetKey"]> | null;
  default_pomodoro_focus_minutes: number | null;
  default_pomodoro_short_break_minutes: number | null;
  default_pomodoro_long_break_minutes: number | null;
  default_pomodoro_long_break_after_focus_count: number | null;
  default_idle_settings_source: string;
  default_idle_pause_enabled: number;
  default_idle_threshold_minutes: number;
  focus_playlist_id: string | null;
  break_playlist_id: string | null;
  work_environment_id: string | null;
  blocker_ruleset_id: string | null;
  notes_default_open_mode: NotesPageOpenMode | null;
  notes_history_retention_days: number | null;
  created_at: string;
  updated_at: string;
}

interface ProjectSectionRow {
  id: string;
  project_id: string;
  name: string;
  sort_order: number;
  collapsed: number;
  hidden_at: string | null;
  archived_at: string | null;
  created_at: string;
  updated_at: string;
}

interface ProjectStatusRow {
  id: string;
  project_id: string;
  name: string;
  category: ProjectStatus["category"];
  color: number;
  sort_order: number;
  terminal: number;
  created_at: string;
  updated_at: string;
}

interface ProjectPriorityRow {
  id: string;
  project_id: string;
  name: string;
  color: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

interface ProjectTaskRow {
  revision: number;
  id: string;
  project_id: string;
  section_id: string;
  status_id: string;
  parent_task_id: string | null;
  title: string;
  description: string;
  priority: ProjectTask["priority"];
  task_type: ProjectTask["taskType"];
  section_sort_order: number;
  status_sort_order: number;
  estimate_minutes: number | null;
  due_date: string | null;
  due_time: string | null;
  due_reminder_minutes: number | null;
  start_date: string | null;
  start_time: string | null;
  start_reminder_minutes: number | null;
  target_end_date: string | null;
  completed_at: string | null;
  archived_at: string | null;
  blocker_reason: string | null;
  milestone: number;
  created_at: string;
  updated_at: string;
}

interface ProjectTaskSummaryRow
  extends Omit<ProjectTaskRow, "description" | "blocker_reason" | "due_reminder_minutes" | "start_reminder_minutes"> {
  blocker_reason_present: number;
}

interface ProjectChecklistItemRow {
  id: string;
  task_id: string;
  title: string;
  completed_at: string | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

interface ProjectTagRow {
  id: string;
  project_id: string;
  name: string;
  color: number | null;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

interface ProjectTaskTagLinkRow {
  task_id: string;
  tag_id: string;
  created_at: string;
}

interface ProjectCustomFieldRow {
  revision: number;
  id: string;
  project_id: string;
  name: string;
  field_type: ProjectCustomField["fieldType"];
  sort_order: number;
  icon: string | null;
  created_at: string;
  updated_at: string;
}

interface ProjectCustomFieldOptionRow {
  revision: number;
  id: string;
  field_id: string;
  name: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

interface ProjectCustomFieldValueRow {
  task_id: string;
  field_id: string;
  text_value: string | null;
  number_value: number | null;
  date_value: string | null;
  checkbox_value: number | null;
  updated_at: string;
}

interface ProjectCustomFieldOptionValueRow {
  task_id: string;
  field_id: string;
  option_id: string;
  created_at: string;
}

interface ProjectTaskDependencyRow {
  id: string;
  blocking_task_id: string;
  blocked_task_id: string;
  dependency_type: ProjectTaskDependency["dependencyType"];
  created_at: string;
}

interface ProjectTaskEventLinkRow {
  task_id: string;
  event_id: string;
  link_kind: ProjectTaskEventLink["linkKind"];
  created_at: string;
}

interface ProjectTaskChangeEventRow {
  id: string;
  task_id: string;
  event_type: ProjectTaskChangeEvent["eventType"];
  field_name: string | null;
  old_value: string | null;
  new_value: string | null;
  reason: string | null;
  occurred_at: string;
}

interface ProjectViewPreferenceRow {
  project_id: string;
  view_id: ProjectViewPreference["viewId"];
  preference_key: string;
  preference_value: string;
  updated_at: string;
}

interface ProjectCustomEmojiRow {
  id: string;
  name: string;
  asset_path: string;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

interface ProjectLinkableEventTaskRow {
  task_id: string;
  title: string;
  archived_at: string | null;
}

interface ProjectLinkableEventRow {
  id: string;
  project_id: string;
  title: string;
  start_time: string;
  end_time: string;
  timezone: string;
  calendar_id: string;
  color: number | null;
  all_day: number;
  status: string;
  linked_tasks: ProjectLinkableEventTaskRow[];
}

interface ProjectsSnapshotRows {
  groups: ProjectGroupRow[];
  projects: ProjectRow[];
  sections: ProjectSectionRow[];
  statuses: ProjectStatusRow[];
  priorities: ProjectPriorityRow[];
  tasks: ProjectTaskRow[];
  checklist_items: ProjectChecklistItemRow[];
  tags: ProjectTagRow[];
  task_tag_links: ProjectTaskTagLinkRow[];
  custom_fields: ProjectCustomFieldRow[];
  custom_field_options: ProjectCustomFieldOptionRow[];
  custom_field_values: ProjectCustomFieldValueRow[];
  custom_field_option_values: ProjectCustomFieldOptionValueRow[];
  dependencies: ProjectTaskDependencyRow[];
  event_links: ProjectTaskEventLinkRow[];
  task_change_events: ProjectTaskChangeEventRow[];
  view_preferences: ProjectViewPreferenceRow[];
  custom_emojis: ProjectCustomEmojiRow[];
}

interface ProjectsWorkspaceSnapshotRows {
  resolved_project_id: string | null;
  active_view: ProjectViewId;
  snapshot: ProjectsSnapshotRows;
}

interface ProjectTaskViewPageRows {
  project_id: string;
  view: ProjectViewId;
  tasks: ProjectTaskSummaryRow[];
  total_count: number;
  matched_count: number;
  archived_count: number;
  column_calculations?: Array<{ column: "name" | ProjectTaskListColumn; total: number; filled: number; sum: number | null; average: number | null; minimum: number | null; maximum: number | null }>;
  next_cursor: string | null;
  column_counts: Array<{ status_id: string; count: number; next_cursor: string | null }>;
  aggregates: null | {
    total: number;
    completed: number;
    open_estimate_minutes: number;
    blocked: number;
    overdue: number;
    unscheduled_due: number;
    missing_estimate: number;
    status_counts: Record<string, number>;
  };
  matched_event_ids: string[];
  task_tag_links: ProjectTaskTagLinkRow[];
  custom_field_values: ProjectCustomFieldValueRow[];
  custom_field_option_values: ProjectCustomFieldOptionValueRow[];
  dependencies: ProjectTaskDependencyRow[];
  event_links: ProjectTaskEventLinkRow[];
  tags: ProjectTagRow[];
  custom_fields: ProjectCustomFieldRow[];
  custom_field_options: ProjectCustomFieldOptionRow[];
}

interface ProjectTaskDetailDataRows {
  task: ProjectTaskRow;
  related_tasks: ProjectTaskSummaryRow[];
  checklist_items: ProjectChecklistItemRow[];
  tags: ProjectTagRow[];
  task_tag_links: ProjectTaskTagLinkRow[];
  custom_fields: ProjectCustomFieldRow[];
  custom_field_options: ProjectCustomFieldOptionRow[];
  custom_field_values: ProjectCustomFieldValueRow[];
  custom_field_option_values: ProjectCustomFieldOptionValueRow[];
  dependencies: ProjectTaskDependencyRow[];
  event_links: ProjectTaskEventLinkRow[];
  task_change_events: ProjectTaskChangeEventRow[];
}

interface ProjectsOptionalDataRows {
  kind: ProjectOptionalDataKind;
  project_id: string | null;
  checklist_items: ProjectChecklistItemRow[];
  tags: ProjectTagRow[];
  task_tag_links: ProjectTaskTagLinkRow[];
  custom_fields: ProjectCustomFieldRow[];
  custom_field_options: ProjectCustomFieldOptionRow[];
  custom_field_values: ProjectCustomFieldValueRow[];
  custom_field_option_values: ProjectCustomFieldOptionValueRow[];
  dependencies: ProjectTaskDependencyRow[];
  event_links: ProjectTaskEventLinkRow[];
  task_change_events: ProjectTaskChangeEventRow[];
  view_preferences: ProjectViewPreferenceRow[];
  custom_emojis: ProjectCustomEmojiRow[];
}

type ProjectMutationRemovalRow =
  | { kind: "group"; id: string }
  | { kind: "project"; id: string }
  | { kind: "status"; id: string }
  | { kind: "priority"; id: string }
  | { kind: "checklist_item"; id: string }
  | { kind: "tag"; id: string }
  | { kind: "task_tag_link"; task_id: string; tag_id: string }
  | { kind: "custom_field"; id: string }
  | { kind: "custom_field_option"; id: string }
  | { kind: "custom_field_value"; task_id: string; field_id: string }
  | { kind: "dependency"; id: string }
  | { kind: "event_link"; task_id: string; event_id: string }
  | { kind: "custom_emoji"; id: string };

interface ProjectsMutationRows extends ProjectsSnapshotRows {
  removals: ProjectMutationRemovalRow[];
  calendar_event_project_assignments: Array<{ event_id: string; project_id: string }>;
}

function optionalText(value: string | null): string | undefined {
  return value ?? undefined;
}

function optionalNumber(value: number | null): number | undefined {
  return value ?? undefined;
}

function optionalNotesPageOpenMode(value: unknown): NotesPageOpenMode | undefined {
  return isNotesPageOpenMode(value) ? value : undefined;
}

function mapProjectIdleSettingsSource(value: string): Project["defaultIdleSettingsSource"] {
  return value === "custom" ? "custom" : "global";
}

function isFocusIdleThresholdMinutes(value: number): value is FocusIdleThresholdMinutes {
  return FOCUS_IDLE_THRESHOLD_MINUTES_OPTIONS.some((option) => option === value);
}

function mapFocusIdleThresholdMinutes(value: number): FocusIdleThresholdMinutes {
  return isFocusIdleThresholdMinutes(value) ? value : DEFAULT_FOCUS_IDLE_THRESHOLD_MINUTES;
}

function mapGroup(row: ProjectGroupRow): ProjectGroup {
  const storedName = row.name;
  return {
    id: row.id,
    get name() {
      return systemProjectGroupName(row.id, storedName, translate);
    },
    icon: row.icon,
    color: optionalNumber(row.color),
    sortOrder: row.sort_order,
    collapsed: row.collapsed !== 0,
    hiddenAt: optionalText(row.hidden_at),
    archivedAt: optionalText(row.archived_at),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapProject(row: ProjectRow): Project {
  const storedName = row.name;
  return {
    id: row.id,
    groupId: row.group_id,
    get name() {
      return systemProjectName(row.id, storedName, translate);
    },
    icon: row.icon,
    color: optionalNumber(row.color),
    sortOrder: row.sort_order,
    status: row.status,
    defaultEventName: row.default_event_name,
    defaultEventTimeMode: row.default_event_time_mode,
    defaultEventDurationMinutes: row.default_event_duration_minutes,
    defaultPomodoroMode: row.default_pomodoro_mode,
    defaultPomodoroPresetKey: row.default_pomodoro_preset_key ?? undefined,
    defaultPomodoroFocusMinutes: optionalNumber(row.default_pomodoro_focus_minutes),
    defaultPomodoroShortBreakMinutes: optionalNumber(row.default_pomodoro_short_break_minutes),
    defaultPomodoroLongBreakMinutes: optionalNumber(row.default_pomodoro_long_break_minutes),
    defaultPomodoroLongBreakAfterFocusCount: optionalNumber(row.default_pomodoro_long_break_after_focus_count),
    defaultIdleSettingsSource: mapProjectIdleSettingsSource(row.default_idle_settings_source),
    defaultIdlePauseEnabled: row.default_idle_pause_enabled !== 0,
    defaultIdleThresholdMinutes: mapFocusIdleThresholdMinutes(row.default_idle_threshold_minutes),
    focusPlaylistId: optionalText(row.focus_playlist_id),
    breakPlaylistId: optionalText(row.break_playlist_id),
    workEnvironmentId: optionalText(row.work_environment_id),
    blockerRulesetId: optionalText(row.blocker_ruleset_id),
    notesDefaultOpenMode: optionalNotesPageOpenMode(row.notes_default_open_mode),
    notesHistoryRetentionDays: isNotesHistoryRetentionDays(row.notes_history_retention_days)
      ? row.notes_history_retention_days
      : undefined,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapSection(row: ProjectSectionRow): ProjectSection {
  return {
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    sortOrder: row.sort_order,
    collapsed: row.collapsed !== 0,
    hiddenAt: optionalText(row.hidden_at),
    archivedAt: optionalText(row.archived_at),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapStatus(row: ProjectStatusRow): ProjectStatus {
  return {
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    category: row.category,
    color: normalizeEventColor(row.color) ?? FALLBACK_COLOR_INDEX,
    sortOrder: row.sort_order,
    terminal: row.terminal !== 0,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapPriority(row: ProjectPriorityRow): ProjectPriorityConfig {
  return {
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    color: normalizeEventColor(row.color) ?? FALLBACK_COLOR_INDEX,
    sortOrder: row.sort_order,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapTask(row: ProjectTaskRow): ProjectTask {
  return {
    revision: row.revision,
    id: row.id,
    projectId: row.project_id,
    sectionId: row.section_id,
    statusId: row.status_id,
    parentTaskId: optionalText(row.parent_task_id),
    title: row.title,
    description: row.description,
    priority: row.priority,
    taskType: row.task_type,
    sectionSortOrder: row.section_sort_order,
    statusSortOrder: row.status_sort_order,
    estimateMinutes: optionalNumber(row.estimate_minutes),
    dueDate: optionalText(row.due_date),
    dueTime: optionalText(row.due_time),
    dueReminderMinutes: optionalNumber(row.due_reminder_minutes),
    startDate: optionalText(row.start_date),
    startTime: optionalText(row.start_time),
    startReminderMinutes: optionalNumber(row.start_reminder_minutes),
    targetEndDate: optionalText(row.target_end_date),
    completedAt: optionalText(row.completed_at),
    archivedAt: optionalText(row.archived_at),
    blockerReason: optionalText(row.blocker_reason),
    milestone: row.milestone !== 0,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
    detailLoaded: true,
  };
}

function mapTaskSummary(row: ProjectTaskSummaryRow): ProjectTask {
  return {
    ...mapTask({
      ...row,
      description: "",
      blocker_reason: null,
      due_reminder_minutes: null,
      start_reminder_minutes: null,
    }),
    detailLoaded: false,
    summaryBlocked: row.blocker_reason_present !== 0,
  };
}

function mapChecklistItem(row: ProjectChecklistItemRow): ProjectChecklistItem {
  return {
    id: row.id,
    taskId: row.task_id,
    title: row.title,
    completedAt: optionalText(row.completed_at),
    sortOrder: row.sort_order,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapTag(row: ProjectTagRow): ProjectTag {
  return {
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    color: optionalNumber(row.color),
    sortOrder: row.sort_order,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapTaskTagLink(row: ProjectTaskTagLinkRow): ProjectTaskTagLink {
  return {
    taskId: row.task_id,
    tagId: row.tag_id,
    createdAt: row.created_at,
  };
}

function mapCustomField(row: ProjectCustomFieldRow): ProjectCustomField {
  return {
    revision: row.revision,
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    fieldType: row.field_type,
    sortOrder: row.sort_order,
    icon: row.icon,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapCustomFieldOption(row: ProjectCustomFieldOptionRow): ProjectCustomFieldOption {
  return {
    revision: row.revision,
    id: row.id,
    fieldId: row.field_id,
    name: row.name,
    sortOrder: row.sort_order,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapCustomFieldValue(row: ProjectCustomFieldValueRow): ProjectCustomFieldValue {
  return {
    taskId: row.task_id,
    fieldId: row.field_id,
    textValue: optionalText(row.text_value),
    numberValue: optionalNumber(row.number_value),
    dateValue: optionalText(row.date_value),
    checkboxValue: row.checkbox_value === null ? undefined : row.checkbox_value !== 0,
    updatedAt: row.updated_at,
  };
}

function mapCustomFieldOptionValue(row: ProjectCustomFieldOptionValueRow): ProjectCustomFieldOptionValue {
  return {
    taskId: row.task_id,
    fieldId: row.field_id,
    optionId: row.option_id,
    createdAt: row.created_at,
  };
}

function mapDependency(row: ProjectTaskDependencyRow): ProjectTaskDependency {
  return {
    id: row.id,
    blockingTaskId: row.blocking_task_id,
    blockedTaskId: row.blocked_task_id,
    dependencyType: row.dependency_type,
    createdAt: row.created_at,
  };
}

function mapEventLink(row: ProjectTaskEventLinkRow): ProjectTaskEventLink {
  return {
    taskId: row.task_id,
    eventId: row.event_id,
    linkKind: row.link_kind,
    createdAt: row.created_at,
  };
}

function mapTaskChangeEvent(row: ProjectTaskChangeEventRow): ProjectTaskChangeEvent {
  return {
    id: row.id,
    taskId: row.task_id,
    eventType: row.event_type,
    fieldName: optionalText(row.field_name),
    oldValue: optionalText(row.old_value),
    newValue: optionalText(row.new_value),
    reason: optionalText(row.reason),
    occurredAt: row.occurred_at,
  };
}

function mapViewPreference(row: ProjectViewPreferenceRow): ProjectViewPreference {
  return {
    projectId: row.project_id,
    viewId: row.view_id,
    preferenceKey: row.preference_key,
    preferenceValue: row.preference_value,
    updatedAt: row.updated_at,
  };
}

function mapCustomEmoji(row: ProjectCustomEmojiRow): ProjectCustomEmoji {
  return {
    id: row.id,
    name: row.name,
    assetPath: row.asset_path,
    sortOrder: row.sort_order,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function mapLinkableEvent(row: ProjectLinkableEventRow): ProjectLinkableEvent {
  const renderZone = localTimezone();
  const allDay = row.all_day !== 0;
  const color = normalizeEventColor(row.color);
  return {
    id: row.id,
    projectId: row.project_id,
    title: row.title,
    start: toCalendarDate(row.start_time, renderZone, allDay),
    end: toCalendarDate(row.end_time, renderZone, allDay),
    timezone: row.timezone,
    calendarId: row.calendar_id,
    color,
    allDay,
    status: row.status,
    linkedTasks: row.linked_tasks.map((task) => ({
      taskId: task.task_id,
      title: task.title,
      archivedAt: optionalText(task.archived_at),
    })),
  };
}

function mapSnapshot(rows: ProjectsSnapshotRows): ProjectsSnapshot {
  return {
    groups: rows.groups.map(mapGroup),
    projects: rows.projects.map(mapProject),
    sections: rows.sections.map(mapSection),
    statuses: rows.statuses.map(mapStatus),
    priorities: rows.priorities.map(mapPriority),
    tasks: rows.tasks.map(mapTask),
    checklistItems: rows.checklist_items.map(mapChecklistItem),
    tags: rows.tags.map(mapTag),
    taskTagLinks: rows.task_tag_links.map(mapTaskTagLink),
    customFields: rows.custom_fields.map(mapCustomField),
    customFieldOptions: rows.custom_field_options.map(mapCustomFieldOption),
    customFieldValues: rows.custom_field_values.map(mapCustomFieldValue),
    customFieldOptionValues: rows.custom_field_option_values.map(mapCustomFieldOptionValue),
    dependencies: rows.dependencies.map(mapDependency),
    eventLinks: rows.event_links.map(mapEventLink),
    taskChangeEvents: rows.task_change_events.map(mapTaskChangeEvent),
    viewPreferences: rows.view_preferences.map(mapViewPreference),
    customEmojis: rows.custom_emojis.map(mapCustomEmoji),
  };
}

function mapWorkspaceSnapshot(rows: ProjectsWorkspaceSnapshotRows): ProjectsWorkspaceSnapshot {
  return {
    resolvedProjectId: rows.resolved_project_id,
    activeView: rows.active_view,
    snapshot: mapSnapshot(rows.snapshot),
  };
}

function mapTaskViewPage(rows: ProjectTaskViewPageRows): ProjectTaskViewPage {
  return {
    projectId: rows.project_id,
    view: rows.view,
    tasks: rows.tasks.map(mapTaskSummary),
    totalCount: rows.total_count,
    matchedCount: rows.matched_count,
    archivedCount: rows.archived_count,
    columnCalculations: rows.column_calculations?.map((column) => ({
      column: column.column, total: column.total, filled: column.filled,
      sum: column.sum ?? undefined, average: column.average ?? undefined,
      minimum: column.minimum ?? undefined, maximum: column.maximum ?? undefined,
    })),
    nextCursor: optionalText(rows.next_cursor),
    columnCounts: rows.column_counts.map((column) => ({
      statusId: column.status_id,
      count: column.count,
      nextCursor: optionalText(column.next_cursor),
    })),
    aggregates: rows.aggregates ? {
      total: rows.aggregates.total,
      completed: rows.aggregates.completed,
      openEstimateMinutes: rows.aggregates.open_estimate_minutes,
      blocked: rows.aggregates.blocked,
      overdue: rows.aggregates.overdue,
      unscheduledDue: rows.aggregates.unscheduled_due,
      missingEstimate: rows.aggregates.missing_estimate,
      statusCounts: rows.aggregates.status_counts,
    } : undefined,
    matchedEventIds: rows.matched_event_ids,
    taskTagLinks: rows.task_tag_links.map(mapTaskTagLink),
    customFieldValues: rows.custom_field_values.map(mapCustomFieldValue),
    customFieldOptionValues: rows.custom_field_option_values.map(mapCustomFieldOptionValue),
    dependencies: rows.dependencies.map(mapDependency),
    eventLinks: rows.event_links.map(mapEventLink),
    tags: rows.tags.map(mapTag),
    customFields: rows.custom_fields.map(mapCustomField),
    customFieldOptions: rows.custom_field_options.map(mapCustomFieldOption),
  };
}

function mapTaskDetailData(rows: ProjectTaskDetailDataRows): ProjectTaskDetailData {
  return {
    task: mapTask(rows.task),
    relatedTasks: rows.related_tasks.map(mapTaskSummary),
    checklistItems: rows.checklist_items.map(mapChecklistItem),
    tags: rows.tags.map(mapTag),
    taskTagLinks: rows.task_tag_links.map(mapTaskTagLink),
    customFields: rows.custom_fields.map(mapCustomField),
    customFieldOptions: rows.custom_field_options.map(mapCustomFieldOption),
    customFieldValues: rows.custom_field_values.map(mapCustomFieldValue),
    customFieldOptionValues: rows.custom_field_option_values.map(mapCustomFieldOptionValue),
    dependencies: rows.dependencies.map(mapDependency),
    eventLinks: rows.event_links.map(mapEventLink),
    taskChangeEvents: rows.task_change_events.map(mapTaskChangeEvent),
  };
}

function mapOptionalData(rows: ProjectsOptionalDataRows): ProjectsOptionalData {
  return {
    kind: rows.kind,
    projectId: rows.project_id,
    checklistItems: rows.checklist_items.map(mapChecklistItem),
    tags: rows.tags.map(mapTag),
    taskTagLinks: rows.task_tag_links.map(mapTaskTagLink),
    customFields: rows.custom_fields.map(mapCustomField),
    customFieldOptions: rows.custom_field_options.map(mapCustomFieldOption),
    customFieldValues: rows.custom_field_values.map(mapCustomFieldValue),
    customFieldOptionValues: rows.custom_field_option_values.map(mapCustomFieldOptionValue),
    dependencies: rows.dependencies.map(mapDependency),
    eventLinks: rows.event_links.map(mapEventLink),
    taskChangeEvents: rows.task_change_events.map(mapTaskChangeEvent),
    viewPreferences: rows.view_preferences.map(mapViewPreference),
    customEmojis: rows.custom_emojis.map(mapCustomEmoji),
  };
}

function mapMutationRemoval(row: ProjectMutationRemovalRow): ProjectMutationRemoval {
  switch (row.kind) {
    case "task_tag_link":
      return { kind: row.kind, taskId: row.task_id, tagId: row.tag_id };
    case "custom_field_value":
      return { kind: row.kind, taskId: row.task_id, fieldId: row.field_id };
    case "event_link":
      return { kind: row.kind, taskId: row.task_id, eventId: row.event_id };
    default:
      return row;
  }
}

function mapMutation(rows: ProjectsMutationRows): ProjectMutation {
  return {
    changed: mapSnapshot(rows),
    removals: rows.removals.map(mapMutationRemoval),
    calendarEventProjectAssignments: rows.calendar_event_project_assignments.map((assignment) => ({
      eventId: assignment.event_id,
      projectId: assignment.project_id,
    })),
  };
}

export async function loadProjectsWorkspace(
  preferredProjectId: string | null,
  activeView: ProjectViewId,
): Promise<ProjectsWorkspaceSnapshot> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectsWorkspaceSnapshotRows>("projects_load_workspace", {
    dbUrl,
    preferredProjectId,
    activeView,
  });
  return mapWorkspaceSnapshot(rows);
}

export async function refreshProjectsWorkspace(
  preferredProjectId: string | null,
  activeView: ProjectViewId,
): Promise<ProjectsWorkspaceSnapshot> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectsWorkspaceSnapshotRows>("projects_refresh_workspace", {
    dbUrl,
    preferredProjectId,
    activeView,
  });
  return mapWorkspaceSnapshot(rows);
}

export async function loadProjectTaskView(
  request: ProjectTaskViewRequest,
): Promise<ProjectTaskViewPage> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectTaskViewPageRows>("projects_load_task_view", {
    dbUrl,
    request,
  });
  return mapTaskViewPage(rows);
}

export async function loadProjectTaskDetail(taskId: string): Promise<ProjectTaskDetailData> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectTaskDetailDataRows>("projects_load_task_detail", {
    dbUrl,
    taskId,
  });
  return mapTaskDetailData(rows);
}

export async function loadProjectsOptionalData(
  kind: ProjectOptionalDataKind,
  projectId: string | null,
): Promise<ProjectsOptionalData> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectsOptionalDataRows>("projects_load_optional_data", {
    dbUrl,
    projectId,
    kind,
  });
  return mapOptionalData(rows);
}

async function invokeProjectMutation(
  command: string,
  args: Record<string, unknown>,
): Promise<ProjectMutation> {
  return mapMutation(await invoke<ProjectsMutationRows>(command, args));
}

export async function createProjectGroup(group: ProjectGroupCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_group", { dbUrl, group });
}

export async function updateProjectGroup(group: ProjectGroupUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_group", { dbUrl, group });
}

export async function deleteProjectGroup(groupId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_group", { dbUrl, groupId });
}

export async function setProjectGroupCollapsed(
  groupId: string,
  collapsed: boolean,
): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_set_group_collapsed", { dbUrl, groupId, collapsed });
}

export async function createProject(project: ProjectCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_project", { dbUrl, project });
}

export async function updateProject(project: ProjectUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_project", { dbUrl, project });
}

export async function updateProjectNotesSettings(
  projectId: string,
  notesDefaultOpenMode: NotesPageOpenMode | null,
  notesHistoryRetentionDays: NotesHistoryRetentionDays | null,
): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_notes_settings", {
    dbUrl,
    projectId,
    notesDefaultOpenMode,
    notesHistoryRetentionDays,
  });
}

export async function createProjectSection(section: ProjectSectionCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_section", { dbUrl, section });
}

export async function updateProjectSection(section: ProjectSectionUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_section", { dbUrl, section });
}

export async function createProjectStatus(status: ProjectStatusCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_status", { dbUrl, status });
}

export async function updateProjectStatus(status: ProjectStatusUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_status", { dbUrl, status });
}

export async function deleteProjectStatus(statusId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_status", { dbUrl, statusId });
}

export async function createProjectPriority(priority: ProjectPriorityCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_priority", { dbUrl, priority });
}

export async function updateProjectPriority(priority: ProjectPriorityUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_priority", { dbUrl, priority });
}

export async function deleteProjectPriority(projectId: string, priorityId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_priority", { dbUrl, projectId, priorityId });
}

export async function createProjectTask(task: ProjectTaskCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_task", { dbUrl, task });
}

export async function createProjectChecklistItem(item: ProjectChecklistItemCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_checklist_item", { dbUrl, item });
}

export async function updateProjectChecklistItem(item: ProjectChecklistItemUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_checklist_item", { dbUrl, item });
}

export async function deleteProjectChecklistItem(itemId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_checklist_item", { dbUrl, itemId });
}

export async function createProjectTag(tag: ProjectTagCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_tag", { dbUrl, tag });
}

export async function updateProjectTag(tag: ProjectTagUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_tag", { dbUrl, tag });
}

export async function deleteProjectTag(tagId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_tag", { dbUrl, tagId });
}

export async function linkProjectTaskTag(link: ProjectTaskTagLinkCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_link_task_tag", { dbUrl, link });
}

export async function unlinkProjectTaskTag(taskId: string, tagId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_unlink_task_tag", { dbUrl, taskId, tagId });
}

export async function createProjectCustomField(field: ProjectCustomFieldCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_custom_field", { dbUrl, field });
}

export async function updateProjectCustomField(field: ProjectCustomFieldUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_custom_field", { dbUrl, field });
}

export async function deleteProjectCustomField(fieldId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_custom_field", { dbUrl, fieldId });
}

export async function createProjectCustomFieldOption(option: ProjectCustomFieldOptionCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_custom_field_option", { dbUrl, option });
}

export async function updateProjectCustomFieldOption(option: ProjectCustomFieldOptionUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_custom_field_option", { dbUrl, option });
}

export async function deleteProjectCustomFieldOption(optionId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_custom_field_option", { dbUrl, optionId });
}

export async function updateProjectCustomFieldValue(value: ProjectCustomFieldValueUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_custom_field_value", { dbUrl, value });
}

export async function updateProjectTask(task: ProjectTaskUpdate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_update_task", { dbUrl, task });
}

/** A field-specific action whose expected values come from the user's displayed selection. */
export type ProjectTaskBulkChange =
  | { kind: "status"; statusId: string }
  | { kind: "priority"; priority: string }
  | { kind: "archive"; archived: boolean };

export interface ProjectTaskBulkRequest {
  operationId: string;
  projectId: string;
  tasks: { id: string; value: string | null }[];
  change: ProjectTaskBulkChange;
}

/** Apply one bulk task change. Retrying with the same operation ID and payload recovers the committed result after a lost response. */
export async function applyProjectTaskBulk(request: ProjectTaskBulkRequest): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  const response = await invoke<unknown>("projects_apply_task_bulk", { dbUrl, request });
  return mapMutation(parseCompoundProjectRows(response, request.projectId, { kind: "task" }));
}

export type ProjectReorderItem =
  | { kind: "task"; id: string; axis: "section" | "status"; groupId: string; parentTaskId: string | null; expectedOrder: number }
  | { kind: "custom_field"; id: string; expectedOrder: number }
  | { kind: "custom_field_option"; id: string; fieldId: string; expectedOrder: number };

export interface ProjectReorderRequest {
  operationId: string;
  projectId: string;
  direction: -1 | 1;
  item: ProjectReorderItem;
}

/** Move one item among its siblings and return only the validated committed result. */
export async function reorderProjectItem(request: ProjectReorderRequest): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  const response = await invoke<unknown>("projects_reorder_item", { dbUrl, request });
  return mapMutation(parseCompoundProjectRows(response, request.projectId, request.item));
}

/** Decode a bounded native task-only transaction result for its owning project. */
export function parseProjectTaskMutation(value: unknown, projectId: string): ProjectMutation {
  return mapMutation(parseCompoundProjectRows(value, projectId, { kind: "task" }));
}

/** Validate the command boundary before it can replace canonical frontend state. */
function parseCompoundProjectRows(
  value: unknown,
  projectId: string,
  scope: { kind: "task" | "custom_field" } | { kind: "custom_field_option"; fieldId: string },
): ProjectsMutationRows {
  const taskLimit = 10_000;
  const historyPerTaskLimit = 32;
  function readObject(value: unknown): Record<string, unknown> {
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      throw new Error("Invalid project bulk result object");
    }
    return Object.fromEntries(Object.entries(value));
  }
  function readText(row: Record<string, unknown>, key: string): string {
    const value = row[key];
    if (typeof value !== "string") throw new Error(`Invalid project bulk result ${key}`);
    return value;
  }
  function readOptionalText(row: Record<string, unknown>, key: string): string | null {
    return row[key] === null ? null : readText(row, key);
  }
  function readNumber(row: Record<string, unknown>, key: string): number {
    const value = row[key];
    if (typeof value !== "number" || !Number.isFinite(value)) {
      throw new Error(`Invalid project bulk result ${key}`);
    }
    return value;
  }
  function readOptionalNumber(row: Record<string, unknown>, key: string): number | null {
    return row[key] === null ? null : readNumber(row, key);
  }
  function readArray(value: unknown, maximum: number): unknown[] {
    if (!Array.isArray(value) || value.length > maximum) throw new Error("Invalid project bulk result array");
    return value;
  }
  const row = readObject(value);
  const tasks = readArray(row.tasks, scope.kind === "task" ? taskLimit : 0).map((value): ProjectTaskRow => {
    const task = readObject(value);
    const revision = readNumber(task, "revision");
    const taskType = readText(task, "task_type");
    if (!Number.isSafeInteger(revision) || revision < 0
      || ![0, 1].includes(readNumber(task, "milestone"))
      || readText(task, "project_id") !== projectId
      || (taskType !== "task" && taskType !== "milestone" && taskType !== "bug" && taskType !== "habit")) {
      throw new Error("Invalid project bulk task identity, revision, or type");
    }
    return {
      id: readText(task, "id"), project_id: projectId, revision,
      section_id: readText(task, "section_id"), status_id: readText(task, "status_id"),
      parent_task_id: readOptionalText(task, "parent_task_id"), title: readText(task, "title"),
      description: readText(task, "description"), priority: readText(task, "priority"), task_type: taskType,
      section_sort_order: readNumber(task, "section_sort_order"), status_sort_order: readNumber(task, "status_sort_order"),
      estimate_minutes: readOptionalNumber(task, "estimate_minutes"), due_date: readOptionalText(task, "due_date"),
      due_time: readOptionalText(task, "due_time"), due_reminder_minutes: readOptionalNumber(task, "due_reminder_minutes"),
      start_date: readOptionalText(task, "start_date"),
      start_time: readOptionalText(task, "start_time"),
      start_reminder_minutes: readOptionalNumber(task, "start_reminder_minutes"),
      target_end_date: readOptionalText(task, "target_end_date"),
      completed_at: readOptionalText(task, "completed_at"), archived_at: readOptionalText(task, "archived_at"),
      blocker_reason: readOptionalText(task, "blocker_reason"), milestone: readNumber(task, "milestone"),
      created_at: readText(task, "created_at"), updated_at: readText(task, "updated_at"),
    };
  });
  const taskIds = new Set(tasks.map((task) => task.id));
  if (taskIds.size !== tasks.length) throw new Error("Duplicate project bulk result task");
  const taskChangeEvents = readArray(row.task_change_events, taskLimit * historyPerTaskLimit).map((value): ProjectTaskChangeEventRow => {
    const event = readObject(value);
    const eventType = readText(event, "event_type");
    if (eventType !== "created" && eventType !== "updated" && eventType !== "status_changed"
      && eventType !== "scheduled" && eventType !== "completed" && eventType !== "reopened"
      && eventType !== "archived" && eventType !== "event_unlinked" && eventType !== "dependency_added"
      && eventType !== "dependency_removed") throw new Error("Invalid project bulk history event");
    const taskId = readText(event, "task_id");
    if (!taskIds.has(taskId)) throw new Error("Invalid project bulk history owner");
    return {
      id: readText(event, "id"), task_id: taskId, event_type: eventType,
      field_name: readOptionalText(event, "field_name"), old_value: readOptionalText(event, "old_value"),
      new_value: readOptionalText(event, "new_value"), reason: readOptionalText(event, "reason"),
      occurred_at: readText(event, "occurred_at"),
    };
  });
  function readRevision(row: Record<string, unknown>): number {
    const value = readNumber(row, "revision");
    if (!Number.isSafeInteger(value) || value < 0) throw new Error("Invalid project schema revision");
    return value;
  }
  function readSortOrder(row: Record<string, unknown>): number {
    const value = readNumber(row, "sort_order");
    if (!Number.isSafeInteger(value)) throw new Error("Invalid project schema order");
    return value;
  }
  const customFields = readArray(row.custom_fields, scope.kind === "custom_field" ? taskLimit : 0).map((value): ProjectCustomFieldRow => {
    const field = readObject(value);
    const fieldType = PROJECT_CUSTOM_FIELD_TYPES.find((type) => type === field.field_type);
    if (!fieldType || readText(field, "project_id") !== projectId) throw new Error("Invalid reordered custom field owner or type");
    return { id: readText(field, "id"), revision: readRevision(field), project_id: projectId,
      name: readText(field, "name"), field_type: fieldType, sort_order: readSortOrder(field), icon: readOptionalText(field, "icon"),
      created_at: readText(field, "created_at"), updated_at: readText(field, "updated_at") };
  });
  const customFieldOptions = readArray(row.custom_field_options, scope.kind === "custom_field_option" ? taskLimit : 0).map((value): ProjectCustomFieldOptionRow => {
    const option = readObject(value);
    if (scope.kind !== "custom_field_option" || readText(option, "field_id") !== scope.fieldId) {
      throw new Error("Invalid reordered option owner");
    }
    return { id: readText(option, "id"), revision: readRevision(option), field_id: scope.fieldId,
      name: readText(option, "name"), sort_order: readSortOrder(option),
      created_at: readText(option, "created_at"), updated_at: readText(option, "updated_at") };
  });
  for (const rows of [customFields, customFieldOptions]) {
    if (new Set(rows.map((entry) => entry.id)).size !== rows.length) throw new Error("Duplicate reordered schema identity");
  }
  const emptyCollections: Omit<ProjectsMutationRows, "tasks" | "task_change_events" | "custom_fields" | "custom_field_options"> = {
    groups: [], projects: [], sections: [], statuses: [], priorities: [], checklist_items: [],
    tags: [], task_tag_links: [], custom_field_values: [],
    custom_field_option_values: [], dependencies: [], event_links: [], view_preferences: [], custom_emojis: [],
    removals: [], calendar_event_project_assignments: [],
  };
  for (const key of Object.keys(emptyCollections)) readArray(row[key], 0);
  return { ...emptyCollections, tasks, task_change_events: taskChangeEvents, custom_fields: customFields, custom_field_options: customFieldOptions };
}

export async function linkProjectTaskEvent(link: ProjectTaskEventLinkCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_link_task_event", { dbUrl, link });
}

export async function unlinkProjectTaskEvent(taskId: string, eventId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_unlink_task_event", { dbUrl, taskId, eventId });
}

export async function searchProjectLinkableEvents(
  projectId: string,
  taskId: string,
  query: string,
  startDate?: string,
  endDate?: string,
  limit = 12,
): Promise<ProjectLinkableEvent[]> {
  const dbUrl = await ensureDbUrl();
  const rows = await invoke<ProjectLinkableEventRow[]>("projects_search_linkable_events", {
    dbUrl,
    search: {
      projectId,
      taskId,
      query,
      startDate,
      endDate,
      limit,
    },
  });
  return rows.map(mapLinkableEvent);
}

export async function createProjectTaskDependency(
  dependency: ProjectTaskDependencyCreate,
): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_task_dependency", { dbUrl, dependency });
}

export async function deleteProjectTaskDependency(dependencyId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_task_dependency", { dbUrl, dependencyId });
}

export async function upsertProjectViewPreference(
  preference: ProjectViewPreferenceUpsert,
): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_upsert_view_preference", { dbUrl, preference });
}


export async function createProjectCustomEmoji(emoji: ProjectCustomEmojiCreate): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_create_custom_emoji", { dbUrl, emoji });
}

export async function deleteProjectCustomEmoji(emojiId: string): Promise<ProjectMutation> {
  const dbUrl = await ensureDbUrl();
  return invokeProjectMutation("projects_delete_custom_emoji", { dbUrl, emojiId });
}
