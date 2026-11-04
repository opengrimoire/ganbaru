use super::*;
use crate::order::rank_key;
use crate::reorder::{ProjectReorderRequest, ReorderItem, TaskOrderAxis, reorder_item};

fn key(index: usize) -> String {
    rank_key(index).unwrap().into_string()
}

async fn seed(pool: &SqlitePool) {
    insert_project_graph_fixture(pool).await;
    for (index, id) in ["task-a", "task-b", "task-c"].iter().enumerate() {
        sqlx::query(
            "UPDATE project_tasks SET section_order_key = ?, status_order_key = ? WHERE id = ?",
        )
        .bind(key(index + 1))
        .bind(key(5))
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
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
            expected_order_key: key(1),
        },
    }
}

async fn order(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT id FROM project_tasks ORDER BY section_order_key, id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn task_versions(pool: &SqlitePool) -> Vec<(String, i64, String)> {
    sqlx::query_as("SELECT id, revision, updated_at FROM project_tasks ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[test]
fn reorder_writes_one_order_key_and_keeps_revisions_and_unrelated_fields() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("UPDATE project_tasks SET title = 'Newer title', completed_at = '2026-10-01T10:00:00Z' WHERE id = 'task-b'")
            .execute(&pool).await.unwrap();
        let before = task_versions(&pool).await;
        let result = reorder_item(&pool, &request()).await.unwrap();
        assert_eq!(order(&pool).await, ["task-b", "task-a", "task-c"]);
        assert_eq!(result.tasks.len(), 1);
        assert_eq!(result.tasks[0].id, "task-a");
        assert!(result.tasks[0].section_order_key > key(2));
        assert!(result.tasks[0].section_order_key < key(3));
        assert_eq!(result.tasks[0].status_order_key, key(5));
        assert_eq!(task_versions(&pool).await, before);
        let sibling: (String, Option<String>) =
            sqlx::query_as("SELECT title, completed_at FROM project_tasks WHERE id = 'task-b'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            sibling,
            (
                "Newer title".to_string(),
                Some("2026-10-01T10:00:00Z".to_string())
            )
        );
    });
}

#[test]
fn reorder_rolls_back_the_order_key_and_receipt_when_the_write_fails() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        let before = order(&pool).await;
        sqlx::raw_sql("CREATE TRIGGER fail_reorder BEFORE UPDATE ON project_tasks WHEN NEW.id = 'task-a' BEGIN SELECT RAISE(ABORT, 'injected write failure'); END;")
            .execute(&pool).await.unwrap();
        let error = reorder_item(&pool, &request()).await.err().unwrap();
        assert!(error.contains("injected write failure"), "{error}");
        assert_eq!(order(&pool).await, before);
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM project_reorder_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
    });
}

#[test]
fn reorder_order_key_ties_resolve_by_id_before_the_move() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("UPDATE project_tasks SET section_order_key = ? WHERE id = 'task-c'")
            .bind(key(1))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(order(&pool).await, ["task-a", "task-c", "task-b"]);
        let mut intent = request();
        intent.direction = -1;
        intent.item = ReorderItem::Task {
            id: "task-b".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: None,
            expected_order_key: key(2),
        };
        reorder_item(&pool, &intent).await.unwrap();
        assert_eq!(order(&pool).await, ["task-a", "task-b", "task-c"]);
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
            ["task-a"]
        );
        assert!(changed.tasks[0].section_order_key > key(3));
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
            expected_order_key: key(2),
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
        sqlx::raw_sql("INSERT INTO project_sections (id, project_id, name, order_key) VALUES ('section-b', 'project-a', 'Another', 'a2');
            UPDATE project_tasks SET parent_task_id = 'task-c' WHERE id IN ('task-a', 'task-b');
            UPDATE project_tasks SET section_id = 'section-b' WHERE id = 'task-b';")
            .execute(&pool).await.unwrap();
        let mut intent = request();
        intent.item = ReorderItem::Task {
            id: "task-a".into(),
            axis: TaskOrderAxis::Section,
            group_id: "section-a".into(),
            parent_task_id: Some("task-c".into()),
            expected_order_key: key(1),
        };
        let changed = reorder_item(&pool, &intent).await.unwrap();
        assert_eq!(changed.tasks.len(), 1);
        assert_eq!(order(&pool).await[0], "task-b");
        assert_eq!(changed.tasks[0].section_id, "section-a");
    });
}

#[test]
fn reorder_receipt_recovers_lost_response_without_repeating_the_move() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        let original =
            serde_json::to_value(reorder_item(&pool, &request()).await.unwrap()).unwrap();
        let moved = order(&pool).await;
        sqlx::query("UPDATE project_tasks SET title = 'Later edit' WHERE id = 'task-a'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(reorder_item(&pool, &request()).await.unwrap()).unwrap(),
            original
        );
        assert_eq!(order(&pool).await, moved);
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
fn reorder_fields_and_options_write_one_order_key_without_revision_bumps() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        seed(&pool).await;
        sqlx::query("INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key) VALUES ('field-a', 'project-a', 'A', 'select', ?), ('field-b', 'project-a', 'B', 'text', ?)")
            .bind(key(1)).bind(key(2)).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO project_custom_field_options (id, field_id, name, order_key) VALUES ('option-a', 'field-a', 'A', ?), ('option-b', 'field-a', 'B', ?)")
            .bind(key(1)).bind(key(2)).execute(&pool).await.unwrap();
        let mut field = request();
        field.item = ReorderItem::CustomField {
            id: "field-a".into(),
            expected_order_key: key(1),
        };
        let moved = reorder_item(&pool, &field).await.unwrap();
        assert_eq!(moved.custom_fields.len(), 1);
        assert_eq!(moved.custom_fields[0].id, "field-a");
        assert_eq!(moved.custom_fields[0].revision, 0);
        assert!(moved.custom_fields[0].order_key > key(2));
        let replay = reorder_item(&pool, &field).await.unwrap();
        assert_eq!(
            replay.custom_fields[0].order_key,
            moved.custom_fields[0].order_key
        );

        let mut option = request();
        option.operation_id = "option-operation".into();
        option.item = ReorderItem::CustomFieldOption {
            id: "option-b".into(),
            field_id: "field-a".into(),
            expected_order_key: key(2),
        };
        option.direction = -1;
        let moved = reorder_item(&pool, &option).await.unwrap();
        assert_eq!(moved.custom_field_options.len(), 1);
        assert_eq!(moved.custom_field_options[0].revision, 0);
        assert!(moved.custom_field_options[0].order_key < key(1));
        option.operation_id = "wrong-field-operation".into();
        option.item = ReorderItem::CustomFieldOption {
            id: "option-a".into(),
            field_id: "field-b".into(),
            expected_order_key: key(1),
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
