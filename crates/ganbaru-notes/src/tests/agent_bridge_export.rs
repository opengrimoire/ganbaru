use super::helpers::*;

const PROJECT_GROUP_ID: &str = "44444444-4444-4444-8444-444444444444";
const PROJECT_ID: &str = "55555555-5555-4555-8555-555555555555";
const PROJECT_SECTION_ID: &str = "66666666-6666-4666-8666-666666666666";
const PROJECT_STATUS_ID: &str = "77777777-7777-4777-8777-777777777777";
const PROJECT_TASK_ID: &str = "88888888-8888-4888-8888-888888888888";
const PROJECT_CHECKLIST_ID: &str = "99999999-9999-4999-8999-999999999999";
const PROJECT_TAG_ID: &str = "12121212-1212-4212-8212-121212121212";

#[test]
fn agent_bridge_export_renders_deterministic_derivative_context() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Agent bridge root".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        create_page(&pool, PAGE_B, BLOCK_D).await;
        writes::update_page(
            &pool,
            PAGE_B,
            NotePageUpdate {
                title: Some("Source page".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();

        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Bridge data",
            BLOCK_A,
        )
        .await;
        seed_database_view(&pool).await;
        seed_project_context(&pool).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(DATABASE_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("Knowledge for "),
                            project_task_mention(PROJECT_TASK_ID, "Implement bridge export"),
                            rich_text(".")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_B),
                after: Some(BLOCK_D.to_string()),
                children: vec![block(
                    BLOCK_E,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("Backlink to "),
                            page_mention(PAGE_A, "Agent bridge root")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();
        search::rebuild_index(&pool).await.unwrap();
        links::backlinks::rebuild_index(&pool).await.unwrap();
        links::facts::rebuild_index(&pool).await.unwrap();

        let request = NoteAgentBridgeExportRequest {
            page_ids: vec![PAGE_A.to_string()],
            project_ids: Vec::new(),
            include_descendants: Some(true),
            include_backlinks: Some(true),
            include_database_views: Some(true),
            include_task_context: Some(true),
            include_page_comments: Some(false),
            include_resolved_comments: Some(false),
        };
        let export = transfers::agent_bridge_export::export_bridge(&pool, request.clone())
            .await
            .unwrap();
        let repeated = transfers::agent_bridge_export::export_bridge(&pool, request)
            .await
            .unwrap();

        assert_eq!(export.markdown, repeated.markdown);
        assert_eq!(export.exported_page_count, 1);
        assert_eq!(export.exported_project_count, 1);
        assert_eq!(export.exported_task_count, 1);
        assert_eq!(export.exported_database_view_count, 1);
        assert_eq!(export.exported_backlink_count, 1);
        assert!(
            export
                .markdown
                .contains("Derivative markdown view generated from Ganbaru AI SQLite")
        );
        assert!(
            export
                .markdown
                .contains("Bridge page id: `11111111-1111-4111-8111-111111111111`")
        );
        assert!(export.markdown.contains("## Project knowledge base pages"));
        assert!(export.markdown.contains("Knowledge for"));
        assert!(export.markdown.contains("## Database views"));
        assert!(export.markdown.contains("| Name | Status |"));
        assert!(export.markdown.contains("| Bridge fixture | Ready |"));
        assert!(export.markdown.contains("## Project task context"));
        assert!(export.markdown.contains("Agent bridge project"));
        assert!(export.markdown.contains("Implement bridge export"));
        assert!(export.markdown.contains("Review deterministic markdown"));
        assert!(export.markdown.contains("agent"));
        assert!(export.markdown.contains("## Backlinks"));
        assert!(export.markdown.contains("Source page"));
    });
}

async fn seed_database_view(pool: &SqlitePool) {
    data_sources::schema::update_data_source_schema(
        pool,
        DATA_SOURCE_A,
        None,
        None,
        NoteDataSourceSchemaUpdate {
            properties: json!({
                "Name": {
                    "id": "title",
                    "name": "Name",
                    "type": "title",
                    "title": {}
                },
                "Status": {
                    "id": "status",
                    "name": "Status",
                    "type": "rich_text",
                    "rich_text": {}
                }
            }),
        },
    )
    .await
    .unwrap();
    data_sources::csv_import::import_csv(
        pool,
        DATA_SOURCE_A,
        NoteDataSourceCsvImportRequest {
            csv: "Name,Status\nBridge fixture,Ready\n".to_string(),
            has_header: Some(true),
            dry_run: Some(false),
        },
    )
    .await
    .unwrap();
}

async fn seed_project_context(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, sort_order)
         VALUES (?, 'Agent context', 'lucide:folder', 0)",
    )
    .bind(PROJECT_GROUP_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name, icon, sort_order, status)
         VALUES (?, ?, 'Agent bridge project', 'lucide:folder', 0, 'active')",
    )
    .bind(PROJECT_ID)
    .bind(PROJECT_GROUP_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_sections (id, project_id, name, sort_order)
         VALUES (?, ?, 'Implementation', 0)",
    )
    .bind(PROJECT_SECTION_ID)
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_statuses (id, project_id, name, category, color, sort_order, terminal)
         VALUES (?, ?, 'In progress', 'active', 19, 0, 0)",
    )
    .bind(PROJECT_STATUS_ID)
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_priorities (id, project_id, name, color, sort_order)
         VALUES ('normal', ?, 'Normal', 19, 0)",
    )
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_tasks (
            id,
            project_id,
            section_id,
            status_id,
            title,
            description,
            priority,
            task_type,
            section_sort_order,
            status_sort_order,
            estimate_minutes,
            due_date
         )
         VALUES (?, ?, ?, ?, 'Implement bridge export', 'Expose deterministic context.', 'normal', 'task', 0, 0, 45, '2026-07-10')",
    )
    .bind(PROJECT_TASK_ID)
    .bind(PROJECT_ID)
    .bind(PROJECT_SECTION_ID)
    .bind(PROJECT_STATUS_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_checklist_items (id, task_id, title, sort_order)
         VALUES (?, ?, 'Review deterministic markdown', 0)",
    )
    .bind(PROJECT_CHECKLIST_ID)
    .bind(PROJECT_TASK_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_tags (id, project_id, name, color, sort_order)
         VALUES (?, ?, 'agent context', 23, 0)",
    )
    .bind(PROJECT_TAG_ID)
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_task_tag_links (task_id, tag_id)
         VALUES (?, ?)",
    )
    .bind(PROJECT_TASK_ID)
    .bind(PROJECT_TAG_ID)
    .execute(pool)
    .await
    .unwrap();
}
