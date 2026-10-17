use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

use super::history::insert_task_change_event;
use super::models::{
    ProjectLinkableEvent, ProjectLinkableEventRow, ProjectLinkableEventSearch,
    ProjectLinkableEventTask, ProjectLinkableEventTaskRow, ProjectMutationRemoval,
    ProjectTaskDependencyCreate, ProjectTaskDependencyRow, ProjectTaskEventLinkCreate,
    ProjectTaskTagLinkCreate, ProjectsMutationRows,
};
use super::mutations::{
    dependency_mutation, ensure_dependency_has_no_cycle, ensure_dependency_tasks_match_project,
    ensure_project_exists_in_pool, ensure_task_belongs_to_project_in_pool, event_link_mutation,
    latest_task_change_events, tag_for_existing_task_tag_link, tag_for_task_tag_link,
    task_tag_link_mutation,
};
use super::validation::{
    normalize_optional_date_filter, require_non_empty, sql_like_contains_pattern,
    validate_task_dependency_create, validate_task_event_link_create,
    validate_task_tag_link_create,
};

pub async fn create_task_dependency(
    pool: &SqlitePool,
    dependency: ProjectTaskDependencyCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_task_dependency_create(&dependency)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    ensure_dependency_tasks_match_project(
        &mut tx,
        &dependency.blocking_task_id,
        &dependency.blocked_task_id,
    )
    .await?;
    ensure_dependency_has_no_cycle(
        &mut tx,
        &dependency.blocking_task_id,
        &dependency.blocked_task_id,
    )
    .await?;
    sqlx::query(
        "INSERT INTO project_task_dependencies
            (id, blocking_task_id, blocked_task_id, dependency_type)
         VALUES (?, ?, ?, ?)",
    )
    .bind(dependency.id.trim())
    .bind(&dependency.blocking_task_id)
    .bind(&dependency.blocked_task_id)
    .bind(&dependency.dependency_type)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create project task dependency: {e}"))?;
    insert_task_change_event(
        &mut tx,
        &dependency.blocked_task_id,
        "dependency_added",
        Some("blocking_task_id"),
        None,
        Some(&dependency.blocking_task_id),
    )
    .await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    dependency_mutation(pool, dependency.id.trim(), &dependency.blocked_task_id).await
}

pub async fn delete_task_dependency(
    pool: &SqlitePool,
    dependency_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&dependency_id, "dependency_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let row = sqlx::query_as::<_, ProjectTaskDependencyRow>(
        "SELECT * FROM project_task_dependencies WHERE id = ?",
    )
    .bind(dependency_id.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("load project task dependency: {e}"))?
    .ok_or_else(|| "project task dependency not found".to_string())?;
    sqlx::query("DELETE FROM project_task_dependencies WHERE id = ?")
        .bind(dependency_id.trim())
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("delete project task dependency: {e}"))?;
    insert_task_change_event(
        &mut tx,
        &row.blocked_task_id,
        "dependency_removed",
        Some("blocking_task_id"),
        Some(&row.blocking_task_id),
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows {
        task_change_events: latest_task_change_events(pool, &row.blocked_task_id).await?,
        ..ProjectsMutationRows::default()
    };
    mutation.removals.push(ProjectMutationRemoval::Dependency {
        id: dependency_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn link_task_tag(
    pool: &SqlitePool,
    link: ProjectTaskTagLinkCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_task_tag_link_create(&link)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let tag_name = tag_for_task_tag_link(&mut tx, &link.task_id, &link.tag_id).await?;
    let result = sqlx::query(
        "INSERT INTO project_task_tag_links (task_id, tag_id)
         VALUES (?, ?)
         ON CONFLICT(task_id, tag_id) DO NOTHING",
    )
    .bind(link.task_id.trim())
    .bind(link.tag_id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("link project task tag: {e}"))?;
    if result.rows_affected() > 0 {
        insert_task_change_event(
            &mut tx,
            link.task_id.trim(),
            "updated",
            Some("tags"),
            None,
            Some(&tag_name),
        )
        .await?;
    }
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    task_tag_link_mutation(pool, link.task_id.trim(), link.tag_id.trim()).await
}

pub async fn unlink_task_tag(
    pool: &SqlitePool,
    task_id: String,
    tag_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&task_id, "task_id")?;
    require_non_empty(&tag_id, "tag_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let tag_name = tag_for_existing_task_tag_link(&mut tx, task_id.trim(), tag_id.trim()).await?;
    sqlx::query(
        "DELETE FROM project_task_tag_links
         WHERE task_id = ? AND tag_id = ?",
    )
    .bind(task_id.trim())
    .bind(tag_id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("unlink project task tag: {e}"))?;
    insert_task_change_event(
        &mut tx,
        task_id.trim(),
        "updated",
        Some("tags"),
        Some(&tag_name),
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows {
        task_change_events: latest_task_change_events(pool, task_id.trim()).await?,
        ..ProjectsMutationRows::default()
    };
    mutation.removals.push(ProjectMutationRemoval::TaskTagLink {
        task_id: task_id.trim().to_string(),
        tag_id: tag_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn link_task_event(
    pool: &SqlitePool,
    link: ProjectTaskEventLinkCreate,
) -> Result<ProjectsMutationRows, String> {
    validate_task_event_link_create(&link)?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    link_task_event_with_project_assignment(&mut tx, &link).await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    event_link_mutation(pool, link.task_id.trim(), link.event_id.trim()).await
}

pub(super) async fn link_task_event_with_project_assignment(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    link: &ProjectTaskEventLinkCreate,
) -> Result<(), String> {
    let task_project_id: String =
        sqlx::query_scalar("SELECT project_id FROM project_tasks WHERE id = ?")
            .bind(&link.task_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load linked project task: {e}"))?
            .ok_or_else(|| "project task not found".to_string())?;
    let event_project_id: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM calendar_events WHERE id = ?")
            .bind(&link.event_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load linked calendar event: {e}"))?
            .ok_or_else(|| "calendar event not found".to_string())?;
    match event_project_id {
        Some(event_project_id) if event_project_id != task_project_id => {
            return Err("calendar event does not belong to the task project".to_string());
        }
        Some(_) => {}
        None => {
            sqlx::query("UPDATE calendar_events SET project_id = ?, updated_at = datetime('now') WHERE id = ?")
                .bind(&task_project_id)
                .bind(&link.event_id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("assign calendar event project: {e}"))?;
        }
    }
    sqlx::query(
        "INSERT INTO project_task_event_links (task_id, event_id, link_kind)
         VALUES (?, ?, ?)
         ON CONFLICT(task_id, event_id) DO UPDATE SET link_kind = excluded.link_kind",
    )
    .bind(&link.task_id)
    .bind(&link.event_id)
    .bind(&link.link_kind)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("link project task event: {e}"))?;
    insert_task_change_event(
        tx,
        &link.task_id,
        "scheduled",
        Some("event_id"),
        None,
        Some(&link.event_id),
    )
    .await?;
    Ok(())
}

pub async fn unlink_task_event(
    pool: &SqlitePool,
    task_id: String,
    event_id: String,
) -> Result<ProjectsMutationRows, String> {
    require_non_empty(&task_id, "task_id")?;
    require_non_empty(&event_id, "event_id")?;
    let mut tx = pool.begin().await.map_err(|e| format!("begin: {e}"))?;
    let existing_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM project_task_event_links
         WHERE task_id = ? AND event_id = ?",
    )
    .bind(task_id.trim())
    .bind(event_id.trim())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("load project task event link: {e}"))?;
    if existing_count == 0 {
        return Err("project task event link not found".to_string());
    }
    sqlx::query(
        "DELETE FROM project_task_event_links
         WHERE task_id = ? AND event_id = ?",
    )
    .bind(task_id.trim())
    .bind(event_id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("unlink project task event: {e}"))?;
    insert_task_change_event(
        &mut tx,
        task_id.trim(),
        "event_unlinked",
        Some("event_id"),
        Some(event_id.trim()),
        None,
    )
    .await?;
    tx.commit().await.map_err(|e| format!("commit: {e}"))?;
    let mut mutation = ProjectsMutationRows {
        task_change_events: latest_task_change_events(pool, task_id.trim()).await?,
        ..ProjectsMutationRows::default()
    };
    mutation.removals.push(ProjectMutationRemoval::EventLink {
        task_id: task_id.trim().to_string(),
        event_id: event_id.trim().to_string(),
    });
    Ok(mutation)
}

pub async fn search_linkable_events(
    pool: &SqlitePool,
    search: ProjectLinkableEventSearch,
) -> Result<Vec<ProjectLinkableEvent>, String> {
    require_non_empty(&search.project_id, "project_id")?;
    let normalized_project_id = search.project_id.trim().to_string();
    let normalized_task_id = search.task_id.unwrap_or_default().trim().to_string();
    let result_limit = search.limit.unwrap_or(12).clamp(1, 50);
    let normalized_query = search.query.unwrap_or_default().trim().to_lowercase();
    let normalized_start_date = normalize_optional_date_filter(search.start_date, "start_date")?;
    let normalized_end_date = normalize_optional_date_filter(search.end_date, "end_date")?;
    if let (Some(start), Some(end)) = (&normalized_start_date, &normalized_end_date) {
        if start > end {
            return Err("start_date must be before end_date".to_string());
        }
    }
    let like_query = sql_like_contains_pattern(&normalized_query);
    ensure_project_exists_in_pool(pool, &normalized_project_id).await?;
    if !normalized_task_id.is_empty() {
        ensure_task_belongs_to_project_in_pool(pool, &normalized_task_id, &normalized_project_id)
            .await?;
    }

    let mut event_rows = sqlx::query_as::<_, ProjectLinkableEventRow>(
        "SELECT id,
                project_id,
                title,
                start_time,
                end_time,
                timezone,
                calendar_id,
                color,
                all_day,
                status
         FROM calendar_events
         WHERE project_id = ?
           AND status != 'cancelled'
           AND (
             ? = ''
             OR lower(title) LIKE ? ESCAPE '\\'
             OR lower(start_time) LIKE ? ESCAPE '\\'
             OR lower(end_time) LIKE ? ESCAPE '\\'
           )
           AND (? IS NULL OR substr(start_time, 1, 10) >= ?)
           AND (? IS NULL OR substr(start_time, 1, 10) <= ?)
         ORDER BY
           CASE WHEN start_time >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now') THEN 0 ELSE 1 END,
           CASE WHEN start_time >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now') THEN start_time END ASC,
           start_time DESC
         LIMIT ?",
    )
    .bind(&normalized_project_id)
    .bind(&normalized_query)
    .bind(&like_query)
    .bind(&like_query)
    .bind(&like_query)
    .bind(&normalized_start_date)
    .bind(&normalized_start_date)
    .bind(&normalized_end_date)
    .bind(&normalized_end_date)
    .bind(result_limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("search project linkable events: {e}"))?;
    if !normalized_task_id.is_empty() {
        let linked_event_rows = sqlx::query_as::<_, ProjectLinkableEventRow>(
            "SELECT ce.id,
                    ce.project_id,
                    ce.title,
                    ce.start_time,
                    ce.end_time,
                    ce.timezone,
                    ce.calendar_id,
                    ce.color,
                    ce.all_day,
                    ce.status
             FROM calendar_events ce
             JOIN project_task_event_links ptel ON ptel.event_id = ce.id
             WHERE ce.project_id = ?
               AND ptel.task_id = ?
               AND ce.status != 'cancelled'
             ORDER BY ce.start_time DESC",
        )
        .bind(&normalized_project_id)
        .bind(&normalized_task_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load linked project events: {e}"))?;
        let mut seen_event_ids: HashSet<String> =
            event_rows.iter().map(|event| event.id.clone()).collect();
        for row in linked_event_rows {
            if seen_event_ids.insert(row.id.clone()) {
                event_rows.push(row);
            }
        }
    }

    let event_ids: HashSet<String> = event_rows.iter().map(|event| event.id.clone()).collect();
    if event_ids.is_empty() {
        return Ok(Vec::new());
    }

    let task_link_rows = sqlx::query_as::<_, ProjectLinkableEventTaskRow>(
        "SELECT ptel.event_id,
                pt.id AS task_id,
                pt.title,
                pt.archived_at
         FROM project_task_event_links ptel
         JOIN project_tasks pt ON pt.id = ptel.task_id
         WHERE pt.project_id = ?
         ORDER BY ptel.created_at DESC",
    )
    .bind(&normalized_project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project event task links: {e}"))?;
    let mut tasks_by_event_id: HashMap<String, Vec<ProjectLinkableEventTask>> = HashMap::new();
    for row in task_link_rows {
        if !event_ids.contains(&row.event_id) {
            continue;
        }
        tasks_by_event_id
            .entry(row.event_id)
            .or_default()
            .push(ProjectLinkableEventTask {
                task_id: row.task_id,
                title: row.title,
                archived_at: row.archived_at,
            });
    }

    Ok(event_rows
        .into_iter()
        .map(|row| ProjectLinkableEvent {
            linked_tasks: tasks_by_event_id.remove(&row.id).unwrap_or_default(),
            id: row.id,
            project_id: row.project_id,
            title: row.title,
            start_time: row.start_time,
            end_time: row.end_time,
            timezone: row.timezone,
            calendar_id: row.calendar_id,
            color: row.color,
            all_day: row.all_day,
            status: row.status,
        })
        .collect())
}
