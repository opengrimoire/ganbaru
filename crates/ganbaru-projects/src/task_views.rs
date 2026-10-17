use super::models::*;
use super::mutations::ensure_project_exists_in_pool;
use super::validation::{require_non_empty, sql_like_contains_pattern, validate_enum};

const LIST_PAGE_SIZE_MAX: i64 = 100;
const KANBAN_COLUMN_PAGE_SIZE_MAX: i64 = 50;
const DASHBOARD_SAMPLE_SIZE: i64 = 30;
const GANTT_LAYOUT_TASK_LIMIT: i64 = 10_000;

const SUMMARY_SELECT: &str =
    "t.id, t.project_id, t.section_id, t.status_id, t.parent_task_id, t.title,
     t.priority, t.task_type, t.section_sort_order, t.status_sort_order,
     t.estimate_minutes, t.due_date, t.due_time, t.start_date, t.start_time,
     t.target_end_date, t.completed_at, t.archived_at,
     CASE WHEN TRIM(COALESCE(t.blocker_reason, '')) <> '' THEN 1 ELSE 0 END AS blocker_reason_present,
     t.milestone, t.created_at, t.updated_at, t.revision";
const SUMMARY_COLUMNS: &str =
    "t.id, t.project_id, t.section_id, t.status_id, t.parent_task_id, t.title,
     t.priority, t.task_type, t.section_sort_order, t.status_sort_order,
     t.estimate_minutes, t.due_date, t.due_time, t.start_date, t.start_time,
     t.target_end_date, t.completed_at, t.archived_at, t.blocker_reason_present,
     t.milestone, t.created_at, t.updated_at, t.revision";

fn validate_task_view_request(request: &ProjectTaskViewRequest) -> Result<(), String> {
    require_non_empty(request.project_id.trim(), "project_id")?;
    if request.page_size <= 0 {
        return Err("page_size must be positive".to_string());
    }
    validate_enum(
        request.status_filter.as_str(),
        "status_filter",
        &["all", "open", "blocked", "done"],
    )?;
    validate_enum(
        request.due_filter.as_str(),
        "due_filter",
        &["all", "overdue", "today", "week", "none", "range"],
    )?;
    validate_enum(
        request.schedule_filter.as_str(),
        "schedule_filter",
        &["all", "scheduled", "unscheduled"],
    )?;
    validate_enum(
        request.dependency_filter.as_str(),
        "dependency_filter",
        &["all", "linked", "blocked_by", "blocking", "none"],
    )?;
    validate_enum(
        request.sort_direction.as_str(),
        "sort_direction",
        &["asc", "desc"],
    )?;
    let core_sort = [
        "manual",
        "title",
        "start",
        "status",
        "section",
        "priority",
        "due",
        "scheduled",
        "created",
        "updated",
        "estimate",
    ];
    if !core_sort.contains(&request.sort_mode.as_str()) {
        let Some(field_id) = request.sort_mode.strip_prefix("custom:") else {
            return Err("sort_mode is invalid".to_string());
        };
        require_non_empty(field_id, "custom sort field id")?;
    }
    if request.visible_section_ids.len() > 1_000 {
        return Err("visible_section_ids exceeds 1000 entries".to_string());
    }
    if request.custom_field_filters.len() > 64 {
        return Err("custom_field_filters exceeds 64 entries".to_string());
    }
    if request.search.len() > 500 {
        return Err("search exceeds 500 characters".to_string());
    }
    for filter in &request.custom_field_filters {
        require_non_empty(filter.field_id.trim(), "custom field filter field_id")?;
        validate_enum(
            filter.mode.as_str(),
            "custom field filter mode",
            &["filled", "empty", "checkbox", "option"],
        )?;
        if filter.mode == "checkbox" && filter.checked.is_none() {
            return Err("checkbox custom field filter requires checked".to_string());
        }
        if filter.mode == "option"
            && filter
                .option_id
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("option custom field filter requires option_id".to_string());
        }
    }
    if request.candidate_event_ids.len() > 2_000 {
        return Err("candidate_event_ids exceeds 2000 entries".to_string());
    }
    Ok(())
}

fn push_base_filters<'a>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    request: &'a ProjectTaskViewRequest,
    include_user_filters: bool,
    exact_status_id: Option<&'a str>,
) {
    query
        .push(" t.project_id = ")
        .push_bind(&request.project_id);
    if !request.show_archived {
        query.push(" AND t.archived_at IS NULL");
    }
    if request.view == ProjectViewId::Gantt {
        query.push(
            " AND (t.start_date IS NOT NULL OR t.due_date IS NOT NULL
                    OR t.target_end_date IS NOT NULL OR t.milestone <> 0)",
        );
    }
    if !request.visible_section_ids.is_empty() {
        query.push(" AND t.section_id IN (");
        let mut separated = query.separated(", ");
        for section_id in &request.visible_section_ids {
            separated.push_bind(section_id);
        }
        separated.push_unseparated(")");
    }
    if let Some(status_id) = exact_status_id {
        query.push(" AND t.status_id = ").push_bind(status_id);
    }
    if !include_user_filters {
        return;
    }
    let search = request.search.trim().to_lowercase();
    if !search.is_empty() {
        let pattern = sql_like_contains_pattern(&search);
        query
            .push(" AND (LOWER(t.title) LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\' OR LOWER(t.description) LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\')");
    }
    match request.status_filter.as_str() {
        "open" => query.push(" AND s.terminal = 0"),
        "done" => query.push(" AND s.terminal <> 0"),
        "blocked" => query.push(
            " AND (s.category = 'blocked'
               OR TRIM(COALESCE(t.blocker_reason, '')) <> ''
               OR EXISTS(SELECT 1 FROM project_task_dependencies d WHERE d.blocked_task_id = t.id))",
        ),
        _ => query,
    };
    if request.section_filter != "all" {
        query
            .push(" AND t.section_id = ")
            .push_bind(&request.section_filter);
    }
    if request.priority_filter != "all" {
        query
            .push(" AND t.priority = ")
            .push_bind(&request.priority_filter);
    }
    match request.due_filter.as_str() {
        "none" => {
            query.push(" AND t.due_date IS NULL");
        }
        "overdue" => {
            query
                .push(" AND t.due_date IS NOT NULL AND t.due_date < ")
                .push_bind(&request.today)
                .push(" AND s.terminal = 0");
        }
        "today" => {
            query.push(" AND t.due_date = ").push_bind(&request.today);
        }
        "week" => {
            query
                .push(" AND t.due_date >= ")
                .push_bind(&request.today)
                .push(" AND t.due_date <= ")
                .push_bind(&request.week_end);
        }
        "range" => {
            if !request.due_range_start.trim().is_empty() {
                query
                    .push(" AND t.due_date >= ")
                    .push_bind(&request.due_range_start);
            }
            if !request.due_range_end.trim().is_empty() {
                query
                    .push(" AND t.due_date <= ")
                    .push_bind(&request.due_range_end);
            }
        }
        _ => {}
    };
    match request.schedule_filter.as_str() {
        "scheduled" => query.push(
            " AND EXISTS(SELECT 1 FROM project_task_event_links el
                          WHERE el.task_id = t.id AND el.link_kind = 'scheduled')",
        ),
        "unscheduled" => query.push(
            " AND NOT EXISTS(SELECT 1 FROM project_task_event_links el
                              WHERE el.task_id = t.id AND el.link_kind = 'scheduled')",
        ),
        _ => query,
    };
    match request.dependency_filter.as_str() {
        "linked" => query.push(
            " AND EXISTS(SELECT 1 FROM project_task_dependencies d
                          WHERE d.blocking_task_id = t.id OR d.blocked_task_id = t.id)",
        ),
        "blocked_by" => query.push(
            " AND EXISTS(SELECT 1 FROM project_task_dependencies d WHERE d.blocked_task_id = t.id)",
        ),
        "blocking" => query.push(
            " AND EXISTS(SELECT 1 FROM project_task_dependencies d WHERE d.blocking_task_id = t.id)",
        ),
        "none" => query.push(
            " AND NOT EXISTS(SELECT 1 FROM project_task_dependencies d
                              WHERE d.blocking_task_id = t.id OR d.blocked_task_id = t.id)",
        ),
        _ => query,
    };
    if request.tag_filter == "none" {
        query.push(
            " AND NOT EXISTS(SELECT 1 FROM project_task_tag_links tl WHERE tl.task_id = t.id)",
        );
    } else if request.tag_filter != "all" {
        query
            .push(" AND EXISTS(SELECT 1 FROM project_task_tag_links tl WHERE tl.task_id = t.id AND tl.tag_id = ")
            .push_bind(&request.tag_filter)
            .push(")");
    }
    for filter in &request.custom_field_filters {
        match filter.mode.as_str() {
            "filled" => {
                query
                    .push(" AND (EXISTS(SELECT 1 FROM project_custom_field_values fv WHERE fv.task_id = t.id AND fv.field_id = ")
                    .push_bind(&filter.field_id)
                    .push(") OR EXISTS(SELECT 1 FROM project_custom_field_option_values ov WHERE ov.task_id = t.id AND ov.field_id = ")
                    .push_bind(&filter.field_id)
                    .push("))");
            }
            "empty" => {
                query
                    .push(" AND NOT EXISTS(SELECT 1 FROM project_custom_field_values fv WHERE fv.task_id = t.id AND fv.field_id = ")
                    .push_bind(&filter.field_id)
                    .push(") AND NOT EXISTS(SELECT 1 FROM project_custom_field_option_values ov WHERE ov.task_id = t.id AND ov.field_id = ")
                    .push_bind(&filter.field_id)
                    .push(")");
            }
            "checkbox" => {
                query
                    .push(" AND EXISTS(SELECT 1 FROM project_custom_field_values fv WHERE fv.task_id = t.id AND fv.field_id = ")
                    .push_bind(&filter.field_id)
                    .push(" AND fv.checkbox_value = ")
                    .push_bind(if filter.checked.unwrap_or(false) { 1_i64 } else { 0_i64 })
                    .push(")");
            }
            "option" => {
                if let Some(option_id) = filter.option_id.as_deref() {
                    query
                        .push(" AND EXISTS(SELECT 1 FROM project_custom_field_option_values ov WHERE ov.task_id = t.id AND ov.field_id = ")
                        .push_bind(&filter.field_id)
                        .push(" AND ov.option_id = ")
                        .push_bind(option_id)
                        .push(")");
                }
            }
            _ => {}
        }
    }
}

fn push_filtered_cte<'a>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    request: &'a ProjectTaskViewRequest,
    exact_status_id: Option<&'a str>,
) {
    query.push(
        "WITH matched AS (
           SELECT t.* FROM project_tasks t
           JOIN project_statuses s ON s.id = t.status_id
           WHERE",
    );
    push_base_filters(query, request, true, exact_status_id);
    query.push(")");
}

fn push_order<'a>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    request: &'a ProjectTaskViewRequest,
    kanban: bool,
) {
    let direction = if request.sort_direction == "desc" {
        " DESC"
    } else {
        " ASC"
    };
    if kanban || request.sort_mode == "manual" {
        let first = if kanban {
            "t.status_sort_order"
        } else {
            "t.section_sort_order"
        };
        let second = if kanban {
            "t.section_sort_order"
        } else {
            "t.status_sort_order"
        };
        query
            .push(first)
            .push(direction)
            .push(", ")
            .push(second)
            .push(direction)
            .push(", t.created_at")
            .push(direction)
            .push(", t.title")
            .push(direction)
            .push(", t.id ASC");
        return;
    }
    let expression = match request.sort_mode.as_str() {
        "title" => "LOWER(t.title)",
        "start" => "t.start_date",
        "status" => "(SELECT sort_order FROM project_statuses ps WHERE ps.id = t.status_id)",
        "section" => "t.section_sort_order",
        "priority" => {
            "(SELECT sort_order FROM project_priorities pp WHERE pp.project_id = t.project_id AND pp.id = t.priority)"
        }
        "due" => "t.due_date",
        "scheduled" => {
            "(SELECT MIN(ce.start_time) FROM project_task_event_links el JOIN calendar_events ce ON ce.id = el.event_id WHERE el.task_id = t.id AND el.link_kind = 'scheduled')"
        }
        "created" => "t.created_at",
        "updated" => "t.updated_at",
        "estimate" => "t.estimate_minutes",
        _ => "NULL",
    };
    if let Some(field_id) = request.sort_mode.strip_prefix("custom:") {
        query.push("(");
        push_custom_column_sort(query, field_id);
        query.push(") IS NULL ASC, ");
        push_custom_column_sort(query, field_id);
        query.push(direction);
    } else {
        query
            .push("(")
            .push(expression)
            .push(") IS NULL ASC, ")
            .push(expression)
            .push(direction);
        if request.sort_mode == "due" {
            query
                .push(", t.due_time IS NULL ASC, t.due_time")
                .push(direction);
        }
        if request.sort_mode == "start" {
            query
                .push(", t.start_time IS NULL ASC, t.start_time")
                .push(direction);
        }
    }
    if request.sort_mode == "status" {
        query.push(", t.status_sort_order ASC, t.section_sort_order ASC");
    } else {
        query.push(", t.section_sort_order ASC, t.status_sort_order ASC");
    }
    query.push(", t.created_at ASC, t.title ASC, t.id ASC");
}

/// Preserve numeric ordering and use option names when sorting a custom property.
fn push_custom_column_sort<'a>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Sqlite>,
    field_id: &'a str,
) {
    query.push("COALESCE((SELECT COALESCE(fv.number_value, fv.checkbox_value, NULLIF(fv.date_value, ''), NULLIF(LOWER(fv.text_value), '')) FROM project_custom_field_values fv WHERE fv.task_id = t.id AND fv.field_id = ")
        .push_bind(field_id)
        .push(" LIMIT 1), (SELECT GROUP_CONCAT(name, ', ') FROM (SELECT LOWER(o.name) AS name FROM project_custom_field_option_values ov JOIN project_custom_field_options o ON o.id = ov.option_id WHERE ov.task_id = t.id AND ov.field_id = ")
        .push_bind(field_id).push(" ORDER BY o.sort_order, o.id)))");
}

async fn count_tasks(
    pool: &sqlx::SqlitePool,
    request: &ProjectTaskViewRequest,
    include_user_filters: bool,
    exact_status_id: Option<&str>,
) -> Result<i64, String> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT COUNT(*) FROM project_tasks t
         JOIN project_statuses s ON s.id = t.status_id WHERE",
    );
    push_base_filters(&mut query, request, include_user_filters, exact_status_id);
    query
        .build_query_scalar::<i64>()
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count project task view: {e}"))
}

async fn load_summary_page(
    pool: &sqlx::SqlitePool,
    request: &ProjectTaskViewRequest,
    exact_status_id: Option<&str>,
    cursor: Option<&str>,
    page_size: i64,
    include_parent_context: bool,
) -> Result<Vec<ProjectTaskSummaryRow>, String> {
    if let Some(cursor_id) = cursor {
        let mut cursor_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
        push_filtered_cte(&mut cursor_query, request, exact_status_id);
        if include_parent_context {
            cursor_query.push(", visible AS (SELECT * FROM matched UNION SELECT p.* FROM project_tasks p JOIN matched c ON p.id = c.parent_task_id)");
        }
        cursor_query
            .push(" SELECT COUNT(*) FROM ")
            .push(if include_parent_context {
                "visible"
            } else {
                "matched"
            })
            .push(" WHERE id = ")
            .push_bind(cursor_id);
        let found = cursor_query
            .build_query_scalar::<i64>()
            .fetch_one(pool)
            .await
            .map_err(|e| format!("validate project task cursor: {e}"))?;
        if found == 0 {
            return Err("project task cursor is stale".to_string());
        }
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
    push_filtered_cte(&mut query, request, exact_status_id);
    if include_parent_context {
        query.push(", visible AS (SELECT * FROM matched UNION SELECT p.* FROM project_tasks p JOIN matched c ON p.id = c.parent_task_id)");
    }
    query.push(", ranked AS (SELECT ").push(SUMMARY_SELECT);
    query.push(", ROW_NUMBER() OVER (ORDER BY ");
    push_order(&mut query, request, exact_status_id.is_some());
    query
        .push(") AS row_number FROM ")
        .push(if include_parent_context { "visible" } else { "matched" })
        .push(" t) SELECT ")
        .push(SUMMARY_COLUMNS)
        .push(" FROM ranked t WHERE t.row_number > COALESCE((SELECT row_number FROM ranked WHERE id = ")
        .push_bind(cursor.unwrap_or("\0"))
        .push("), 0) ORDER BY t.row_number ASC LIMIT ")
        .push_bind(page_size + 1);
    query
        .build_query_as::<ProjectTaskSummaryRow>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project task view page: {e}"))
}

async fn load_rows_for_task_ids<T>(
    pool: &sqlx::SqlitePool,
    select_prefix: &str,
    task_column: &str,
    task_ids: &[String],
) -> Result<Vec<T>, String>
where
    for<'row> T: sqlx::FromRow<'row, sqlx::sqlite::SqliteRow> + Send + Unpin,
{
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(select_prefix);
    query.push(" WHERE ").push(task_column).push(" IN (");
    let mut separated = query.separated(", ");
    for task_id in task_ids {
        separated.push_bind(task_id);
    }
    separated.push_unseparated(")");
    query
        .build_query_as::<T>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project task window relationships: {e}"))
}

async fn attach_window_relationships(
    pool: &sqlx::SqlitePool,
    page: &mut ProjectTaskViewPage,
) -> Result<(), String> {
    let task_ids: Vec<String> = page.tasks.iter().map(|task| task.id.clone()).collect();
    page.task_tag_links = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_task_tag_links",
        "task_id",
        &task_ids,
    )
    .await?;
    page.custom_field_values = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_custom_field_values",
        "task_id",
        &task_ids,
    )
    .await?;
    page.custom_field_option_values = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_custom_field_option_values",
        "task_id",
        &task_ids,
    )
    .await?;
    if !task_ids.is_empty() {
        let mut dependency_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT * FROM project_task_dependencies WHERE blocking_task_id IN (",
        );
        {
            let mut separated = dependency_query.separated(", ");
            for task_id in &task_ids {
                separated.push_bind(task_id);
            }
            separated.push_unseparated(") OR blocked_task_id IN (");
        }
        {
            let mut separated = dependency_query.separated(", ");
            for task_id in &task_ids {
                separated.push_bind(task_id);
            }
            separated.push_unseparated(")");
        }
        page.dependencies = dependency_query
            .build_query_as::<ProjectTaskDependencyRow>()
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project task window dependencies: {e}"))?;
    }
    page.event_links = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_task_event_links",
        "task_id",
        &task_ids,
    )
    .await?;
    Ok(())
}

async fn dashboard_aggregates(
    pool: &sqlx::SqlitePool,
    request: &ProjectTaskViewRequest,
) -> Result<ProjectDashboardTaskAggregates, String> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
    push_filtered_cte(&mut query, request, None);
    query.push(
        " SELECT
            COUNT(*) AS total,
            COALESCE(SUM(CASE WHEN s.terminal <> 0 THEN 1 ELSE 0 END), 0) AS completed,
            COALESCE(SUM(CASE WHEN s.terminal = 0 THEN COALESCE(m.estimate_minutes, 0) ELSE 0 END), 0) AS open_estimate_minutes,
            COALESCE(SUM(CASE WHEN s.category = 'blocked' OR TRIM(COALESCE(m.blocker_reason, '')) <> '' OR EXISTS(SELECT 1 FROM project_task_dependencies d WHERE d.blocked_task_id = m.id) THEN 1 ELSE 0 END), 0) AS blocked,
            COALESCE(SUM(CASE WHEN s.terminal = 0 AND m.due_date < ? THEN 1 ELSE 0 END), 0) AS overdue,
            COALESCE(SUM(CASE WHEN s.terminal = 0 AND m.due_date IS NOT NULL AND NOT EXISTS(SELECT 1 FROM project_task_event_links el WHERE el.task_id = m.id AND el.link_kind = 'scheduled') THEN 1 ELSE 0 END), 0) AS unscheduled_due,
            COALESCE(SUM(CASE WHEN s.terminal = 0 AND m.estimate_minutes IS NULL THEN 1 ELSE 0 END), 0) AS missing_estimate
          FROM matched m JOIN project_statuses s ON s.id = m.status_id",
    );
    let row: (i64, i64, i64, i64, i64, i64, i64) = query
        .build_query_as()
        .bind(&request.today)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("load project dashboard aggregates: {e}"))?;
    let mut status_query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
    push_filtered_cte(&mut status_query, request, None);
    status_query.push(" SELECT status_id, COUNT(*) FROM matched GROUP BY status_id");
    let status_counts = status_query
        .build_query_as::<(String, i64)>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project dashboard status counts: {e}"))?
        .into_iter()
        .collect();
    Ok(ProjectDashboardTaskAggregates {
        total: row.0,
        completed: row.1,
        open_estimate_minutes: row.2,
        blocked: row.3,
        overdue: row.4,
        unscheduled_due: row.5,
        missing_estimate: row.6,
        status_counts,
    })
}

/// Complete-result counts and numeric reductions for built-in list columns.
#[derive(sqlx::FromRow)]
struct ListColumnAggregateRow {
    total: i64,
    name_filled: i64,
    start_filled: i64,
    due_filled: i64,
    estimate_filled: i64,
    estimate_sum: Option<f64>,
    estimate_average: Option<f64>,
    estimate_minimum: Option<f64>,
    estimate_maximum: Option<f64>,
    scheduled_filled: i64,
    dependencies_filled: i64,
}

/// Complete-result reductions for one custom field, including empty filtered results.
#[derive(sqlx::FromRow)]
struct CustomColumnAggregateRow {
    field_id: String,
    filled: i64,
    sum: Option<f64>,
    average: Option<f64>,
    minimum: Option<f64>,
    maximum: Option<f64>,
}

/// Compute footer reductions with the same filters as the query, before any cursor or task cap.
async fn list_column_calculations(
    pool: &sqlx::SqlitePool,
    request: &ProjectTaskViewRequest,
) -> Result<Vec<ProjectTaskColumnCalculations>, String> {
    let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
    push_filtered_cte(&mut query, request, None);
    query.push(" SELECT COUNT(*) AS total, COUNT(NULLIF(TRIM(title), '')) AS name_filled, COUNT(start_date) AS start_filled, COUNT(due_date) AS due_filled,
        COUNT(estimate_minutes) AS estimate_filled, CAST(SUM(estimate_minutes) AS REAL) AS estimate_sum, CAST(AVG(estimate_minutes) AS REAL) AS estimate_average, CAST(MIN(estimate_minutes) AS REAL) AS estimate_minimum, CAST(MAX(estimate_minutes) AS REAL) AS estimate_maximum,
        COALESCE(SUM(EXISTS(SELECT 1 FROM project_task_event_links el WHERE el.task_id = matched.id AND el.link_kind = 'scheduled')), 0) AS scheduled_filled,
        COALESCE(SUM(EXISTS(SELECT 1 FROM project_task_dependencies d WHERE d.blocking_task_id = matched.id OR d.blocked_task_id = matched.id)), 0) AS dependencies_filled
        FROM matched");
    let row = query
        .build_query_as::<ListColumnAggregateRow>()
        .fetch_one(pool)
        .await
        .map_err(|e| format!("calculate project columns: {e}"))?;
    let mut result = Vec::new();
    for (column, filled) in [
        ("name", row.name_filled),
        ("status", row.total),
        ("priority", row.total),
        ("start", row.start_filled),
        ("due", row.due_filled),
        ("estimate", row.estimate_filled),
        ("scheduled", row.scheduled_filled),
        ("dependencies", row.dependencies_filled),
        ("assignee", row.total),
        ("reviewer", row.total),
    ] {
        let numeric = column == "estimate";
        result.push(ProjectTaskColumnCalculations {
            column: column.to_string(),
            total: row.total,
            filled,
            sum: numeric.then_some(row.estimate_sum).flatten(),
            average: numeric.then_some(row.estimate_average).flatten(),
            minimum: numeric.then_some(row.estimate_minimum).flatten(),
            maximum: numeric.then_some(row.estimate_maximum).flatten(),
        });
    }
    let mut fields = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
    push_filtered_cte(&mut fields, request, None);
    fields.push(" SELECT f.id AS field_id,
        COUNT(DISTINCT CASE WHEN v.number_value IS NOT NULL OR v.checkbox_value IS NOT NULL
          OR NULLIF(TRIM(v.text_value), '') IS NOT NULL OR NULLIF(TRIM(v.date_value), '') IS NOT NULL
          OR EXISTS(SELECT 1 FROM project_custom_field_option_values ov WHERE ov.task_id = m.id AND ov.field_id = f.id)
          THEN m.id END) AS filled, CAST(SUM(v.number_value) AS REAL) AS sum, CAST(AVG(v.number_value) AS REAL) AS average, CAST(MIN(v.number_value) AS REAL) AS minimum, CAST(MAX(v.number_value) AS REAL) AS maximum
        FROM project_custom_fields f LEFT JOIN matched m ON 1 = 1
        LEFT JOIN project_custom_field_values v ON v.task_id = m.id AND v.field_id = f.id
        WHERE f.project_id = ").push_bind(&request.project_id).push(" GROUP BY f.id ORDER BY f.sort_order, f.id");
    let custom = fields
        .build_query_as::<CustomColumnAggregateRow>()
        .fetch_all(pool)
        .await
        .map_err(|e| format!("calculate project custom columns: {e}"))?;
    result.extend(
        custom
            .into_iter()
            .map(|custom| ProjectTaskColumnCalculations {
                column: format!("custom:{}", custom.field_id),
                total: row.total,
                filled: custom.filled,
                sum: custom.sum,
                average: custom.average,
                minimum: custom.minimum,
                maximum: custom.maximum,
            }),
    );
    Ok(result)
}

pub async fn load_task_view(
    pool: &sqlx::SqlitePool,
    request: ProjectTaskViewRequest,
) -> Result<ProjectTaskViewPage, String> {
    validate_task_view_request(&request)?;
    ensure_project_exists_in_pool(pool, request.project_id.trim()).await?;
    let total_count = count_tasks(pool, &request, false, None).await?;
    let matched_count = count_tasks(pool, &request, true, None).await?;
    let mut archived_request = request.clone();
    archived_request.view = ProjectViewId::List;
    archived_request.show_archived = true;
    let all_count = count_tasks(pool, &archived_request, false, None).await?;
    archived_request.show_archived = false;
    let active_count = count_tasks(pool, &archived_request, false, None).await?;
    let mut page = ProjectTaskViewPage {
        project_id: request.project_id.clone(),
        view: request.view,
        tasks: Vec::new(),
        total_count,
        matched_count,
        archived_count: all_count - active_count,
        next_cursor: None,
        column_counts: Vec::new(),
        aggregates: None,
        column_calculations: Vec::new(),
        matched_event_ids: Vec::new(),
        task_tag_links: Vec::new(),
        custom_field_values: Vec::new(),
        custom_field_option_values: Vec::new(),
        dependencies: Vec::new(),
        event_links: Vec::new(),
        tags: sqlx::query_as::<_, ProjectTagRow>(
            "SELECT * FROM project_tags WHERE project_id = ? ORDER BY sort_order, name",
        )
        .bind(&request.project_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project task view tags: {e}"))?,
        custom_fields: sqlx::query_as::<_, ProjectCustomFieldRow>(
            "SELECT * FROM project_custom_fields WHERE project_id = ? ORDER BY sort_order, name",
        )
        .bind(&request.project_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project task view custom fields: {e}"))?,
        custom_field_options: sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
            "SELECT o.* FROM project_custom_field_options o
             JOIN project_custom_fields f ON f.id = o.field_id
             WHERE f.project_id = ? ORDER BY o.field_id, o.sort_order, o.name",
        )
        .bind(&request.project_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load project task view custom field options: {e}"))?,
    };
    match request.view {
        ProjectViewId::Calendar => {
            if request.candidate_event_ids.is_empty() {
                return Ok(page);
            }
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new("");
            push_filtered_cte(&mut query, &request, None);
            query.push(
                " SELECT DISTINCT el.event_id FROM project_task_event_links el
                  JOIN matched m ON m.id = el.task_id WHERE el.event_id IN (",
            );
            let mut separated = query.separated(", ");
            for event_id in &request.candidate_event_ids {
                separated.push_bind(event_id);
            }
            separated.push_unseparated(")");
            page.matched_event_ids = query
                .build_query_scalar::<String>()
                .fetch_all(pool)
                .await
                .map_err(|e| format!("load project calendar task matches: {e}"))?;
            return Ok(page);
        }
        ProjectViewId::Kanban => {
            let status_ids = sqlx::query_scalar::<_, String>(
                "SELECT id FROM project_statuses WHERE project_id = ? ORDER BY sort_order, name",
            )
            .bind(&request.project_id)
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load project Kanban statuses: {e}"))?;
            let page_size = request.page_size.min(KANBAN_COLUMN_PAGE_SIZE_MAX);
            for status_id in status_ids {
                let count = count_tasks(pool, &request, true, Some(&status_id)).await?;
                let cursor = request.column_cursors.get(&status_id).map(String::as_str);
                let mut rows =
                    load_summary_page(pool, &request, Some(&status_id), cursor, page_size, false)
                        .await?;
                let next_cursor = if rows.len() as i64 > page_size {
                    rows.truncate(page_size as usize);
                    rows.last().map(|task| task.id.clone())
                } else {
                    None
                };
                page.column_counts.push(ProjectTaskColumnCount {
                    status_id,
                    count,
                    next_cursor,
                });
                page.tasks.extend(rows);
            }
        }
        ProjectViewId::Dashboard => {
            let mut rows =
                load_summary_page(pool, &request, None, None, DASHBOARD_SAMPLE_SIZE, false).await?;
            rows.truncate(DASHBOARD_SAMPLE_SIZE as usize);
            page.tasks = rows;
            page.aggregates = Some(dashboard_aggregates(pool, &request).await?);
        }
        ProjectViewId::Gantt => {
            let mut gantt_request = request.clone();
            gantt_request.page_size = GANTT_LAYOUT_TASK_LIMIT;
            let mut rows = load_summary_page(
                pool,
                &gantt_request,
                None,
                None,
                GANTT_LAYOUT_TASK_LIMIT,
                false,
            )
            .await?;
            rows.truncate(GANTT_LAYOUT_TASK_LIMIT as usize);
            page.tasks = rows;
        }
        ProjectViewId::List => {
            page.column_calculations = list_column_calculations(pool, &request).await?;
            let page_size = request.page_size.min(LIST_PAGE_SIZE_MAX);
            let mut rows = load_summary_page(
                pool,
                &request,
                None,
                request.cursor.as_deref(),
                page_size,
                true,
            )
            .await?;
            if rows.len() as i64 > page_size {
                rows.truncate(page_size as usize);
                page.next_cursor = rows.last().map(|task| task.id.clone());
            }
            page.tasks = rows;
        }
    }
    attach_window_relationships(pool, &mut page).await?;
    Ok(page)
}

pub async fn load_task_detail(
    pool: &sqlx::SqlitePool,
    task_id: &str,
) -> Result<ProjectTaskDetailData, String> {
    require_non_empty(task_id.trim(), "task_id")?;
    let task = sqlx::query_as::<_, ProjectTaskRow>("SELECT * FROM project_tasks WHERE id = ?")
        .bind(task_id.trim())
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load project task detail: {e}"))?
        .ok_or_else(|| "project task not found".to_string())?;
    let project_id = task.project_id.clone();
    let related_tasks = sqlx::query_as::<_, ProjectTaskSummaryRow>(&format!(
        "SELECT {SUMMARY_SELECT} FROM project_tasks t
         WHERE t.project_id = ? AND (
           t.id = ? OR t.parent_task_id = ? OR t.id = ?
           OR EXISTS(SELECT 1 FROM project_task_dependencies d
                     WHERE (d.blocking_task_id = ? AND d.blocked_task_id = t.id)
                        OR (d.blocked_task_id = ? AND d.blocking_task_id = t.id))
         ) ORDER BY t.section_sort_order, t.created_at LIMIT 200"
    ))
    .bind(&project_id)
    .bind(task_id.trim())
    .bind(task_id.trim())
    .bind(task.parent_task_id.as_deref().unwrap_or("\0"))
    .bind(task_id.trim())
    .bind(task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load related project task summaries: {e}"))?;
    let checklist_items = sqlx::query_as::<_, ProjectChecklistItemRow>(
        "SELECT * FROM project_checklist_items WHERE task_id = ? ORDER BY sort_order, created_at",
    )
    .bind(task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task checklist detail: {e}"))?;
    let tags = sqlx::query_as::<_, ProjectTagRow>(
        "SELECT * FROM project_tags WHERE project_id = ? ORDER BY sort_order, name",
    )
    .bind(&project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task detail tags: {e}"))?;
    let task_ids = vec![task_id.trim().to_string()];
    let task_tag_links = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_task_tag_links",
        "task_id",
        &task_ids,
    )
    .await?;
    let custom_fields = sqlx::query_as::<_, ProjectCustomFieldRow>(
        "SELECT * FROM project_custom_fields WHERE project_id = ? ORDER BY sort_order, name",
    )
    .bind(&project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task detail custom fields: {e}"))?;
    let custom_field_options = sqlx::query_as::<_, ProjectCustomFieldOptionRow>(
        "SELECT o.* FROM project_custom_field_options o
         JOIN project_custom_fields f ON f.id = o.field_id
         WHERE f.project_id = ? ORDER BY o.field_id, o.sort_order, o.name",
    )
    .bind(&project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task detail custom field options: {e}"))?;
    let custom_field_values = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_custom_field_values",
        "task_id",
        &task_ids,
    )
    .await?;
    let custom_field_option_values = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_custom_field_option_values",
        "task_id",
        &task_ids,
    )
    .await?;
    let dependencies = sqlx::query_as::<_, ProjectTaskDependencyRow>(
        "SELECT * FROM project_task_dependencies
         WHERE blocking_task_id = ? OR blocked_task_id = ? ORDER BY created_at",
    )
    .bind(task_id.trim())
    .bind(task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task detail dependencies: {e}"))?;
    let event_links = load_rows_for_task_ids(
        pool,
        "SELECT * FROM project_task_event_links",
        "task_id",
        &task_ids,
    )
    .await?;
    let task_change_events = sqlx::query_as::<_, ProjectTaskChangeEventRow>(
        "SELECT * FROM project_task_change_events WHERE task_id = ?
         ORDER BY occurred_at DESC, id DESC LIMIT 100",
    )
    .bind(task_id.trim())
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load project task detail history: {e}"))?;
    Ok(ProjectTaskDetailData {
        task,
        related_tasks,
        checklist_items,
        tags,
        task_tag_links,
        custom_fields,
        custom_field_options,
        custom_field_values,
        custom_field_option_values,
        dependencies,
        event_links,
        task_change_events,
    })
}

#[cfg(test)]
pub(super) const LIST_RESPONSE_TASK_CAP: i64 = LIST_PAGE_SIZE_MAX;
