use sqlx::SqlitePool;

use super::models::*;
use super::routine::ensure_built_in_routine_defaults;
pub use super::task_views::{load_task_detail, load_task_view};

/// Loads the workspace after repairing built-in Routine defaults.
pub async fn load_workspace(
    pool: &SqlitePool,
    preferred_project_id: Option<&str>,
    active_view: ProjectViewId,
) -> Result<ProjectsWorkspaceSnapshot, String> {
    load_projects_workspace(pool, preferred_project_id, active_view, true).await
}

/// Reloads the workspace without built-in repair, so refreshes stay read-only.
pub async fn refresh_workspace(
    pool: &SqlitePool,
    preferred_project_id: Option<&str>,
    active_view: ProjectViewId,
) -> Result<ProjectsWorkspaceSnapshot, String> {
    load_projects_workspace(pool, preferred_project_id, active_view, false).await
}

fn resolved_project_id(
    projects: &[ProjectRow],
    preferred_project_id: Option<&str>,
) -> Option<String> {
    if let Some(preferred) = preferred_project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if projects.iter().any(|project| project.id == preferred) {
            return Some(preferred.to_string());
        }
    }
    projects
        .iter()
        .filter(|project| project.status == "active")
        .min_by(|left, right| {
            left.sort_order
                .cmp(&right.sort_order)
                .then_with(|| left.name.cmp(&right.name))
        })
        .or_else(|| projects.first())
        .map(|project| project.id.clone())
}

async fn load_projects_workspace(
    pool: &sqlx::SqlitePool,
    preferred_project_id: Option<&str>,
    active_view: ProjectViewId,
    repair_built_ins: bool,
) -> Result<ProjectsWorkspaceSnapshot, String> {
    if repair_built_ins {
        ensure_built_in_routine_defaults(pool).await?;
    }
    let groups = sqlx::query_as::<_, ProjectGroupRow>(
        "SELECT * FROM project_groups ORDER BY sort_order ASC, name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project groups: {e}"))?;
    let projects = sqlx::query_as::<_, ProjectRow>(
        "SELECT * FROM projects ORDER BY group_id ASC, sort_order ASC, name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load projects: {e}"))?;
    let resolved_project_id = resolved_project_id(&projects, preferred_project_id);
    let mut snapshot = load_projects_core_snapshot(pool, resolved_project_id.as_deref()).await?;
    snapshot.groups = groups;
    snapshot.projects = projects;
    Ok(ProjectsWorkspaceSnapshot {
        resolved_project_id,
        active_view,
        snapshot,
    })
}

async fn load_projects_core_snapshot(
    pool: &sqlx::SqlitePool,
    project_id: Option<&str>,
) -> Result<ProjectsSnapshot, String> {
    let normalized_project_id = project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("\0");
    let sections = sqlx::query_as::<_, ProjectSectionRow>(
        "SELECT * FROM project_sections
         WHERE project_id = ?
         ORDER BY project_id ASC, sort_order ASC, name ASC",
    )
    .bind(normalized_project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project sections: {e}"))?;
    let statuses = sqlx::query_as::<_, ProjectStatusRow>(
        "SELECT * FROM project_statuses
         WHERE project_id = ?
         ORDER BY project_id ASC, sort_order ASC, name ASC",
    )
    .bind(normalized_project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project statuses: {e}"))?;
    let priorities = sqlx::query_as::<_, ProjectPriorityRow>(
        "SELECT * FROM project_priorities
         WHERE project_id = ?
         ORDER BY project_id ASC, sort_order ASC, name ASC",
    )
    .bind(normalized_project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project priorities: {e}"))?;
    Ok(ProjectsSnapshot {
        groups: Vec::new(),
        projects: Vec::new(),
        sections,
        statuses,
        priorities,
        tasks: Vec::new(),
        checklist_items: Vec::new(),
        tags: Vec::new(),
        task_tag_links: Vec::new(),
        custom_fields: Vec::new(),
        custom_field_options: Vec::new(),
        custom_field_values: Vec::new(),
        custom_field_option_values: Vec::new(),
        dependencies: Vec::new(),
        event_links: Vec::new(),
        task_change_events: Vec::new(),
        view_preferences: Vec::new(),
        custom_emojis: Vec::new(),
    })
}

fn required_optional_project_id(
    project_id: Option<&str>,
    kind: ProjectOptionalDataKind,
) -> Result<&str, String> {
    project_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("project id is required for {kind:?} data"))
}

/// Loads one optional data kind on demand, outside the workspace snapshot.
pub async fn load_optional_data(
    pool: &sqlx::SqlitePool,
    project_id: Option<&str>,
    kind: ProjectOptionalDataKind,
) -> Result<ProjectsOptionalData, String> {
    let mut result = ProjectsOptionalData {
        kind,
        project_id: project_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        checklist_items: Vec::new(),
        tags: Vec::new(),
        task_tag_links: Vec::new(),
        custom_fields: Vec::new(),
        custom_field_options: Vec::new(),
        custom_field_values: Vec::new(),
        custom_field_option_values: Vec::new(),
        dependencies: Vec::new(),
        event_links: Vec::new(),
        task_change_events: Vec::new(),
        view_preferences: Vec::new(),
        custom_emojis: Vec::new(),
    };

    match kind {
        ProjectOptionalDataKind::Relationships => {
            let project_id = required_optional_project_id(project_id, kind)?;
            result.tags = sqlx::query_as::<_, ProjectTagRow>(
                "SELECT * FROM project_tags
                 WHERE project_id = ?
                 ORDER BY project_id ASC, sort_order ASC, name ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project tags: {e}"))?;
            result.task_tag_links = sqlx::query_as::<_, ProjectTaskTagLinkRow>(
                "SELECT project_task_tag_links.*
                 FROM project_task_tag_links
                 JOIN project_tasks ON project_tasks.id = project_task_tag_links.task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_task_tag_links.created_at ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project task tag links: {e}"))?;
            result.dependencies = sqlx::query_as::<_, ProjectTaskDependencyRow>(
                "SELECT project_task_dependencies.*
                 FROM project_task_dependencies
                 JOIN project_tasks ON project_tasks.id = project_task_dependencies.blocked_task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_task_dependencies.created_at ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project task dependencies: {e}"))?;
            result.event_links = sqlx::query_as::<_, ProjectTaskEventLinkRow>(
                "SELECT project_task_event_links.*
                 FROM project_task_event_links
                 JOIN project_tasks ON project_tasks.id = project_task_event_links.task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_task_event_links.created_at ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project task event links: {e}"))?;
        }
        ProjectOptionalDataKind::CustomFields => {
            let project_id = required_optional_project_id(project_id, kind)?;
            result.custom_fields = sqlx::query_as::<_, ProjectCustomFieldRow>(
                "SELECT * FROM project_custom_fields
                 WHERE project_id = ?
                 ORDER BY project_id ASC, sort_order ASC, name ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project custom fields: {e}"))?;
            result.custom_field_options = sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
                "SELECT project_custom_field_options.*
                 FROM project_custom_field_options
                 JOIN project_custom_fields ON project_custom_fields.id = project_custom_field_options.field_id
                 WHERE project_custom_fields.project_id = ?
                 ORDER BY project_custom_field_options.field_id ASC, project_custom_field_options.sort_order ASC, project_custom_field_options.name ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project custom field options: {e}"))?;
            result.custom_field_values = sqlx::query_as::<_, ProjectCustomFieldValueRow>(
                "SELECT project_custom_field_values.*
                 FROM project_custom_field_values
                 JOIN project_tasks ON project_tasks.id = project_custom_field_values.task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_custom_field_values.updated_at ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project custom field values: {e}"))?;
            result.custom_field_option_values =
                sqlx::query_as::<_, ProjectCustomFieldOptionValueRow>(
                    "SELECT project_custom_field_option_values.*
                     FROM project_custom_field_option_values
                     JOIN project_tasks ON project_tasks.id = project_custom_field_option_values.task_id
                     WHERE project_tasks.project_id = ?
                     ORDER BY project_custom_field_option_values.created_at ASC",
                )
                .bind(project_id)
                .fetch_all(pool)
                .await
                .map_err(|e| format!("load project custom field option values: {e}"))?;
        }
        ProjectOptionalDataKind::History => {
            let project_id = required_optional_project_id(project_id, kind)?;
            result.task_change_events = sqlx::query_as::<_, ProjectTaskChangeEventRow>(
                "SELECT project_task_change_events.*
                 FROM project_task_change_events
                 JOIN project_tasks ON project_tasks.id = project_task_change_events.task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_task_change_events.occurred_at DESC
                 LIMIT 500",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project task change events: {e}"))?;
        }
        ProjectOptionalDataKind::Checklist => {
            let project_id = required_optional_project_id(project_id, kind)?;
            result.checklist_items = sqlx::query_as::<_, ProjectChecklistItemRow>(
                "SELECT project_checklist_items.*
                 FROM project_checklist_items
                 JOIN project_tasks ON project_tasks.id = project_checklist_items.task_id
                 WHERE project_tasks.project_id = ?
                 ORDER BY project_checklist_items.task_id ASC, project_checklist_items.sort_order ASC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project checklist items: {e}"))?;
        }
        ProjectOptionalDataKind::SavedViews => {
            let project_id = required_optional_project_id(project_id, kind)?;
            result.view_preferences = sqlx::query_as::<_, ProjectViewPreferenceRow>(
                "SELECT * FROM project_view_preferences
                 WHERE project_id = ?
                 ORDER BY view_id ASC, updated_at DESC",
            )
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project view preferences: {e}"))?;
        }
        ProjectOptionalDataKind::CustomEmojis => {
            if result.project_id.is_some() {
                return Err("project id must be omitted for custom emoji data".to_string());
            }
            result.custom_emojis = load_project_custom_emojis(pool).await?;
        }
    }

    Ok(result)
}

pub(super) async fn load_project_custom_emojis(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<ProjectCustomEmojiRow>, String> {
    sqlx::query_as::<_, ProjectCustomEmojiRow>(
        "SELECT * FROM project_custom_emojis ORDER BY sort_order ASC, name ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project custom emoji: {e}"))
}
