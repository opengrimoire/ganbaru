use super::helpers::*;

#[test]
fn link_facts_rebuild_payload_and_file_references() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let target_url =
            format!("http://localhost:1420/?view=notes#notes?page={PAGE_B}&block={BLOCK_B}");
        let block_asset_path =
            "notes/files/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";
        let comment_asset_path =
            "notes/files/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.pdf";
        let callout_icon_path =
            "notes/page-icons/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png";

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(
                        BLOCK_C,
                        "paragraph",
                        json!({
                            "rich_text": [
                                rich_text("See "),
                                page_mention(PAGE_B, "Target page"),
                                linked_rich_text(" local target", &target_url),
                                linked_rich_text(" docs", "https://example.com/docs"),
                                project_task_mention(BLOCK_E, "Task")
                            ],
                            "color": "default"
                        }),
                    ),
                    block(
                        BLOCK_D,
                        "image",
                        local_media_payload(
                            block_asset_path,
                            "image/png",
                            12,
                            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "Diagram",
                            Some("diagram.png"),
                        ),
                    ),
                    block(
                        BLOCK_F,
                        "callout",
                        json!({
                            "rich_text": [rich_text("Note")],
                            "color": "gray_background",
                            "icon": {
                                "type": "file",
                                "file": {
                                    "url": format!("ganbaru-asset:{callout_icon_path}"),
                                    "ganbaru_asset_path": callout_icon_path,
                                    "content_type": "image/png",
                                    "byte_size": 12,
                                    "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                                }
                            }
                        }),
                    ),
                ],
            },
        )
        .await
        .unwrap();

        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![page_mention(PAGE_B, "Comment target")],
                attachments: Some(vec![local_comment_attachment(
                    comment_asset_path,
                    "application/pdf",
                    21,
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "brief.pdf",
                )]),
            },
        )
        .await
        .unwrap();

        let count = links::facts::rebuild_index(&pool).await.unwrap();
        assert!(count >= 7);
        assert_fact(&pool, "block", BLOCK_C, "page", PAGE_B, "page_mention").await;
        assert_fact(&pool, "block", BLOCK_C, "page", PAGE_B, "page_link").await;
        assert_fact(&pool, "block", BLOCK_C, "block", BLOCK_B, "block_link").await;
        assert_fact(
            &pool,
            "block",
            BLOCK_C,
            "external_url",
            "https://example.com/docs",
            "external_url",
        )
        .await;
        assert_fact(
            &pool,
            "block",
            BLOCK_C,
            "project_task",
            BLOCK_E,
            "local_object_mention",
        )
        .await;
        assert_fact(
            &pool,
            "block",
            BLOCK_D,
            "file",
            block_asset_path,
            "block_file",
        )
        .await;
        assert_fact(
            &pool,
            "block",
            BLOCK_F,
            "file",
            callout_icon_path,
            "callout_icon",
        )
        .await;
        assert_fact(
            &pool,
            "comment",
            COMMENT_A,
            "file",
            comment_asset_path,
            "comment_attachment",
        )
        .await;

        sqlx::query("DELETE FROM notes_link_facts")
            .execute(&pool)
            .await
            .unwrap();
        let rebuilt_count = links::facts::rebuild_index(&pool).await.unwrap();
        assert_eq!(rebuilt_count, count);
        assert_fact(&pool, "block", BLOCK_C, "block", BLOCK_B, "block_link").await;
    });
}

#[test]
fn link_facts_rebuild_database_relations_and_property_urls() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Projects",
            DATABASE_A,
        )
        .await;
        data_sources::schema::update_data_source_schema(
            &pool,
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
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B
                        }
                    },
                    "URL": {
                        "id": "url",
                        "name": "URL",
                        "type": "url",
                        "url": {}
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                title: "Project Alpha".to_string(),
                first_block_id: BLOCK_C.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Write graph tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: Some(json!({
                    "Project": {
                        "id": "project_relation",
                        "type": "relation",
                        "relation": [PAGE_C]
                    },
                    "URL": {
                        "id": "url",
                        "type": "url",
                        "url": "https://example.com/project"
                    }
                })),
            },
        )
        .await
        .unwrap();

        let count = links::facts::rebuild_index(&pool).await.unwrap();
        assert!(count >= 2);
        assert_fact(
            &pool,
            "property",
            &format!("{PAGE_B}:project_relation"),
            "database_row",
            PAGE_C,
            "database_relation",
        )
        .await;
        assert_fact(
            &pool,
            "property",
            &format!("{PAGE_B}:url"),
            "external_url",
            "https://example.com/project",
            "external_url",
        )
        .await;
    });
}

async fn assert_fact(
    pool: &SqlitePool,
    source_type: &str,
    source_id: &str,
    target_type: &str,
    target_id: &str,
    link_type: &str,
) {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM notes_link_facts
         WHERE source_object_type = ?
           AND source_object_id = ?
           AND target_object_type = ?
           AND target_object_id = ?
           AND link_type = ?",
    )
    .bind(source_type)
    .bind(source_id)
    .bind(target_type)
    .bind(target_id)
    .bind(link_type)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}
