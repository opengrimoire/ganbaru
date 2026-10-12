use super::history::insert_task_change_event;
use super::models::{
    CalendarEventProjectAssignment, ProjectChecklistItemRow, ProjectCustomEmojiRow,
    ProjectCustomFieldOptionRow, ProjectCustomFieldRow, ProjectGroupRow, ProjectPriorityRow,
    ProjectRow, ProjectSectionRow, ProjectStatusRow, ProjectTagRow, ProjectTaskChangeEventRow,
    ProjectTaskDependencyRow, ProjectTaskEventLinkRow, ProjectTaskRow, ProjectTaskTagLinkRow,
    ProjectViewPreferenceRow, ProjectsMutationRows,
};

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
define_mutation_loader!(
    priority_mutation,
    priorities,
    ProjectPriorityRow,
    "project_priorities"
);
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
    project_id: &str,
    view_id: &str,
    preference_key: &str,
) -> Result<ProjectsMutationRows, String> {
    let preference = sqlx::query_as::<_, ProjectViewPreferenceRow>(
        "SELECT * FROM project_view_preferences
         WHERE project_id = ? AND view_id = ? AND preference_key = ?",
    )
    .bind(project_id)
    .bind(view_id)
    .bind(preference_key)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("load view preference mutation result: {e}"))?;
    let mut mutation = ProjectsMutationRows::default();
    mutation.view_preferences.push(preference);
    Ok(mutation)
}

pub(super) async fn delete_unused_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
) -> Result<(), String> {
    let project_id =
        sqlx::query_scalar::<_, String>("SELECT project_id FROM project_statuses WHERE id = ?")
            .bind(status_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load project status: {e}"))?
            .ok_or_else(|| "project status not found".to_string())?;

    let status_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_statuses WHERE project_id = ?")
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

    let result = sqlx::query("DELETE FROM project_statuses WHERE id = ?")
        .bind(status_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete project status: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project status not found".to_string());
    }
    Ok(())
}

pub(super) async fn delete_unused_priority(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    priority_id: &str,
) -> Result<(), String> {
    let priority_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM project_priorities WHERE project_id = ? AND id = ?",
    )
    .bind(project_id)
    .bind(priority_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load project priority: {e}"))?;
    if priority_exists == 0 {
        return Err("project priority not found".to_string());
    }

    let priority_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM project_priorities WHERE project_id = ?")
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

    let result = sqlx::query("DELETE FROM project_priorities WHERE project_id = ? AND id = ?")
        .bind(project_id)
        .bind(priority_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("delete project priority: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("project priority not found".to_string());
    }
    Ok(())
}

pub(super) async fn next_task_sort_order(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    owner_column: &'static str,
    project_id: &str,
    owner_id: &str,
    sort_column: &'static str,
) -> Result<f64, String> {
    let sql = format!(
        "SELECT CAST(COALESCE(MAX({sort_column}), 0) + 1000 AS REAL)
         FROM project_tasks
         WHERE project_id = ? AND {owner_column} = ?"
    );
    sqlx::query_scalar::<_, f64>(&sql)
        .bind(project_id)
        .bind(owner_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("load next task sort order: {e}"))
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
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    parent_task_id: &str,
) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
             FROM project_tasks
             WHERE id = ? AND project_id = ? AND parent_task_id IS NULL",
    )
    .bind(parent_task_id)
    .bind(project_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check parent project task: {e}"))?;
    if count == 0 {
        return Err("parent task must be a top-level task in the same project".to_string());
    }
    Ok(())
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
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("check project task dependency cycle: {e}"))?;
    if cycle_count > 0 {
        return Err("dependency would create a cycle".to_string());
    }
    Ok(())
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

pub(super) async fn tag_for_existing_task_tag_link(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    tag_id: &str,
) -> Result<String, String> {
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
    .map_err(|e| format!("load project task tag link: {e}"))?
    .ok_or_else(|| "project task tag link not found".to_string())
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
