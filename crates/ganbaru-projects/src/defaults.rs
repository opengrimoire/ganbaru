use super::views::insert_default_views;

const DEFAULT_STATUSES: &[(&str, &str, &str, i64, i64, i64)] = &[
    ("backlog", "Backlog", "not_started", 30, 0, 0),
    ("todo", "To do", "not_started", 31, 10, 0),
    ("in-progress", "In progress", "active", 19, 20, 0),
    ("in-review", "In review", "active", 23, 30, 0),
    ("blocked", "Blocked", "blocked", 2, 40, 0),
    ("done", "Done", "done", 13, 50, 1),
];

const DEFAULT_PRIORITIES: &[(&str, &str, i64, i64)] = &[
    ("urgent", "Urgent", 2, 0),
    ("high", "High", 7, 10),
    ("normal", "Normal", 19, 20),
    ("low", "Low", 30, 30),
];

/// Inserts the General section, default statuses, priorities, and views that every project starts with.
pub(super) async fn insert_default_project_graphs(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_ids: &[&str],
) -> Result<(), String> {
    if project_ids.is_empty() {
        return Ok(());
    }

    let mut sections = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "INSERT INTO project_sections (id, project_id, name, sort_order) ",
    );
    sections.push_values(project_ids, |mut row, project_id| {
        row.push_bind(format!("section-{project_id}-general"))
            .push_bind(project_id)
            .push_bind("General")
            .push_bind(0_i64);
    });
    sections
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project sections: {e}"))?;

    let status_rows = project_ids.iter().flat_map(|project_id| {
        DEFAULT_STATUSES
            .iter()
            .map(move |status| (*project_id, status))
    });
    let mut statuses = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "INSERT INTO project_statuses (id, project_id, name, category, color, sort_order, terminal) ",
    );
    statuses.push_values(status_rows, |mut row, (project_id, status)| {
        let (slug, name, category, color, sort_order, terminal) = *status;
        row.push_bind(format!("status-{project_id}-{slug}"))
            .push_bind(project_id)
            .push_bind(name)
            .push_bind(category)
            .push_bind(color)
            .push_bind(sort_order)
            .push_bind(terminal);
    });
    statuses
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project statuses: {e}"))?;

    let priority_rows = project_ids.iter().flat_map(|project_id| {
        DEFAULT_PRIORITIES
            .iter()
            .map(move |priority| (*project_id, priority))
    });
    let mut priorities = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "INSERT INTO project_priorities (id, project_id, name, color, sort_order) ",
    );
    priorities.push_values(priority_rows, |mut row, (project_id, priority)| {
        let (id, name, color, sort_order) = *priority;
        row.push_bind(id)
            .push_bind(project_id)
            .push_bind(name)
            .push_bind(color)
            .push_bind(sort_order);
    });
    priorities
        .build()
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("create default project priorities: {e}"))?;

    insert_default_views(tx, project_ids).await
}
