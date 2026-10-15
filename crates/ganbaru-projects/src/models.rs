use ganbaru_db::impl_sqlite_from_row;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Default, Serialize)]
pub struct ProjectsMutationRows {
    pub(crate) groups: Vec<ProjectGroupRow>,
    pub(crate) projects: Vec<ProjectRow>,
    pub(crate) sections: Vec<ProjectSectionRow>,
    pub(crate) statuses: Vec<ProjectStatusRow>,
    pub(crate) priorities: Vec<ProjectPriorityRow>,
    pub(crate) tasks: Vec<ProjectTaskRow>,
    pub(crate) checklist_items: Vec<ProjectChecklistItemRow>,
    pub(crate) tags: Vec<ProjectTagRow>,
    pub(crate) task_tag_links: Vec<ProjectTaskTagLinkRow>,
    pub(crate) custom_fields: Vec<ProjectCustomFieldRow>,
    pub(crate) custom_field_options: Vec<ProjectCustomFieldOptionRow>,
    pub(crate) custom_field_values: Vec<ProjectCustomFieldValueRow>,
    pub(crate) custom_field_option_values: Vec<ProjectCustomFieldOptionValueRow>,
    pub(crate) dependencies: Vec<ProjectTaskDependencyRow>,
    pub(crate) event_links: Vec<ProjectTaskEventLinkRow>,
    pub(crate) task_change_events: Vec<ProjectTaskChangeEventRow>,
    pub(crate) view_preferences: Vec<ProjectViewPreferenceRow>,
    pub(crate) views: Vec<ProjectViewRow>,
    pub(crate) custom_emojis: Vec<ProjectCustomEmojiRow>,
    pub(crate) removals: Vec<ProjectMutationRemoval>,
    pub(crate) calendar_event_project_assignments: Vec<CalendarEventProjectAssignment>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectMutationRemoval {
    Group { id: String },
    Project { id: String },
    Status { id: String },
    Priority { id: String },
    ChecklistItem { id: String },
    Tag { id: String },
    TaskTagLink { task_id: String, tag_id: String },
    CustomField { id: String },
    CustomFieldOption { id: String },
    CustomFieldValue { task_id: String, field_id: String },
    Dependency { id: String },
    EventLink { task_id: String, event_id: String },
    CustomEmoji { id: String },
    View { id: String },
}

#[derive(Serialize)]
pub struct CalendarEventProjectAssignment {
    pub(crate) event_id: String,
    pub(crate) project_id: String,
}

#[derive(Serialize)]
pub struct ProjectGroupRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) collapsed: i64,
    pub(crate) hidden_at: Option<String>,
    pub(crate) archived_at: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectGroupRow {
    id,
    name,
    icon,
    color,
    sort_order,
    collapsed,
    hidden_at,
    archived_at,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectRow {
    pub(crate) id: String,
    pub(crate) group_id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) status: String,
    pub(crate) default_event_name: Option<String>,
    pub(crate) default_event_time_mode: String,
    pub(crate) default_event_duration_minutes: Option<i64>,
    pub(crate) default_pomodoro_mode: String,
    pub(crate) default_pomodoro_preset_key: Option<String>,
    pub(crate) default_pomodoro_focus_minutes: Option<i64>,
    pub(crate) default_pomodoro_short_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_after_focus_count: Option<i64>,
    pub(crate) default_idle_settings_source: String,
    pub(crate) default_idle_pause_enabled: i64,
    pub(crate) default_idle_threshold_minutes: i64,
    pub(crate) focus_playlist_id: Option<String>,
    pub(crate) break_playlist_id: Option<String>,
    pub(crate) work_environment_id: Option<String>,
    pub(crate) blocker_ruleset_id: Option<String>,
    pub(crate) notes_default_open_mode: Option<String>,
    pub(crate) notes_history_retention_days: Option<i64>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectRow {
    id,
    group_id,
    name,
    icon,
    color,
    sort_order,
    status,
    default_event_name,
    default_event_time_mode,
    default_event_duration_minutes,
    default_pomodoro_mode,
    default_pomodoro_preset_key,
    default_pomodoro_focus_minutes,
    default_pomodoro_short_break_minutes,
    default_pomodoro_long_break_minutes,
    default_pomodoro_long_break_after_focus_count,
    default_idle_settings_source,
    default_idle_pause_enabled,
    default_idle_threshold_minutes,
    focus_playlist_id,
    break_playlist_id,
    work_environment_id,
    blocker_ruleset_id,
    notes_default_open_mode,
    notes_history_retention_days,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectSectionRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
    pub(crate) collapsed: i64,
    pub(crate) hidden_at: Option<String>,
    pub(crate) archived_at: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectSectionRow {
    id,
    project_id,
    name,
    sort_order,
    collapsed,
    hidden_at,
    archived_at,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectStatusRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
    pub(crate) terminal: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectStatusRow {
    id,
    project_id,
    name,
    category,
    color,
    sort_order,
    terminal,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectPriorityRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectPriorityRow {
    id,
    project_id,
    name,
    color,
    sort_order,
    created_at,
    updated_at,
});

#[derive(Serialize, Deserialize)]
pub struct ProjectTaskRow {
    #[serde(default)]
    pub(crate) revision: i64,
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) section_id: String,
    pub(crate) status_id: String,
    pub(crate) parent_task_id: Option<String>,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) priority: String,
    pub(crate) task_type: String,
    pub(crate) section_sort_order: f64,
    pub(crate) status_sort_order: f64,
    pub(crate) estimate_minutes: Option<i64>,
    pub(crate) due_date: Option<String>,
    pub(crate) due_time: Option<String>,
    pub(crate) due_reminder_minutes: Option<i64>,
    pub(crate) start_date: Option<String>,
    pub(crate) start_time: Option<String>,
    pub(crate) start_reminder_minutes: Option<i64>,
    pub(crate) target_end_date: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) archived_at: Option<String>,
    pub(crate) blocker_reason: Option<String>,
    pub(crate) milestone: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Serialize)]
pub struct ProjectTaskSummaryRow {
    pub(crate) revision: i64,
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) section_id: String,
    pub(crate) status_id: String,
    pub(crate) parent_task_id: Option<String>,
    pub(crate) title: String,
    pub(crate) priority: String,
    pub(crate) task_type: String,
    pub(crate) section_sort_order: f64,
    pub(crate) status_sort_order: f64,
    pub(crate) estimate_minutes: Option<i64>,
    pub(crate) due_date: Option<String>,
    pub(crate) due_time: Option<String>,
    pub(crate) start_date: Option<String>,
    pub(crate) start_time: Option<String>,
    pub(crate) target_end_date: Option<String>,
    pub(crate) completed_at: Option<String>,
    pub(crate) archived_at: Option<String>,
    pub(crate) blocker_reason_present: i64,
    pub(crate) milestone: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectTaskSummaryRow {
    revision,
    id,
    project_id,
    section_id,
    status_id,
    parent_task_id,
    title,
    priority,
    task_type,
    section_sort_order,
    status_sort_order,
    estimate_minutes,
    due_date,
    due_time,
    start_date,
    start_time,
    target_end_date,
    completed_at,
    archived_at,
    blocker_reason_present,
    milestone,
    created_at,
    updated_at,
});
impl_sqlite_from_row!(ProjectTaskRow {
    revision,
    id,
    project_id,
    section_id,
    status_id,
    parent_task_id,
    title,
    description,
    priority,
    task_type,
    section_sort_order,
    status_sort_order,
    estimate_minutes,
    due_date,
    due_time,
    due_reminder_minutes,
    start_date,
    start_time,
    start_reminder_minutes,
    target_end_date,
    completed_at,
    archived_at,
    blocker_reason,
    milestone,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectChecklistItemRow {
    pub(crate) id: String,
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) completed_at: Option<String>,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectChecklistItemRow {
    id,
    task_id,
    title,
    completed_at,
    sort_order,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectTagRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectTagRow {
    id,
    project_id,
    name,
    color,
    sort_order,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectTaskTagLinkRow {
    pub(crate) task_id: String,
    pub(crate) tag_id: String,
    pub(crate) created_at: String,
}
impl_sqlite_from_row!(ProjectTaskTagLinkRow {
    task_id,
    tag_id,
    created_at,
});

#[derive(Deserialize, Serialize)]
pub struct ProjectCustomFieldRow {
    pub(crate) revision: i64,
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) field_type: String,
    pub(crate) sort_order: i64,
    /// Chosen `lucide:` icon; `None` shows the field type icon.
    pub(crate) icon: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectCustomFieldRow {
    revision,
    id,
    project_id,
    name,
    field_type,
    sort_order,
    icon,
    created_at,
    updated_at,
});

#[derive(Deserialize, Serialize)]
pub struct ProjectCustomFieldOptionRow {
    pub(crate) revision: i64,
    pub(crate) id: String,
    pub(crate) field_id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectCustomFieldOptionRow {
    revision,
    id,
    field_id,
    name,
    sort_order,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectCustomFieldValueRow {
    pub(crate) task_id: String,
    pub(crate) field_id: String,
    pub(crate) text_value: Option<String>,
    pub(crate) number_value: Option<f64>,
    pub(crate) date_value: Option<String>,
    pub(crate) checkbox_value: Option<i64>,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectCustomFieldValueRow {
    task_id,
    field_id,
    text_value,
    number_value,
    date_value,
    checkbox_value,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectCustomFieldOptionValueRow {
    pub(crate) task_id: String,
    pub(crate) field_id: String,
    pub(crate) option_id: String,
    pub(crate) created_at: String,
}
impl_sqlite_from_row!(ProjectCustomFieldOptionValueRow {
    task_id,
    field_id,
    option_id,
    created_at,
});

#[derive(Serialize)]
pub struct ProjectTaskDependencyRow {
    pub(crate) id: String,
    pub(crate) blocking_task_id: String,
    pub(crate) blocked_task_id: String,
    pub(crate) dependency_type: String,
    pub(crate) created_at: String,
}
impl_sqlite_from_row!(ProjectTaskDependencyRow {
    id,
    blocking_task_id,
    blocked_task_id,
    dependency_type,
    created_at,
});

#[derive(Serialize)]
pub struct ProjectTaskEventLinkRow {
    pub(crate) task_id: String,
    pub(crate) event_id: String,
    pub(crate) link_kind: String,
    pub(crate) created_at: String,
}
impl_sqlite_from_row!(ProjectTaskEventLinkRow {
    task_id,
    event_id,
    link_kind,
    created_at,
});

#[derive(Serialize, Deserialize)]
pub struct ProjectTaskChangeEventRow {
    pub(crate) id: String,
    pub(crate) task_id: String,
    pub(crate) event_type: String,
    pub(crate) field_name: Option<String>,
    pub(crate) old_value: Option<String>,
    pub(crate) new_value: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) occurred_at: String,
}
impl_sqlite_from_row!(ProjectTaskChangeEventRow {
    id,
    task_id,
    event_type,
    field_name,
    old_value,
    new_value,
    reason,
    occurred_at,
});

/// One view in a project's sidebar. A `None` name shows the localized label of the view type.
#[derive(Serialize)]
pub struct ProjectViewRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) view_type: String,
    pub(crate) name: Option<String>,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectViewRow {
    id,
    project_id,
    view_type,
    name,
    sort_order,
    created_at,
    updated_at,
});

/// A preference of one view, reported with the project that owns the view.
#[derive(Serialize)]
pub struct ProjectViewPreferenceRow {
    pub(crate) project_id: String,
    pub(crate) view_id: String,
    pub(crate) preference_key: String,
    pub(crate) preference_value: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectViewPreferenceRow {
    project_id,
    view_id,
    preference_key,
    preference_value,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectCustomEmojiRow {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) asset_path: String,
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}
impl_sqlite_from_row!(ProjectCustomEmojiRow {
    id,
    name,
    asset_path,
    sort_order,
    created_at,
    updated_at,
});

#[derive(Serialize)]
pub struct ProjectLinkableEventTask {
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) archived_at: Option<String>,
}

#[derive(Serialize)]
pub struct ProjectLinkableEvent {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) start_time: String,
    pub(crate) end_time: String,
    pub(crate) timezone: String,
    pub(crate) calendar_id: String,
    pub(crate) color: Option<i64>,
    pub(crate) all_day: i64,
    pub(crate) status: String,
    pub(crate) linked_tasks: Vec<ProjectLinkableEventTask>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLinkableEventSearch {
    pub(crate) project_id: String,
    pub(crate) task_id: Option<String>,
    pub(crate) query: Option<String>,
    pub(crate) start_date: Option<String>,
    pub(crate) end_date: Option<String>,
    pub(crate) limit: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct ProjectLinkableEventRow {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) start_time: String,
    pub(crate) end_time: String,
    pub(crate) timezone: String,
    pub(crate) calendar_id: String,
    pub(crate) color: Option<i64>,
    pub(crate) all_day: i64,
    pub(crate) status: String,
}
impl_sqlite_from_row!(ProjectLinkableEventRow {
    id,
    project_id,
    title,
    start_time,
    end_time,
    timezone,
    calendar_id,
    color,
    all_day,
    status,
});

pub(crate) struct ProjectLinkableEventTaskRow {
    pub(crate) event_id: String,
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) archived_at: Option<String>,
}
impl_sqlite_from_row!(ProjectLinkableEventTaskRow {
    event_id,
    task_id,
    title,
    archived_at,
});

#[derive(Serialize)]
pub struct ProjectsSnapshot {
    pub(crate) groups: Vec<ProjectGroupRow>,
    pub(crate) projects: Vec<ProjectRow>,
    pub(crate) sections: Vec<ProjectSectionRow>,
    pub(crate) statuses: Vec<ProjectStatusRow>,
    pub(crate) priorities: Vec<ProjectPriorityRow>,
    pub(crate) tasks: Vec<ProjectTaskRow>,
    pub(crate) checklist_items: Vec<ProjectChecklistItemRow>,
    pub(crate) tags: Vec<ProjectTagRow>,
    pub(crate) task_tag_links: Vec<ProjectTaskTagLinkRow>,
    pub(crate) custom_fields: Vec<ProjectCustomFieldRow>,
    pub(crate) custom_field_options: Vec<ProjectCustomFieldOptionRow>,
    pub(crate) custom_field_values: Vec<ProjectCustomFieldValueRow>,
    pub(crate) custom_field_option_values: Vec<ProjectCustomFieldOptionValueRow>,
    pub(crate) dependencies: Vec<ProjectTaskDependencyRow>,
    pub(crate) event_links: Vec<ProjectTaskEventLinkRow>,
    pub(crate) task_change_events: Vec<ProjectTaskChangeEventRow>,
    pub(crate) view_preferences: Vec<ProjectViewPreferenceRow>,
    pub(crate) views: Vec<ProjectViewRow>,
    pub(crate) custom_emojis: Vec<ProjectCustomEmojiRow>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectViewId {
    List,
    Kanban,
    Calendar,
    Gantt,
    Dashboard,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskCustomFieldFilterRequest {
    pub(crate) field_id: String,
    pub(crate) mode: String,
    pub(crate) checked: Option<bool>,
    pub(crate) option_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskViewRequest {
    pub(crate) project_id: String,
    pub(crate) view: ProjectViewId,
    pub(crate) page_size: i64,
    pub(crate) cursor: Option<String>,
    pub(crate) column_cursors: HashMap<String, String>,
    pub(crate) show_archived: bool,
    pub(crate) visible_section_ids: Vec<String>,
    pub(crate) search: String,
    pub(crate) status_filter: String,
    pub(crate) section_filter: String,
    pub(crate) priority_filter: String,
    pub(crate) due_filter: String,
    pub(crate) due_range_start: String,
    pub(crate) due_range_end: String,
    pub(crate) today: String,
    pub(crate) week_end: String,
    pub(crate) schedule_filter: String,
    pub(crate) dependency_filter: String,
    pub(crate) tag_filter: String,
    pub(crate) custom_field_filters: Vec<ProjectTaskCustomFieldFilterRequest>,
    pub(crate) sort_mode: String,
    pub(crate) sort_direction: String,
    pub(crate) candidate_event_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct ProjectTaskColumnCount {
    pub(crate) status_id: String,
    pub(crate) count: i64,
    pub(crate) next_cursor: Option<String>,
}

#[derive(Default, Serialize)]
pub struct ProjectDashboardTaskAggregates {
    pub(crate) total: i64,
    pub(crate) completed: i64,
    pub(crate) open_estimate_minutes: i64,
    pub(crate) blocked: i64,
    pub(crate) overdue: i64,
    pub(crate) unscheduled_due: i64,
    pub(crate) missing_estimate: i64,
    pub(crate) status_counts: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct ProjectTaskViewPage {
    pub(crate) project_id: String,
    pub(crate) view: ProjectViewId,
    pub(crate) tasks: Vec<ProjectTaskSummaryRow>,
    pub(crate) total_count: i64,
    pub(crate) matched_count: i64,
    pub(crate) archived_count: i64,
    pub(crate) next_cursor: Option<String>,
    pub(crate) column_counts: Vec<ProjectTaskColumnCount>,
    pub(crate) aggregates: Option<ProjectDashboardTaskAggregates>,
    pub(crate) column_calculations: Vec<ProjectTaskColumnCalculations>,
    pub(crate) matched_event_ids: Vec<String>,
    pub(crate) task_tag_links: Vec<ProjectTaskTagLinkRow>,
    pub(crate) custom_field_values: Vec<ProjectCustomFieldValueRow>,
    pub(crate) custom_field_option_values: Vec<ProjectCustomFieldOptionValueRow>,
    pub(crate) dependencies: Vec<ProjectTaskDependencyRow>,
    pub(crate) event_links: Vec<ProjectTaskEventLinkRow>,
    pub(crate) tags: Vec<ProjectTagRow>,
    pub(crate) custom_fields: Vec<ProjectCustomFieldRow>,
    pub(crate) custom_field_options: Vec<ProjectCustomFieldOptionRow>,
}

/// Property reductions over the complete matched task set, excluding pagination and parent context.
#[derive(Serialize)]
pub struct ProjectTaskColumnCalculations {
    pub(crate) column: String,
    pub(crate) total: i64,
    pub(crate) filled: i64,
    pub(crate) sum: Option<f64>,
    pub(crate) average: Option<f64>,
    pub(crate) minimum: Option<f64>,
    pub(crate) maximum: Option<f64>,
}

#[derive(Serialize)]
pub struct ProjectTaskDetailData {
    pub(crate) task: ProjectTaskRow,
    pub(crate) related_tasks: Vec<ProjectTaskSummaryRow>,
    pub(crate) checklist_items: Vec<ProjectChecklistItemRow>,
    pub(crate) tags: Vec<ProjectTagRow>,
    pub(crate) task_tag_links: Vec<ProjectTaskTagLinkRow>,
    pub(crate) custom_fields: Vec<ProjectCustomFieldRow>,
    pub(crate) custom_field_options: Vec<ProjectCustomFieldOptionRow>,
    pub(crate) custom_field_values: Vec<ProjectCustomFieldValueRow>,
    pub(crate) custom_field_option_values: Vec<ProjectCustomFieldOptionValueRow>,
    pub(crate) dependencies: Vec<ProjectTaskDependencyRow>,
    pub(crate) event_links: Vec<ProjectTaskEventLinkRow>,
    pub(crate) task_change_events: Vec<ProjectTaskChangeEventRow>,
}

#[derive(Serialize)]
pub struct ProjectsWorkspaceSnapshot {
    pub(crate) resolved_project_id: Option<String>,
    pub(crate) snapshot: ProjectsSnapshot,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectOptionalDataKind {
    Relationships,
    CustomFields,
    History,
    Checklist,
    ViewPreferences,
    CustomEmojis,
}

#[derive(Serialize)]
pub struct ProjectsOptionalData {
    pub(crate) kind: ProjectOptionalDataKind,
    pub(crate) project_id: Option<String>,
    pub(crate) checklist_items: Vec<ProjectChecklistItemRow>,
    pub(crate) tags: Vec<ProjectTagRow>,
    pub(crate) task_tag_links: Vec<ProjectTaskTagLinkRow>,
    pub(crate) custom_fields: Vec<ProjectCustomFieldRow>,
    pub(crate) custom_field_options: Vec<ProjectCustomFieldOptionRow>,
    pub(crate) custom_field_values: Vec<ProjectCustomFieldValueRow>,
    pub(crate) custom_field_option_values: Vec<ProjectCustomFieldOptionValueRow>,
    pub(crate) dependencies: Vec<ProjectTaskDependencyRow>,
    pub(crate) event_links: Vec<ProjectTaskEventLinkRow>,
    pub(crate) task_change_events: Vec<ProjectTaskChangeEventRow>,
    pub(crate) view_preferences: Vec<ProjectViewPreferenceRow>,
    pub(crate) custom_emojis: Vec<ProjectCustomEmojiRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupCreate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) collapsed: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreate {
    pub(crate) id: String,
    pub(crate) group_id: String,
    pub(crate) template_id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) default_event_name: Option<String>,
    pub(crate) default_event_time_mode: String,
    pub(crate) default_event_duration_minutes: Option<i64>,
    pub(crate) default_pomodoro_mode: String,
    pub(crate) default_pomodoro_preset_key: Option<String>,
    pub(crate) default_pomodoro_focus_minutes: Option<i64>,
    pub(crate) default_pomodoro_short_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_after_focus_count: Option<i64>,
    pub(crate) default_idle_settings_source: String,
    pub(crate) default_idle_pause_enabled: bool,
    pub(crate) default_idle_threshold_minutes: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectUpdate {
    pub(crate) id: String,
    pub(crate) group_id: String,
    pub(crate) name: String,
    pub(crate) icon: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
    pub(crate) status: String,
    pub(crate) default_event_name: Option<String>,
    pub(crate) default_event_time_mode: String,
    pub(crate) default_event_duration_minutes: Option<i64>,
    pub(crate) default_pomodoro_mode: String,
    pub(crate) default_pomodoro_preset_key: Option<String>,
    pub(crate) default_pomodoro_focus_minutes: Option<i64>,
    pub(crate) default_pomodoro_short_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_minutes: Option<i64>,
    pub(crate) default_pomodoro_long_break_after_focus_count: Option<i64>,
    pub(crate) default_idle_settings_source: String,
    pub(crate) default_idle_pause_enabled: bool,
    pub(crate) default_idle_threshold_minutes: i64,
    pub(crate) focus_playlist_id: Option<String>,
    pub(crate) break_playlist_id: Option<String>,
    pub(crate) work_environment_id: Option<String>,
    pub(crate) blocker_ruleset_id: Option<String>,
    pub(crate) music_assignments:
        Option<Vec<ganbaru_music::assignments::MusicContextAssignmentDraft>>,
    pub(crate) music_assignments_updated_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSectionCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSectionUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
    pub(crate) collapsed: bool,
    pub(crate) hidden_at: Option<String>,
    pub(crate) archived_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatusCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
    pub(crate) terminal: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatusUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) category: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
    pub(crate) terminal: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPriorityCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPriorityUpdate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) color: i64,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) section_id: String,
    pub(crate) status_id: String,
    pub(crate) parent_task_id: Option<String>,
    pub(crate) title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskUpdate {
    pub(crate) id: String,
    pub(crate) section_id: String,
    pub(crate) status_id: String,
    pub(crate) parent_task_id: Option<String>,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) priority: String,
    pub(crate) task_type: String,
    pub(crate) section_sort_order: f64,
    pub(crate) status_sort_order: f64,
    pub(crate) estimate_minutes: Option<i64>,
    pub(crate) due_date: Option<String>,
    pub(crate) due_time: Option<String>,
    pub(crate) due_reminder_minutes: Option<i64>,
    pub(crate) start_date: Option<String>,
    pub(crate) start_time: Option<String>,
    pub(crate) start_reminder_minutes: Option<i64>,
    pub(crate) target_end_date: Option<String>,
    pub(crate) archived_at: Option<String>,
    pub(crate) blocker_reason: Option<String>,
    pub(crate) milestone: bool,
    pub(crate) change_reason: Option<String>,
    /// Revision the edit was based on. When present, the update is refused if the task changed since, so a stale
    /// window never overwrites fields it did not edit.
    #[serde(default)]
    pub(crate) expected_revision: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskDependencyCreate {
    pub(crate) id: String,
    pub(crate) blocking_task_id: String,
    pub(crate) blocked_task_id: String,
    pub(crate) dependency_type: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChecklistItemCreate {
    pub(crate) id: String,
    pub(crate) task_id: String,
    pub(crate) title: String,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChecklistItemUpdate {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) completed: bool,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTagCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTagUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) color: Option<i64>,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskTagLinkCreate {
    pub(crate) task_id: String,
    pub(crate) tag_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomFieldCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) name: String,
    pub(crate) field_type: String,
    pub(crate) sort_order: i64,
    #[serde(default)]
    pub(crate) icon: Option<String>,
    #[serde(default)]
    pub(crate) duplicate_source_id: Option<String>,
}

/// Full replacement of a field's editable definition; a missing `icon` resets it to the type icon.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomFieldUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
    #[serde(default)]
    pub(crate) icon: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomFieldOptionCreate {
    pub(crate) id: String,
    pub(crate) field_id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomFieldOptionUpdate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) sort_order: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomFieldValueUpdate {
    pub(crate) task_id: String,
    pub(crate) field_id: String,
    pub(crate) text_value: Option<String>,
    pub(crate) number_value: Option<f64>,
    pub(crate) date_value: Option<String>,
    pub(crate) checkbox_value: Option<bool>,
    pub(crate) option_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTaskEventLinkCreate {
    pub(crate) task_id: String,
    pub(crate) event_id: String,
    pub(crate) link_kind: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectViewPreferenceUpsert {
    pub(crate) view_id: String,
    pub(crate) preference_key: String,
    pub(crate) preference_value: String,
}

/// Adds a view at the end of a project's views.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectViewCreate {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) view_type: String,
    pub(crate) name: Option<String>,
}

/// Renames a view; `None` or a blank name restores the label of its type.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectViewRename {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
}

/// The complete new order of a project's views.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectViewReorder {
    pub(crate) project_id: String,
    pub(crate) view_ids: Vec<String>,
}

/// The complete new order of every project group.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGroupReorder {
    pub(crate) group_ids: Vec<String>,
}

/// The complete new order of the projects in one group.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupProjectReorder {
    pub(crate) group_id: String,
    pub(crate) project_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCustomEmojiCreate {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) asset_path: String,
    pub(crate) sort_order: i64,
}
