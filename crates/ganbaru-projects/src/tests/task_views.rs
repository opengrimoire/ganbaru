use super::*;
use std::collections::{HashMap, HashSet};

#[test]
fn sql_like_contains_pattern_escapes_wildcards() {
    assert_eq!(sql_like_contains_pattern("100% done"), "%100\\% done%");
    assert_eq!(sql_like_contains_pattern("task_1"), "%task\\_1%");
    assert_eq!(sql_like_contains_pattern(r"c:\work"), r"%c:\\work%");
}

#[test]
fn normalize_optional_date_filter_accepts_blank_and_iso_dates() {
    assert_eq!(normalize_optional_date_filter(None, "start_date"), Ok(None));
    assert_eq!(
        normalize_optional_date_filter(Some(" ".to_string()), "start_date"),
        Ok(None)
    );
    assert_eq!(
        normalize_optional_date_filter(Some("2026-06-12".to_string()), "start_date"),
        Ok(Some("2026-06-12".to_string()))
    );
}

#[test]
fn normalize_optional_date_filter_rejects_malformed_dates() {
    assert_eq!(
        normalize_optional_date_filter(Some("2026-6-12".to_string()), "start_date"),
        Err("start_date must use YYYY-MM-DD".to_string())
    );
    assert_eq!(
        normalize_optional_date_filter(Some("2026/06/12".to_string()), "end_date"),
        Err("end_date must use YYYY-MM-DD".to_string())
    );
}

fn list_task_view_request(page_size: i64) -> ProjectTaskViewRequest {
    ProjectTaskViewRequest {
        project_id: "project-a".to_string(),
        view: ProjectViewId::List,
        page_size,
        cursor: None,
        column_cursors: HashMap::new(),
        show_archived: false,
        visible_section_ids: vec!["section-a".to_string()],
        search: String::new(),
        status_filter: "all".to_string(),
        section_filter: "all".to_string(),
        priority_filter: "all".to_string(),
        due_filter: "all".to_string(),
        due_range_start: String::new(),
        due_range_end: String::new(),
        today: "2026-07-11".to_string(),
        week_end: "2026-07-18".to_string(),
        schedule_filter: "all".to_string(),
        dependency_filter: "all".to_string(),
        tag_filter: "all".to_string(),
        custom_field_filters: Vec::new(),
        sort_mode: "manual".to_string(),
        sort_direction: "asc".to_string(),
        candidate_event_ids: Vec::new(),
    }
}

#[test]
fn task_view_column_calculations_cover_filtered_results_before_pagination() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::raw_sql("UPDATE project_tasks SET estimate_minutes = CASE id WHEN 'task-a' THEN NULL WHEN 'task-b' THEN 20 ELSE 100 END;
            UPDATE project_tasks SET archived_at = '2026-07-01' WHERE id = 'task-c';
            INSERT INTO project_custom_fields (id, project_id, name, field_type, sort_order) VALUES ('number', 'project-a', 'Points', 'number', 100), ('choice', 'project-a', 'Phase', 'select', 200), ('empty', 'project-a', 'Empty', 'number', 300);
            INSERT INTO project_custom_field_values (task_id, field_id, number_value) VALUES ('task-a', 'number', 0), ('task-b', 'number', 10), ('task-c', 'number', 100);
            INSERT INTO project_custom_field_options (id, field_id, name, sort_order) VALUES ('choice-one', 'choice', 'Planning', 100);
            INSERT INTO project_custom_field_option_values (task_id, field_id, option_id) VALUES ('task-b', 'choice', 'choice-one');")
            .execute(&pool).await.unwrap();
        let first = load_task_view(&pool, list_task_view_request(1))
            .await
            .unwrap();
        assert_eq!(first.tasks.len(), 1);
        let points = first
            .column_calculations
            .iter()
            .find(|entry| entry.column == "custom:number")
            .unwrap();
        assert_eq!(
            (
                points.total,
                points.filled,
                points.sum,
                points.average,
                points.minimum,
                points.maximum
            ),
            (2, 2, Some(10.0), Some(5.0), Some(0.0), Some(10.0))
        );
        let choices = first
            .column_calculations
            .iter()
            .find(|entry| entry.column == "custom:choice")
            .unwrap();
        assert_eq!((choices.total, choices.filled), (2, 1));
        let empty = first
            .column_calculations
            .iter()
            .find(|entry| entry.column == "custom:empty")
            .unwrap();
        assert_eq!((empty.total, empty.filled, empty.sum), (2, 0, None));
        let estimate = first
            .column_calculations
            .iter()
            .find(|entry| entry.column == "estimate")
            .unwrap();
        assert_eq!(
            (estimate.total, estimate.filled, estimate.sum),
            (2, 1, Some(20.0))
        );
        let mut next = list_task_view_request(1);
        next.cursor = first.next_cursor;
        let second = load_task_view(&pool, next).await.unwrap();
        let next_points = second
            .column_calculations
            .iter()
            .find(|entry| entry.column == "custom:number")
            .unwrap();
        assert_eq!((next_points.total, next_points.sum), (2, Some(10.0)));
        let mut filtered = list_task_view_request(1);
        filtered.search = "task-b".to_string();
        let filtered = load_task_view(&pool, filtered).await.unwrap();
        let filtered_points = filtered
            .column_calculations
            .iter()
            .find(|entry| entry.column == "custom:number")
            .unwrap();
        assert_eq!(
            (filtered_points.total, filtered_points.sum),
            (1, Some(10.0))
        );
    });
}

#[test]
fn task_view_custom_column_sort_keeps_numeric_order_and_resolves_option_names() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::raw_sql("INSERT INTO project_custom_fields (id, project_id, name, field_type, sort_order) VALUES ('number', 'project-a', 'Points', 'number', 100), ('choice', 'project-a', 'Phase', 'select', 200);
            INSERT INTO project_custom_field_values (task_id, field_id, number_value) VALUES ('task-a', 'number', 10), ('task-b', 'number', 2);
            INSERT INTO project_custom_field_options (id, field_id, name, sort_order) VALUES ('choice-z', 'choice', 'Zebra', 100), ('choice-a', 'choice', 'Alpha', 200);
            INSERT INTO project_custom_field_option_values (task_id, field_id, option_id) VALUES ('task-a', 'choice', 'choice-z'), ('task-b', 'choice', 'choice-a');")
            .execute(&pool).await.unwrap();
        for field in ["number", "choice"] {
            let mut request = list_task_view_request(10);
            request.sort_mode = format!("custom:{field}");
            let result = load_task_view(&pool, request).await.unwrap();
            assert_eq!(
                result
                    .tasks
                    .iter()
                    .map(|task| task.id.as_str())
                    .collect::<Vec<_>>(),
                vec!["task-b", "task-a", "task-c"]
            );
        }
    });
}

#[test]
fn task_view_list_is_keyset_paginated_and_body_bounded() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query("DELETE FROM project_tasks")
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        for index in 0..10_000 {
            sqlx::query(
                "INSERT INTO project_tasks
                 (id, project_id, section_id, status_id, title, description, section_sort_order)
                 VALUES (?, 'project-a', 'section-a', 'status-a', ?, ?, ?)",
            )
            .bind(format!("task-{index:05}"))
            .bind(format!("Task {index:05}"))
            .bind("body".repeat(1_000))
            .bind(index as f64)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();

        let first = load_task_view(&pool, list_task_view_request(10_000))
            .await
            .unwrap();
        assert_eq!(first.total_count, 10_000);
        assert_eq!(first.matched_count, 10_000);
        assert_eq!(
            first.tasks.len(),
            crate::task_views::LIST_RESPONSE_TASK_CAP as usize
        );
        assert!(serde_json::to_vec(&first).unwrap().len() < 100_000);
        let first_ids: HashSet<_> = first.tasks.iter().map(|task| task.id.as_str()).collect();

        let mut next_request = list_task_view_request(100);
        next_request.cursor = first.next_cursor.clone();
        let second = load_task_view(&pool, next_request).await.unwrap();
        assert_eq!(second.tasks.len(), 100);
        assert!(
            second
                .tasks
                .iter()
                .all(|task| !first_ids.contains(task.id.as_str()))
        );

        let mut stale_request = list_task_view_request(100);
        stale_request.cursor = Some("removed-task".to_string());
        let error = match load_task_view(&pool, stale_request).await {
            Ok(_) => panic!("stale cursor should fail"),
            Err(error) => error,
        };
        assert_eq!(error, "project task cursor is stale");
    });
}

#[test]
fn task_view_handles_empty_single_and_exact_page_sizes() {
    block_on(async {
        for count in [0, 1, 100] {
            let pool = migrated_memory_pool().await;
            insert_project_graph_fixture(&pool).await;
            sqlx::query("DELETE FROM project_tasks")
                .execute(&pool)
                .await
                .unwrap();
            for index in 0..count {
                sqlx::query(
                    "INSERT INTO project_tasks
                     (id, project_id, section_id, status_id, title, section_sort_order)
                     VALUES (?, 'project-a', 'section-a', 'status-a', ?, ?)",
                )
                .bind(format!("task-{index}"))
                .bind(format!("Task {index}"))
                .bind(index as f64)
                .execute(&pool)
                .await
                .unwrap();
            }
            let page = load_task_view(&pool, list_task_view_request(100))
                .await
                .unwrap();
            assert_eq!(page.tasks.len(), count);
            assert_eq!(page.matched_count, count as i64);
            assert!(page.next_cursor.is_none());
        }
    });
}

#[test]
fn task_view_filters_bodies_without_returning_them_and_detail_hydrates_one_task() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "UPDATE project_tasks SET description = 'private needle', blocker_reason = 'secret',
             due_date = '2026-07-10', estimate_minutes = 25 WHERE id = 'task-b'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut request = list_task_view_request(100);
        request.search = "needle".to_string();
        request.sort_mode = "due".to_string();
        let page = load_task_view(&pool, request).await.unwrap();
        assert_eq!(page.matched_count, 1);
        assert_eq!(page.tasks[0].id, "task-b");
        let summary_json = serde_json::to_value(&page.tasks[0]).unwrap();
        assert!(summary_json.get("description").is_none());
        assert!(summary_json.get("blocker_reason").is_none());
        assert_eq!(summary_json["blocker_reason_present"], 1);

        let detail = load_task_detail(&pool, "task-b").await.unwrap();
        assert_eq!(detail.task.description, "private needle");
        assert_eq!(detail.task.blocker_reason.as_deref(), Some("secret"));
        assert!(detail.related_tasks.len() <= 200);
    });
}

#[test]
fn kanban_and_dashboard_return_counts_with_bounded_samples() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let mut kanban = list_task_view_request(100);
        kanban.view = ProjectViewId::Kanban;
        let kanban_page = load_task_view(&pool, kanban).await.unwrap();
        assert_eq!(kanban_page.column_counts.len(), 1);
        assert_eq!(kanban_page.column_counts[0].count, 3);

        let mut dashboard = list_task_view_request(100);
        dashboard.view = ProjectViewId::Dashboard;
        let dashboard_page = load_task_view(&pool, dashboard).await.unwrap();
        assert_eq!(dashboard_page.tasks.len(), 3);
        assert_eq!(dashboard_page.aggregates.unwrap().total, 3);
    });
}
