use super::history::insert_task_change_event;
use super::models::{
    CalendarEventProjectAssignment, ProjectChecklistItemRow, ProjectCustomEmojiRow,
    ProjectCustomFieldOptionRow, ProjectCustomFieldRow, ProjectGroupRow, ProjectPriorityRow,
    ProjectRow, ProjectSectionRow, ProjectStatusRow, ProjectTagRow, ProjectTaskChangeEventRow,
    ProjectTaskDependencyRow, ProjectTaskEventLinkRow, ProjectTaskRow, ProjectTaskTagLinkRow,
    ProjectViewPreferenceRow, ProjectsMutationRows,
};
use super::order::OrderGroup;

macro_rules! define_mutation_loader {
    ($name:ident, $field:ident, $row:ty, $table:literal) => {
        pub(super) async fn $name(
            pool: &sqlx::SqlitePool,
            id: &str,
        ) -> Result<ProjectsMutationRows, String> {
            let row = sqlx::query_as::<_, $row>(concat!("SELECT * FROM ", $table, " WHERE id = ?"))
                .bind(id)
                .fetch_one(pool)
                .await
                .map_err(|e| format!("load mutation result from {}: {e}", $table))?;
            let mut mutation = ProjectsMutationRows::default();
            mutation.$field.push(row);
            Ok(mutation)
        }
    };
}

define_mutation_loader!(group_mutation, groups, ProjectGroupRow, "project_groups");
define_mutation_loader!(project_mutation, projects, ProjectRow, "projects");
define_mutation_loader!(
    section_mutation,
    sections,
    ProjectSectionRow,
    "project_sections"
);
define_mutation_loader!(
    status_mutation,
    statuses,
    ProjectStatusRow,
    "project_statuses"
);
/// Priority ids are unique only within their project, so the loader takes both.
pub(super) async fn priority_mutation(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    id: &str,
) -> Result<ProjectsMutationRows, String> {
    let row = sqlx::query_as::<_, ProjectPriorityRow>(
        "SELECT * FROM project_priorities WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load mutation result from project_priorities: {e}"))?;
    Ok(ProjectsMutationRows {
        priorities: vec![row],
        ..ProjectsMutationRows::default()
    })
}
define_mutation_loader!(task_mutation_base, tasks, ProjectTaskRow, "project_tasks");
define_mutation_loader!(
    checklist_item_mutation_base,
    checklist_items,
    ProjectChecklistItemRow,
    "project_checklist_items"
);
define_mutation_loader!(tag_mutation, tags, ProjectTagRow, "project_tags");
define_mutation_loader!(
    custom_field_mutation,
    custom_fields,
    ProjectCustomFieldRow,
    "project_custom_fields"
);
define_mutation_loader!(
    custom_field_option_mutation,
    custom_field_options,
    ProjectCustomFieldOptionRow,
    "project_custom_field_options"
);
define_mutation_loader!(
    dependency_mutation_base,
    dependencies,
    ProjectTaskDependencyRow,
    "project_task_dependencies"
);
define_mutation_loader!(
    custom_emoji_mutation,
    custom_emojis,
    ProjectCustomEmojiRow,
    "project_custom_emojis"
);

pub(super) async fn latest_task_change_events(
    pool: &sqlx::SqlitePool,
    task_id: &str,
) -> Result<Vec<ProjectTaskChangeEventRow>, String> {
    sqlx::query_as::<_, ProjectTaskChangeEventRow>(
        "SELECT * FROM project_task_change_events
         WHERE task_id = ?
         ORDER BY occurred_at DESC, id DESC
         LIMIT 32",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load mutation task history: {e}"))
}

pub(super) async fn task_mutation(
    pool: &sqlx::SqlitePool,
    task_id: &str,
) -> Result<ProjectsMutationRows, String> {
    let mut mutation = task_mutation_base(pool, task_id).await?;
    mutation.task_change_events = latest_task_change_events(pool, task_id).await?;
    Ok(mutation)
}

pub(super) async fn checklist_item_mutation(
    pool: &sqlx::SqlitePool,
    item_id: &str,
    task_id: &str,
) -> Result<ProjectsMutationRows, String> {
    let mut mutation = checklist_item_mutation_base(pool, item_id).await?;
    mutation.task_change_events = latest_task_change_events(pool, task_id).await?;
    Ok(mutation)
}

pub(super) async fn dependency_mutation(
    pool: &sqlx::SqlitePool,
    dependency_id: &str,
    task_id: &str,
) -> Result<ProjectsMutationRows, String> {
    let mut mutation = dependency_mutation_base(pool, dependency_id).await?;
    mutation.task_change_events = latest_task_change_events(pool, task_id).await?;
    Ok(mutation)
}

pub(super) async fn task_tag_link_mutation(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    tag_id: &str,
) -> Result<ProjectsMutationRows, String> {
    let link = sqlx::query_as::<_, ProjectTaskTagLinkRow>(
        "SELECT * FROM project_task_tag_links WHERE task_id = ? AND tag_id = ?",
    )
    .bind(task_id)
    .bind(tag_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load task tag mutation result: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.task_tag_links.push(link);
    mutation.task_change_events = latest_task_change_events(pool, task_id).await?;
    Ok(mutation)
}

pub(super) async fn event_link_mutation(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    event_id: &str,
) -> Result<ProjectsMutationRows, String> {
    let link = sqlx::query_as::<_, ProjectTaskEventLinkRow>(
        "SELECT * FROM project_task_event_links WHERE task_id = ? AND event_id = ?",
    )
    .bind(task_id)
    .bind(event_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load task event mutation result: {e}"))?;
    let project_id: String =
        sqlx::query_scalar("SELECT project_id FROM calendar_events WHERE id = ?")
            .bind(event_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("load assigned calendar event project: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.event_links.push(link);
    mutation.task_change_events = latest_task_change_events(pool, task_id).await?;
    mutation
        .calendar_event_project_assignments
        .push(CalendarEventProjectAssignment {
            event_id: event_id.to_string(),
            project_id,
        });
    Ok(mutation)
}

pub(super) async fn view_preference_mutation(
    pool: &sqlx::SqlitePool,
    view_id: &str,
    preference_key: &str,
) -> Result<ProjectsMutationRows, String> {
    let preference = sqlx::query_as::<_, ProjectViewPreferenceRow>(
        "SELECT project_views.project_id, project_view_preferences.*
         FROM project_view_preferences
         JOIN project_views ON project_views.id = project_view_preferences.view_id
         WHERE project_view_preferences.view_id = ? AND project_view_preferences.preference_key = ?",
    )
    .bind(view_id)
    .bind(preference_key)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load view preference mutation result: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.view_preferences.push(preference);
    Ok(mutation)
}

/// Marks a status deleted. The project keeps at least one live status, and a status still used by a task stays.
/// Deleted statuses remain readable so tasks synced from another device still show them.
pub(super) async fn delete_unused_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
) -> Result<(), String> {
    let project_id = sqlx::query_scalar::<_, String>(
        "SELECT project_id FROM project_statuses WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(status_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load project status: {e}"))?
    .ok_or_else(|| "project status not found".to_string())?;

    let status_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_statuses WHERE project_id = ? AND deleted_at IS NULL",
    )
    .bind(&project_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("count project statuses: {e}"))?;
    if status_count <= 1 {
        return Err("project must keep at least one task status".to_string());
    }

    let task_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE status_id = ?")
            .bind(status_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("count status tasks: {e}"))?;
    if task_count > 0 {
        return Err("move or delete tasks before deleting this task status".to_string());
    }

    sqlx::query(
        "UPDATE project_statuses
         SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(status_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("delete project status: {e}"))?;
    Ok(())
}

/// Marks a priority deleted. The project keeps at least one live priority, and a priority still used by a task stays.
pub(super) async fn delete_unused_priority(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    priority_id: &str,
) -> Result<(), String> {
    let priority_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_priorities
         WHERE project_id = ? AND id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(priority_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load project priority: {e}"))?;
    if priority_exists == 0 {
        return Err("project priority not found".to_string());
    }

    let priority_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_priorities WHERE project_id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("count project priorities: {e}"))?;
    if priority_count <= 1 {
        return Err("project must keep at least one task priority".to_string());
    }

    let task_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_tasks WHERE project_id = ? AND priority = ?",
    )
    .bind(project_id)
    .bind(priority_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("count priority tasks: {e}"))?;
    if task_count > 0 {
        return Err("move or delete tasks before deleting this task priority".to_string());
    }

    sqlx::query(
        "UPDATE project_priorities
         SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(priority_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("delete project priority: {e}"))?;
    Ok(())
}

/// Section order of a task: top-level tasks order within their section, subtasks within their parent.
pub(super) fn task_section_order<'a>(
    project_id: &'a str,
    section_id: &'a str,
    parent_task_id: Option<&'a str>,
) -> OrderGroup<'a> {
    let group = OrderGroup::new("project_tasks", "section_order_key");
    match parent_task_id {
        Some(parent_task_id) => group.filter("parent_task_id", Some(parent_task_id)),
        None => group
            .filter("project_id", Some(project_id))
            .filter("section_id", Some(section_id))
            .filter("parent_task_id", None),
    }
}

/// Status order of a task: tasks order within their status, separately for top-level tasks and each parent.
pub(super) fn task_status_order<'a>(
    project_id: &'a str,
    status_id: &'a str,
    parent_task_id: Option<&'a str>,
) -> OrderGroup<'a> {
    OrderGroup::new("project_tasks", "status_order_key")
        .filter("project_id", Some(project_id))
        .filter("status_id", Some(status_id))
        .filter("parent_task_id", parent_task_id)
}

/// Refuses a status that was deleted, so only live statuses are newly assigned.
pub(super) async fn ensure_status_is_live(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
) -> Result<(), String> {
    let deleted: Option<Option<String>> =
        sqlx::query_scalar("SELECT deleted_at FROM project_statuses WHERE id = ?")
            .bind(status_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("check project status: {e}"))?;
    match deleted {
        None => Err("project status not found".to_string()),
        Some(Some(_)) => Err("task status was deleted".to_string()),
        Some(None) => Ok(()),
    }
}

/// Refuses a priority that was deleted, so only live priorities are newly assigned.
pub(super) async fn ensure_priority_is_live(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    priority_id: &str,
) -> Result<(), String> {
    let deleted: Option<Option<String>> = sqlx::query_scalar(
        "SELECT deleted_at FROM project_priorities WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(priority_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("check project priority: {e}"))?;
    match deleted {
        Some(Some(_)) => Err("task priority was deleted".to_string()),
        _ => Ok(()),
    }
}

/// Rows whose names are unique within a scope.
#[derive(Clone, Copy)]
pub(super) enum UniqueName<'a> {
    /// Tags within the project with this id.
    Tag(&'a str),
    /// Custom fields within the project with this id.
    CustomField(&'a str),
    /// Options within the custom field with this id.
    CustomFieldOption(&'a str),
}

/// Refuses a name already used by another row of the same scope, compared case-insensitively. The check is local:
/// rows merged from another device may still share a name.
pub(super) async fn ensure_name_is_unique(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    unique: UniqueName<'_>,
    name: &str,
    except_id: Option<&str>,
) -> Result<(), String> {
    let (table, scope_column, scope_id, message) = match unique {
        UniqueName::Tag(project_id) => (
            "project_tags",
            "project_id",
            project_id,
            "a tag with this name already exists",
        ),
        UniqueName::CustomField(project_id) => (
            "project_custom_fields",
            "project_id",
            project_id,
            "a custom field with this name already exists",
        ),
        UniqueName::CustomFieldOption(field_id) => (
            "project_custom_field_options",
            "field_id",
            field_id,
            "an option with this name already exists",
        ),
    };
    let sql = format!(
        "SELECT COUNT(*) FROM {table}
         WHERE {scope_column} = ? AND lower(name) = lower(?) AND id IS NOT ?"
    );
    let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(scope_id)
        .bind(name.trim())
        .bind(except_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check {table} name: {e}"))?;
    if count > 0 {
        return Err(message.to_string());
    }
    Ok(())
}

pub(super) async fn ensure_section_and_status_match_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    section_id: &str,
    status_id: &str,
) -> Result<(), String> {
    let section_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_sections WHERE id = ? AND project_id = ?")
            .bind(section_id)
            .bind(project_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("check project section: {e}"))?;
    if section_count == 0 {
        return Err("section does not belong to project".to_string());
    }
    let status_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_statuses WHERE id = ? AND project_id = ?")
            .bind(status_id)
            .bind(project_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("check project status: {e}"))?;
    if status_count == 0 {
        return Err("status does not belong to project".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_priority_matches_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    priority_id: &str,
) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_priorities WHERE id = ? AND project_id = ?",
    )
    .bind(priority_id)
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check project priority: {e}"))?;
    if count > 0 {
        return Ok(());
    }
    let project_priority_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_priorities WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("count project priorities: {e}"))?;
    if project_priority_count == 0 && matches!(priority_id, "low" | "normal" | "high" | "urgent") {
        return Ok(());
    }
    if count == 0 {
        return Err("priority does not belong to project".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_parent_task_matches_project(
    conn: &mut sqlx::SqliteConnection,
    project_id: &str,
    parent_task_id: &str,
) -> Result<(), String> {
    if !can_parent_subtasks(conn, project_id, parent_task_id).await? {
        return Err("parent task must be a top-level task in the same project".to_string());
    }
    Ok(())
}

/// Whether `parent_task_id` can hold subtasks of a task in `project_id`: it is a top-level task
/// of that project, which keeps subtasks one level deep.
pub(super) async fn can_parent_subtasks(
    conn: &mut sqlx::SqliteConnection,
    project_id: &str,
    parent_task_id: &str,
) -> Result<bool, String> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
             FROM project_tasks
             WHERE id = ? AND project_id = ? AND parent_task_id IS NULL",
    )
    .bind(parent_task_id)
    .bind(project_id)
    .fetch_one(conn)
    .await
    .map_err(|e| format!("check parent project task: {e}"))?;
    Ok(count > 0)
}

pub(super) async fn ensure_task_has_no_children(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<(), String> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE parent_task_id = ?")
            .bind(task_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("check child project tasks: {e}"))?;
    if count > 0 {
        return Err("task with subtasks cannot become a subtask".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_project_exists_in_pool(
    pool: &sqlx::SqlitePool,
    project_id: &str,
) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("check project: {e}"))?;
    if count == 0 {
        return Err("project not found".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_task_belongs_to_project_in_pool(
    pool: &sqlx::SqlitePool,
    task_id: &str,
    project_id: &str,
) -> Result<(), String> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE id = ? AND project_id = ?")
            .bind(task_id)
            .bind(project_id)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("check project task: {e}"))?;
    if count == 0 {
        return Err("project task does not belong to project".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_task_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check project task: {e}"))?;
    if count == 0 {
        return Err("project task not found".to_string());
    }
    Ok(())
}

pub(super) async fn project_id_for_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<String, String> {
    sqlx::query_scalar("SELECT project_id FROM project_tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load project task: {e}"))?
        .ok_or_else(|| "project task not found".to_string())
}

pub(super) async fn ensure_dependency_tasks_match_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    blocking_task_id: &str,
    blocked_task_id: &str,
) -> Result<(), String> {
    let blocking_project_id = project_id_for_task(tx, blocking_task_id).await?;
    let blocked_project_id = project_id_for_task(tx, blocked_task_id).await?;
    if blocking_project_id != blocked_project_id {
        return Err("dependency tasks must belong to the same project".to_string());
    }
    Ok(())
}

pub(super) async fn ensure_dependency_has_no_cycle(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    blocking_task_id: &str,
    blocked_task_id: &str,
) -> Result<(), String> {
    if dependency_would_cycle(tx, blocking_task_id, blocked_task_id).await? {
        return Err("dependency would create a cycle".to_string());
    }
    Ok(())
}

/// Whether a dependency from `blocking_task_id` to `blocked_task_id` would close a cycle over the
/// dependencies that exist now.
pub(super) async fn dependency_would_cycle(
    conn: &mut sqlx::SqliteConnection,
    blocking_task_id: &str,
    blocked_task_id: &str,
) -> Result<bool, String> {
    let cycle_count: i64 = sqlx::query_scalar(
        "WITH RECURSIVE dependency_chain(task_id) AS (
             SELECT blocked_task_id
             FROM project_task_dependencies
             WHERE blocking_task_id = ?
             UNION
             SELECT project_task_dependencies.blocked_task_id
             FROM project_task_dependencies
             JOIN dependency_chain
               ON project_task_dependencies.blocking_task_id = dependency_chain.task_id
         )
         SELECT COUNT(*)
         FROM dependency_chain
         WHERE task_id = ?",
    )
    .bind(blocked_task_id)
    .bind(blocking_task_id)
    .fetch_one(conn)
    .await
    .map_err(|e| format!("check project task dependency cycle: {e}"))?;
    Ok(cycle_count > 0)
}

pub(super) async fn task_id_for_checklist_item(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    item_id: &str,
) -> Result<String, String> {
    sqlx::query_scalar("SELECT task_id FROM project_checklist_items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load project checklist item: {e}"))?
        .ok_or_else(|| "project checklist item not found".to_string())
}

pub(super) async fn tag_for_task_tag_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    tag_id: &str,
) -> Result<String, String> {
    sqlx::query_scalar(
        "SELECT pl.name
         FROM project_tags pl
         JOIN project_tasks pt ON pt.project_id = pl.project_id
         WHERE pt.id = ? AND pl.id = ?",
    )
    .bind(task_id.trim())
    .bind(tag_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load project task tag: {e}"))?
    .ok_or_else(|| "tag must belong to the task project".to_string())
}

/// Name of the linked tag, or `None` when the link is already gone.
pub(super) async fn tag_for_existing_task_tag_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    tag_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT pl.name
         FROM project_task_tag_links ptl
         JOIN project_tags pl ON pl.id = ptl.tag_id
         WHERE ptl.task_id = ? AND ptl.tag_id = ?",
    )
    .bind(task_id.trim())
    .bind(tag_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load project task tag link: {e}"))
}

pub(super) async fn delete_tag_with_history(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    tag_id: &str,
) -> Result<(), String> {
    let tag = sqlx::query_as::<_, ProjectTagRow>("SELECT * FROM project_tags WHERE id = ?")
        .bind(tag_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load project tag: {e}"))?
        .ok_or_else(|| "project tag not found".to_string())?;
    let linked_task_ids = sqlx::query_scalar::<_, String>(
        "SELECT task_id
         FROM project_task_tag_links
         WHERE tag_id = ?
         ORDER BY created_at ASC",
    )
    .bind(tag_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load project tag tasks: {e}"))?;
    sqlx::query("DELETE FROM project_tags WHERE id = ?")
        .bind(tag_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete project tag: {e}"))?;
    for task_id in linked_task_ids {
        insert_task_change_event(tx, &task_id, "updated", Some("tags"), Some(&tag.name), None)
            .await?;
    }
    Ok(())
}

pub(super) fn normalized_optional_identifier(value: Option<&str>) -> Option<String> {
    normalized_optional_text(value)
}

pub(super) fn normalized_optional_text(value: Option<&str>) -> Option<String> {
    value.and_then(|identifier| {
        let trimmed = identifier.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}
