use super::*;
use crate::custom_fields::{create_custom_field_in_pool, update_custom_field_value_with_history};
use crate::order::OrderPlacement;

#[test]
fn duplicate_custom_property_copies_option_schema_with_fresh_ids_and_empty_values() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::raw_sql("INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key) VALUES ('source', 'project-a', 'Phase', 'select', 'a1');
            INSERT INTO project_custom_field_options (id, field_id, name, order_key) VALUES ('option-one', 'source', 'Planning', 'a1'), ('option-two', 'source', 'Done', 'a2');
            INSERT INTO project_custom_field_values (task_id, field_id, sync_key) VALUES ('task-a', 'source', 'value-a-source');
            INSERT INTO project_custom_field_option_values (value_key, ordinal, option_id) VALUES ('value-a-source', 0, 'option-one');")
            .execute(&pool).await.unwrap();
        let result = create_custom_field_in_pool(
            &pool,
            &ProjectCustomFieldCreate {
                id: "copy".to_string(),
                project_id: "project-a".to_string(),
                name: "Phase copy".to_string(),
                field_type: "select".to_string(),
                placement: OrderPlacement::End,
                icon: None,
                duplicate_source_id: Some("source".to_string()),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.custom_fields[0].id, "copy");
        assert_eq!(result.custom_field_options.len(), 2);
        assert_eq!(result.custom_field_options[0].name, "Planning");
        assert_eq!(result.custom_field_options[1].name, "Done");
        assert!(
            result
                .custom_field_options
                .iter()
                .all(|option| option.field_id == "copy"
                    && !["option-one", "option-two"].contains(&option.id.as_str()))
        );
        let values: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM project_custom_field_values WHERE field_id = 'copy'), (SELECT COUNT(*) FROM project_custom_field_values WHERE field_id = 'source')")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(values, (0, 1));
        let invalid = create_custom_field_in_pool(
            &pool,
            &ProjectCustomFieldCreate {
                id: "invalid-copy".to_string(),
                project_id: "project-a".to_string(),
                name: "Invalid".to_string(),
                field_type: "text".to_string(),
                placement: OrderPlacement::End,
                icon: None,
                duplicate_source_id: Some("source".to_string()),
            },
        )
        .await;
        assert!(invalid.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM project_custom_fields WHERE id = 'invalid-copy'"
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
    });
}

#[test]
fn custom_field_icons_accept_only_lucide_icons_and_reset_to_the_type_icon() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        let field = |id: &str, icon: Option<&str>| ProjectCustomFieldCreate {
            id: id.to_string(),
            project_id: "project-a".to_string(),
            name: id.to_string(),
            field_type: "text".to_string(),
            placement: OrderPlacement::End,
            icon: icon.map(str::to_string),
            duplicate_source_id: None,
        };
        let created =
            create_custom_field_in_pool(&pool, &field("Notes", Some(" lucide:notebook:3 ")))
                .await
                .unwrap();
        assert_eq!(
            created.custom_fields[0].icon.as_deref(),
            Some("lucide:notebook:3")
        );
        for icon in [
            "emoji:x",
            "asset:project-icons/a.png",
            "lucide:Bad Slug",
            "none",
        ] {
            assert_eq!(
                create_custom_field_in_pool(&pool, &field("Rejected", Some(icon)))
                    .await
                    .err(),
                Some(if icon.starts_with("lucide:") {
                    "icon must use a canonical project icon encoding".to_string()
                } else {
                    "custom field icon must be a Lucide icon".to_string()
                })
            );
        }

        let update = |icon: Option<&str>| ProjectCustomFieldUpdate {
            id: "Notes".to_string(),
            name: "Notes".to_string(),
            placement: None,
            icon: icon.map(str::to_string),
        };
        let changed = crate::custom_fields::update_custom_field(&pool, update(Some("lucide:pen")))
            .await
            .unwrap();
        assert_eq!(changed.custom_fields[0].icon.as_deref(), Some("lucide:pen"));
        let reset = crate::custom_fields::update_custom_field(&pool, update(None))
            .await
            .unwrap();
        assert_eq!(reset.custom_fields[0].icon, None);
    });
}

#[test]
fn custom_field_value_rejects_fields_from_another_project() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-b', 'group-a', 'Project B', 'lucide:folder', 'a2')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES ('field-b', 'project-b', 'Client note', 'text', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let value = ProjectCustomFieldValueUpdate {
            task_id: "task-a".to_string(),
            field_id: "field-b".to_string(),
            text_value: Some("Wrong project".to_string()),
            number_value: None,
            date_value: None,
            checkbox_value: None,
            option_ids: Vec::new(),
        };
        let mut tx = pool.begin().await.unwrap();
        let result = update_custom_field_value_with_history(&mut tx, &value).await;

        assert_eq!(
            result,
            Err("custom field must belong to the task project".to_string())
        );
    });
}

#[test]
fn custom_field_value_rejects_options_from_another_field() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES
                ('field-a', 'project-a', 'Phase', 'select', 'a1'),
                ('field-b', 'project-a', 'Risk', 'select', 'a2')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO project_custom_field_options (id, field_id, name, order_key)
             VALUES ('option-b', 'field-b', 'High', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let value = ProjectCustomFieldValueUpdate {
            task_id: "task-a".to_string(),
            field_id: "field-a".to_string(),
            text_value: None,
            number_value: None,
            date_value: None,
            checkbox_value: None,
            option_ids: vec!["option-b".to_string()],
        };
        let mut tx = pool.begin().await.unwrap();
        let result = update_custom_field_value_with_history(&mut tx, &value).await;

        assert_eq!(
            result,
            Err("custom field option must belong to the field".to_string())
        );
    });
}

#[test]
fn custom_field_value_records_task_history() {
    block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project_graph_fixture(&pool).await;
        sqlx::query(
            "INSERT INTO project_custom_fields (id, project_id, name, field_type, order_key)
             VALUES ('field-a', 'project-a', 'Risk', 'text', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let value = ProjectCustomFieldValueUpdate {
            task_id: "task-a".to_string(),
            field_id: "field-a".to_string(),
            text_value: Some("High".to_string()),
            number_value: None,
            date_value: None,
            checkbox_value: None,
            option_ids: Vec::new(),
        };
        let mut tx = pool.begin().await.unwrap();
        update_custom_field_value_with_history(&mut tx, &value)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let stored_value: String = sqlx::query_scalar(
            "SELECT text_value
             FROM project_custom_field_values
             WHERE task_id = 'task-a' AND field_id = 'field-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let history_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM project_task_change_events
             WHERE task_id = 'task-a'
               AND event_type = 'updated'
               AND field_name = 'custom_field:Risk'
               AND old_value IS NULL
               AND new_value = 'High'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(stored_value, "High");
        assert_eq!(history_count, 1);
    });
}
