use super::*;
use crate::custom_fields::update_custom_field_value;
use crate::keys::{custom_field_value_key, dependency_key, task_tag_key};
use crate::relationship_commands::{
    create_task_dependency, delete_task_dependency, link_task_tag, unlink_task_tag,
};

fn dependency(blocking: &str, blocked: &str) -> ProjectTaskDependencyCreate {
    ProjectTaskDependencyCreate {
        blocking_task_id: blocking.to_string(),
        blocked_task_id: blocked.to_string(),
        dependency_type: "blocks".to_string(),
    }
}

fn number_value(task_id: &str, number: Option<&str>) -> ProjectCustomFieldValueUpdate {
    ProjectCustomFieldValueUpdate {
        task_id: task_id.to_string(),
        field_id: "points".to_string(),
        text_value: None,
        number_value: number.map(|text| FixedPointNumber::parse(text).unwrap()),
        date_value: None,
        checkbox_value: None,
        option_ids: Vec::new(),
    }
}

#[test]
fn sync_key_of_a_dependency_is_its_id_and_repeated_creates_and_deletes_converge() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let key = dependency_key("task-a", "task-b").unwrap();
        assert_ne!(key, dependency_key("task-b", "task-a").unwrap());

        for _ in 0..2 {
            create_task_dependency(&pool, dependency(" task-a ", "task-b"))
                .await
                .unwrap();
        }
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, sync_key FROM project_task_dependencies")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, [(key.clone(), key.clone())]);
        let added: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM project_task_change_events WHERE event_type = 'dependency_added'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(added, 1);

        for _ in 0..2 {
            delete_task_dependency(&pool, key.clone()).await.unwrap();
        }
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM project_task_dependencies")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn sync_key_of_a_tag_link_is_derived_and_repeated_links_and_unlinks_converge() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_tags (id, project_id, name, order_key)
             VALUES ('tag-a', 'project-a', 'Backend', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let link = || ProjectTaskTagLinkCreate {
            task_id: "task-a".to_string(),
            tag_id: "tag-a".to_string(),
        };
        link_task_tag(&pool, link()).await.unwrap();
        link_task_tag(&pool, link()).await.unwrap();
        let keys: Vec<String> = sqlx::query_scalar("SELECT sync_key FROM project_task_tag_links")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(keys, [task_tag_key("task-a", "tag-a").unwrap()]);

        for _ in 0..2 {
            unlink_task_tag(&pool, "task-a".to_string(), "tag-a".to_string())
                .await
                .unwrap();
        }
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM project_task_tag_links")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn sync_key_of_a_custom_field_value_survives_updates_and_an_empty_value_deletes_the_row() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES ('points', 'project-a', 'Points', 'number', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let first = update_custom_field_value(&pool, number_value("task-a", Some("2.5")))
            .await
            .unwrap();
        assert_eq!(
            first.custom_field_values[0].number_value,
            Some(FixedPointNumber::from_millionths(2_500_000))
        );
        update_custom_field_value(&pool, number_value("task-a", Some("-0.000001")))
            .await
            .unwrap();
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT sync_key, number_value FROM project_custom_field_values")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows,
            [(custom_field_value_key("task-a", "points").unwrap(), -1)]
        );

        update_custom_field_value(&pool, number_value("task-a", None))
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM project_custom_field_values")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}
