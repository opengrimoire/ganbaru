use super::keys::priority_key;
use super::order::rank_key;
use super::views::insert_default_views;

/// Default statuses in order: id slug, name, category, color, and terminal flag.
const DEFAULT_STATUSES: &[(&str, &str, &str, i64, i64)] = &[
    ("backlog", "Backlog", "not_started", 30, 0),
    ("todo", "To do", "not_started", 31, 0),
    ("in-progress", "In progress", "active", 19, 0),
    ("in-review", "In review", "active", 23, 0),
    ("blocked", "Blocked", "blocked", 2, 0),
    ("done", "Done", "done", 13, 1),
];

/// Default priorities in order: id, name, and color.
const DEFAULT_PRIORITIES: &[(&str, &str, i64)] = &[
    ("urgent", "Urgent", 2),
    ("high", "High", 7),
    ("normal", "Normal", 19),
    ("low", "Low", 30),
];

/// Inserts the General section, default statuses, priorities, and views that a new project starts with.
pub(super) async fn insert_default_project_graph(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO project_sections (id, project_id, name, order_key) VALUES (?, ?, ?, ?)",
    )
    .bind(format!("section-{project_id}-general"))
    .bind(project_id)
    .bind("General")
    .bind(rank_key(0)?.as_str())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("create default project section: {e}"))?;

    for (index, (slug, name, category, color, terminal)) in DEFAULT_STATUSES.iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_statuses (id, project_id, name, category, color, order_key, terminal)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(format!("status-{project_id}-{slug}"))
        .bind(project_id)
        .bind(name)
        .bind(category)
        .bind(color)
        .bind(rank_key(index)?.as_str())
        .bind(terminal)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project status: {e}"))?;
    }

    for (index, (id, name, color)) in DEFAULT_PRIORITIES.iter().enumerate() {
        sqlx::query(
            "INSERT INTO project_priorities (id, project_id, name, color, order_key, sync_key)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(project_id)
        .bind(name)
        .bind(color)
        .bind(rank_key(index)?.as_str())
        .bind(priority_key(project_id, id)?)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project priority: {e}"))?;
    }

    insert_default_views(tx, project_id).await
}
