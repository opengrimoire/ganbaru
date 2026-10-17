use super::models::{ProjectTaskRow, ProjectTaskUpdate};

pub(crate) async fn status_is_terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
) -> Result<bool, String> {
    let terminal: i64 = sqlx::query_scalar("SELECT terminal FROM project_statuses WHERE id = ?")
        .bind(status_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("load project status: {e}"))?;
    Ok(terminal != 0)
}

pub(crate) async fn current_timestamp(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<String, String> {
    sqlx::query_scalar::<_, String>("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("load timestamp: {e}"))
}

async fn label_for_project_status(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    status_id: &str,
) -> Result<String, String> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT name FROM project_statuses WHERE id = ?")
            .bind(status_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load project status label: {e}"))?
            .unwrap_or_else(|| status_id.to_string()),
    )
}

async fn label_for_project_section(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    section_id: &str,
) -> Result<String, String> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT name FROM project_sections WHERE id = ?")
            .bind(section_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load project section label: {e}"))?
            .unwrap_or_else(|| section_id.to_string()),
    )
}

async fn label_for_project_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<String, String> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT title FROM project_tasks WHERE id = ?")
            .bind(task_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load project task label: {e}"))?
            .unwrap_or_else(|| task_id.to_string()),
    )
}

async fn optional_task_label(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: Option<&str>,
) -> Result<Option<String>, String> {
    match task_id {
        Some(value) => Ok(Some(label_for_project_task(tx, value).await?)),
        None => Ok(None),
    }
}

pub(crate) async fn insert_task_field_change_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    event_type: &str,
    field_name: &str,
    old_value: Option<String>,
    new_value: Option<String>,
    reason: Option<&str>,
) -> Result<(), String> {
    insert_task_change_event_with_reason(
        tx,
        task_id,
        event_type,
        Some(field_name),
        old_value.as_deref(),
        new_value.as_deref(),
        reason,
    )
    .await
}

fn optional_i64_to_string(value: Option<i64>) -> Option<String> {
    value.map(|number| number.to_string())
}

pub(crate) fn bool_to_string(value: bool) -> String {
    if value {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn normalized_task_change_reason(value: Option<&str>) -> Option<String> {
    value.and_then(|reason| {
        let trimmed = reason.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub(crate) async fn insert_task_update_change_events(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    previous: &ProjectTaskRow,
    next: &ProjectTaskUpdate,
) -> Result<(), String> {
    let task_id = next.id.as_str();
    let next_title = next.title.trim().to_string();
    let change_reason = normalized_task_change_reason(next.change_reason.as_deref());
    if previous.title != next_title {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "title",
            Some(previous.title.clone()),
            Some(next_title),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.description != next.description {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "description",
            Some(previous.description.clone()),
            Some(next.description.clone()),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.status_id != next.status_id {
        let previous_terminal = status_is_terminal(tx, &previous.status_id).await?;
        let next_terminal = status_is_terminal(tx, &next.status_id).await?;
        let event_type = if !previous_terminal && next_terminal {
            "completed"
        } else if previous_terminal && !next_terminal {
            "reopened"
        } else {
            "status_changed"
        };
        let old_status = label_for_project_status(tx, &previous.status_id).await?;
        let new_status = label_for_project_status(tx, &next.status_id).await?;
        insert_task_field_change_event(
            tx,
            task_id,
            event_type,
            "status",
            Some(old_status),
            Some(new_status),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.section_id != next.section_id {
        let old_section = label_for_project_section(tx, &previous.section_id).await?;
        let new_section = label_for_project_section(tx, &next.section_id).await?;
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "section",
            Some(old_section),
            Some(new_section),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.parent_task_id != next.parent_task_id {
        let old_parent = optional_task_label(tx, previous.parent_task_id.as_deref()).await?;
        let new_parent = optional_task_label(tx, next.parent_task_id.as_deref()).await?;
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "parent",
            old_parent,
            new_parent,
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.priority != next.priority {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "priority",
            Some(previous.priority.clone()),
            Some(next.priority.clone()),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.task_type != next.task_type {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "type",
            Some(previous.task_type.clone()),
            Some(next.task_type.clone()),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.estimate_minutes != next.estimate_minutes {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "estimate",
            optional_i64_to_string(previous.estimate_minutes),
            optional_i64_to_string(next.estimate_minutes),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.due_date != next.due_date {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "due_date",
            previous.due_date.clone(),
            next.due_date.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.due_time != next.due_time {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "due_time",
            previous.due_time.clone(),
            next.due_time.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.start_date != next.start_date {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "start_date",
            previous.start_date.clone(),
            next.start_date.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.start_time != next.start_time {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "start_time",
            previous.start_time.clone(),
            next.start_time.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.target_end_date != next.target_end_date {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "target_date",
            previous.target_end_date.clone(),
            next.target_end_date.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.archived_at != next.archived_at {
        let event_type = if previous.archived_at.is_none() && next.archived_at.is_some() {
            "archived"
        } else {
            "updated"
        };
        insert_task_field_change_event(
            tx,
            task_id,
            event_type,
            "archived_at",
            previous.archived_at.clone(),
            next.archived_at.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if previous.blocker_reason != next.blocker_reason {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "blocker_reason",
            previous.blocker_reason.clone(),
            next.blocker_reason.clone(),
            change_reason.as_deref(),
        )
        .await?;
    }
    if (previous.milestone != 0) != next.milestone {
        insert_task_field_change_event(
            tx,
            task_id,
            "updated",
            "milestone",
            Some(bool_to_string(previous.milestone != 0)),
            Some(bool_to_string(next.milestone)),
            change_reason.as_deref(),
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn insert_task_change_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    event_type: &str,
    field_name: Option<&str>,
    old_value: Option<&str>,
    new_value: Option<&str>,
) -> Result<(), String> {
    insert_task_change_event_with_reason(
        tx, task_id, event_type, field_name, old_value, new_value, None,
    )
    .await
}

async fn insert_task_change_event_with_reason(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    event_type: &str,
    field_name: Option<&str>,
    old_value: Option<&str>,
    new_value: Option<&str>,
    reason: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO project_task_change_events
            (id, task_id, event_type, field_name, old_value, new_value, reason)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(event_type)
    .bind(field_name)
    .bind(old_value)
    .bind(new_value)
    .bind(reason)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert project task change event: {e}"))?;
    Ok(())
}
