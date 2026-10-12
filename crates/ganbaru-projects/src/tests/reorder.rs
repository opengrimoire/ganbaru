use super::*;
use crate::reorder::{ProjectReorderRequest, ReorderItem, TaskOrderAxis, reorder_item};

async fn seed(pool: &SqlitePool) {
    insert_project_graph_fixture(pool).await;
    sqlx::query("UPDATE project_tasks SET section_sort_order = CASE id WHEN 'task-a' THEN 1000 WHEN 'task-b' THEN 2000 ELSE 3000 END, status_sort_order = 5000, created_at = '2026-10-02T00:00:00Z'")
        .execute(pool).await.unwrap();
}

fn request() -> ProjectReorderRequest {
    ProjectReorderRequest {
        operation_id: "reorder-operation".into(),
        project_id: "project-a".into(),
        direction: 1,
        item: ReorderItem::Task {
            id: "task-a".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: None,
            expected_order: 1000.0,
        },
    }
}

async fn order(pool: &SqlitePool) -> Vec<(String, f64)> {
    sqlx::query_as("SELECT id, section_sort_order FROM project_tasks ORDER BY section_sort_order, created_at, id")
        .fetch_all(pool).await.unwrap()
}

#[test]
fn reorder_selects_an_unloaded_sibling_and_preserves_unrelated_native_fields() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("UPDATE project_tasks SET title = 'Newer title', completed_at = '2026-10-01T10:00:00Z' WHERE id = 'task-b'")
            .execute(&pool).await.unwrap();
        let result = reorder_item(&pool, &request()).await.unwrap();
        assert_eq!(
            order(&pool).await,
            vec![
                ("task-b".into(), 1000.0),
                ("task-a".into(), 2000.0),
                ("task-c".into(), 3000.0)
            ]
        );
        assert_eq!(result.tasks.len(), 2);
        let sibling = result
            .tasks
            .iter()
            .find(|task| task.id == "task-b")
            .unwrap();
        assert_eq!(sibling.title, "Newer title");
        assert_eq!(
            sibling.completed_at.as_deref(),
            Some("2026-10-01T10:00:00Z")
        );
        assert_eq!(sibling.status_sort_order, 5000.0);
    });
}

#[test]
fn reorder_rolls_back_both_ranks_and_receipt_when_the_second_write_fails() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        let before = order(&pool).await;
        sqlx::raw_sql("CREATE TRIGGER fail_reorder BEFORE UPDATE ON project_tasks WHEN NEW.id = 'task-b' BEGIN SELECT RAISE(ABORT, 'injected second write'); END;")
            .execute(&pool).await.unwrap();
        let error = reorder_item(&pool, &request()).await.err().unwrap();
        assert!(error.contains("injected second write"), "{error}");
        assert_eq!(order(&pool).await, before);
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM project_reorder_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
    });
}

#[test]
fn reorder_normalizes_tied_ranks_in_complete_sibling_order() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("UPDATE project_tasks SET section_sort_order = 1000 WHERE id = 'task-c'")
            .execute(&pool)
            .await
            .unwrap();
        let mut intent = request();
        intent.direction = -1;
        intent.item = ReorderItem::Task {
            id: "task-b".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: None,
            expected_order: 2000.0,
        };
        reorder_item(&pool, &intent).await.unwrap();
        assert_eq!(
            order(&pool).await,
            vec![
                ("task-a".into(), 1000.0),
                ("task-b".into(), 2000.0),
                ("task-c".into(), 3000.0)
            ]
        );
    });
}

#[test]
fn reorder_rejects_changed_placement_and_never_moves_into_another_parent() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("UPDATE project_tasks SET parent_task_id = 'task-c' WHERE id = 'task-b'")
            .execute(&pool)
            .await
            .unwrap();
        let changed = reorder_item(&pool, &request()).await.unwrap();
        assert_eq!(
            changed
                .tasks
                .iter()
                .map(|task| task.id.as_str())
                .collect::<Vec<_>>(),
            vec!["task-a", "task-c"]
        );
        let mut stale = request();
        stale.operation_id = "another-operation".into();
        assert!(
            reorder_item(&pool, &stale)
                .await
                .err()
                .unwrap()
                .contains("changed since selection")
        );
        let mut foreign = request();
        foreign.operation_id = "foreign-operation".into();
        foreign.item = ReorderItem::Task {
            id: "task-b".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: None,
            expected_order: 2000.0,
        };
        assert!(
            reorder_item(&pool, &foreign)
                .await
                .err()
                .unwrap()
                .contains("selected group")
        );
    });
}

#[test]
fn reorder_subtasks_includes_siblings_in_other_sections() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::raw_sql("INSERT INTO project_sections (id, project_id, name, sort_order) VALUES ('section-b', 'project-a', 'Another', 200);
            UPDATE project_tasks SET parent_task_id = 'task-c' WHERE id IN ('task-a', 'task-b');
            UPDATE project_tasks SET section_id = 'section-b' WHERE id = 'task-b';")
            .execute(&pool).await.unwrap();
        let mut intent = request();
        intent.item = ReorderItem::Task {
            id: "task-a".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: Some("task-c".into()),
            expected_order: 1000.0,
        };
        let changed = reorder_item(&pool, &intent).await.unwrap();
        assert_eq!(changed.tasks.len(), 2);
        assert_eq!(order(&pool).await[0], ("task-b".into(), 1000.0));
        assert_eq!(
            changed
                .tasks
                .iter()
                .find(|task| task.id == "task-b")
                .unwrap()
                .section_id,
            "section-b"
        );
    });
}

#[test]
fn reorder_receipt_recovers_lost_response_without_repeating_the_move() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        let original =
            serde_json::to_value(reorder_item(&pool, &request()).await.unwrap()).unwrap();
        sqlx::query("UPDATE project_tasks SET title = 'Later edit' WHERE id = 'task-a'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(reorder_item(&pool, &request()).await.unwrap()).unwrap(),
            original
        );
        let current: String =
            sqlx::query_scalar("SELECT title FROM project_tasks WHERE id = 'task-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(current, "Later edit");
        let mut reused = request();
        reused.direction = -1;
        assert!(
            reorder_item(&pool, &reused)
                .await
                .err()
                .unwrap()
                .contains("different intent")
        );
    });
}

#[test]
fn reorder_fields_and_options_commit_together_and_track_later_renames() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::raw_sql("INSERT INTO project_custom_fields (id, project_id, name, field_type, sort_order) VALUES ('field-a', 'project-a', 'A', 'select', 1000), ('field-b', 'project-a', 'B', 'text', 2000);
            INSERT INTO project_custom_field_options (id, field_id, name, sort_order) VALUES ('option-a', 'field-a', 'A', 1000), ('option-b', 'field-a', 'B', 2000);")
            .execute(&pool).await.unwrap();
        let mut field = request();
        field.item = ReorderItem::CustomField {
            id: "field-a".into(),
            expected_order: 1000,
        };
        let moved = reorder_item(&pool, &field).await.unwrap();
        assert_eq!(moved.custom_fields.len(), 2);
        assert!(moved.custom_fields.iter().all(|field| field.revision == 1));
        sqlx::query("UPDATE project_custom_fields SET name = 'Renamed' WHERE id = 'field-a'")
            .execute(&pool)
            .await
            .unwrap();
        let current_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM project_custom_fields WHERE id = 'field-a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(current_revision, 2);
        let replay = reorder_item(&pool, &field).await.unwrap();
        assert!(replay.custom_fields.iter().all(|field| field.revision == 1));
        let mut option = request();
        option.operation_id = "option-operation".into();
        option.item = ReorderItem::CustomFieldOption {
            id: "option-a".into(),
            field_id: "field-a".into(),
            expected_order: 1000,
        };
        let moved = reorder_item(&pool, &option).await.unwrap();
        assert_eq!(moved.custom_field_options.len(), 2);
        assert!(
            moved
                .custom_field_options
                .iter()
                .all(|option| option.revision == 1)
        );
        option.operation_id = "wrong-field-operation".into();
        option.item = ReorderItem::CustomFieldOption {
            id: "option-a".into(),
            field_id: "field-b".into(),
            expected_order: 2000,
        };
        assert!(
            reorder_item(&pool, &option)
                .await
                .err()
                .unwrap()
                .contains("selected group")
        );
    });
}
