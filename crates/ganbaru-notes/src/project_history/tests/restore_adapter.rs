use super::*;

#[test]
fn project_restore_trashes_later_notes_and_keeps_a_safety_version() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        let baseline =
            create_checkpoint(&pool, PROJECT_ID, "baseline", None, None, "Initial version")
                .await
                .unwrap()
                .unwrap();
        insert_project_page(&pool, LATER_PAGE_ID, LATER_BLOCK_ID, "Later note").await;
        restore::restore_version(&pool, PROJECT_ID, &baseline.id)
            .await
            .unwrap();
        let later_page: (i64, Option<String>) =
            sqlx::query_as("SELECT in_trash, trash_owner FROM notes_pages WHERE id = ?")
                .bind(LATER_PAGE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(later_page, (1, None));
        let later_block_trashed: i64 =
            sqlx::query_scalar("SELECT in_trash FROM notes_blocks WHERE id = ?")
                .bind(LATER_BLOCK_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(later_block_trashed, 0);
        let version_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_project_history_versions WHERE project_id = ?",
        )
        .bind(PROJECT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(version_count >= 3);
    });
}

#[test]
fn project_restore_brings_back_comment_reactions() {
    crate::test_block_on(async {
        const COMMENT_ID: &str = "60606060-6060-4060-8060-606060606060";
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        crate::comments::create_comment(
            &pool,
            crate::models::NoteCommentCreate {
                id: COMMENT_ID.to_string(),
                parent: Some(crate::models::NoteParent::PageId {
                    page_id: PAGE_ID.to_string(),
                }),
                discussion_id: None,
                anchor: None,
                rich_text: vec![serde_json::json!({
                    "type": "text",
                    "text": { "content": "Ship it", "link": null },
                    "annotations": {
                        "bold": false,
                        "italic": false,
                        "strikethrough": false,
                        "underline": false,
                        "code": false,
                        "color": "default"
                    },
                    "plain_text": "Ship it",
                    "href": null
                })],
                attachments: None,
            },
        )
        .await
        .unwrap();
        crate::comments::toggle_comment_reaction(&pool, COMMENT_ID, "👍")
            .await
            .unwrap();
        let reacted = create_checkpoint(&pool, PROJECT_ID, "manual", None, None, "With reaction")
            .await
            .unwrap()
            .unwrap();
        crate::comments::toggle_comment_reaction(&pool, COMMENT_ID, "👍")
            .await
            .unwrap();

        restore::restore_version(&pool, PROJECT_ID, &reacted.id)
            .await
            .unwrap();

        let reactions: Vec<String> =
            sqlx::query_scalar("SELECT emoji FROM notes_comment_reactions WHERE comment_id = ?")
                .bind(COMMENT_ID)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(reactions, vec!["👍".to_string()]);
    });
}

async fn row_state(pool: &SqlitePool, table: &str, id: &str) -> (i64, i64) {
    let sql = format!("SELECT rowid, in_trash FROM {table} WHERE id = ?");
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn project_restore_rewrites_rows_in_place_and_keeps_nested_content_and_page_history() {
    crate::test_block_on(async {
        const NESTED_PAGE_ID: &str = "70707070-7070-4070-8070-707070707070";
        const NESTED_BLOCK_ID: &str = "80808080-8080-4080-8080-808080808080";
        const EXTRA_BLOCK_ID: &str = "90909090-9090-4090-8090-909090909090";
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        crate::writes::create_page(
            &pool,
            crate::models::NotePageCreate {
                id: NESTED_PAGE_ID.to_string(),
                title: "Nested".to_string(),
                parent: crate::NoteParent::PageId {
                    page_id: PAGE_ID.to_string(),
                },
                folder_id: None,
                project_id: None,
                first_block_id: NESTED_BLOCK_ID.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let snapshot_id = crate::page_history::record_page_snapshot_tx(&mut tx, PAGE_ID, "manual")
            .await
            .unwrap()
            .expect("the page snapshot is recorded");
        tx.commit().await.unwrap();
        let version = create_checkpoint(&pool, PROJECT_ID, "manual", None, None, "Nested")
            .await
            .unwrap()
            .unwrap();
        let page_before = row_state(&pool, "notes_pages", PAGE_ID).await;
        let block_before = row_state(&pool, "notes_blocks", BLOCK_ID).await;
        let nested_page_before = row_state(&pool, "notes_pages", NESTED_PAGE_ID).await;
        let nested_block_before = row_state(&pool, "notes_blocks", NESTED_BLOCK_ID).await;

        sqlx::query("UPDATE notes_blocks SET text = ?, plain_text = 'Edited' WHERE id = ?")
            .bind(crate::writes::default_text_payload("Edited").to_string())
            .bind(BLOCK_ID)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO notes_blocks (
                id, page_id, parent_type, parent_page_id, type, attrs, text, plain_text, order_key
             ) VALUES (?, ?, 'page_id', ?, 'paragraph', '{}', '{}', '', 'a9')",
        )
        .bind(EXTRA_BLOCK_ID)
        .bind(PAGE_ID)
        .bind(PAGE_ID)
        .execute(&pool)
        .await
        .unwrap();
        insert_project_page(&pool, LATER_PAGE_ID, LATER_BLOCK_ID, "Later note").await;

        restore::restore_version(&pool, PROJECT_ID, &version.id)
            .await
            .unwrap();

        assert_eq!(row_state(&pool, "notes_pages", PAGE_ID).await, page_before);
        assert_eq!(
            row_state(&pool, "notes_blocks", BLOCK_ID).await,
            block_before
        );
        assert_eq!(
            row_state(&pool, "notes_pages", NESTED_PAGE_ID).await,
            nested_page_before
        );
        assert_eq!(
            row_state(&pool, "notes_blocks", NESTED_BLOCK_ID).await,
            nested_block_before
        );
        let restored_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(restored_text, "First version");
        let nested_stored_project: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM notes_pages WHERE id = ?")
                .bind(NESTED_PAGE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(nested_stored_project, None);
        let mut tx = pool.begin().await.unwrap();
        let nested_project = crate::structure::page_project_id_tx(&mut tx, NESTED_PAGE_ID)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(nested_project.as_deref(), Some(PROJECT_ID));
        assert_eq!(row_state(&pool, "notes_blocks", EXTRA_BLOCK_ID).await.1, 1);
        assert_eq!(row_state(&pool, "notes_pages", LATER_PAGE_ID).await.1, 1);
        let snapshot_kept: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_page_history_snapshots WHERE id = ? AND page_id = ?",
        )
        .bind(&snapshot_id)
        .bind(PAGE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(snapshot_kept, 1);
    });
}

#[test]
fn project_restore_nests_a_page_unnested_within_the_project_in_place_instead_of_copying_it() {
    crate::test_block_on(async {
        const NESTED_PAGE_ID: &str = "a1a1a1a1-a1a1-41a1-81a1-a1a1a1a1a1a1";
        const NESTED_BLOCK_ID: &str = "b1b1b1b1-b1b1-41b1-81b1-b1b1b1b1b1b1";
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        crate::writes::create_page(
            &pool,
            crate::models::NotePageCreate {
                id: NESTED_PAGE_ID.to_string(),
                title: "Nested".to_string(),
                parent: crate::NoteParent::PageId {
                    page_id: PAGE_ID.to_string(),
                },
                folder_id: None,
                project_id: None,
                first_block_id: NESTED_BLOCK_ID.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        let version = create_checkpoint(&pool, PROJECT_ID, "manual", None, None, "Nested")
            .await
            .unwrap()
            .unwrap();
        crate::writes::move_page(
            &pool,
            NESTED_PAGE_ID,
            crate::models::NoteMovePage {
                parent: crate::NoteParent::Workspace { workspace: true },
                folder_id: None,
            },
        )
        .await
        .unwrap();
        let unnested: (String, Option<String>) =
            sqlx::query_as("SELECT parent_type, project_id FROM notes_pages WHERE id = ?")
                .bind(NESTED_PAGE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            unnested,
            ("workspace".to_string(), Some(PROJECT_ID.to_string()))
        );
        // Trash retention purges the hidden `child_page` block, so the current graph keeps the
        // page while its paired block is gone.
        sqlx::query("DELETE FROM notes_blocks WHERE id = ? AND in_trash = 1")
            .bind(NESTED_PAGE_ID)
            .execute(&pool)
            .await
            .unwrap();
        let pages_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();

        let plan = restore::preview_restore(&pool, PROJECT_ID, &version.id)
            .await
            .unwrap();
        assert_eq!(plan.copy_count, 0);
        restore::restore_version(&pool, PROJECT_ID, &version.id)
            .await
            .unwrap();

        let pages_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(pages_after, pages_before);
        let restored: (String, Option<String>, Option<String>, i64) = sqlx::query_as(
            "SELECT parent_type, parent_page_id, project_id, in_trash
             FROM notes_pages WHERE id = ?",
        )
        .bind(NESTED_PAGE_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            restored,
            ("page_id".to_string(), Some(PAGE_ID.to_string()), None, 0)
        );
        let child_page_block: (String, String) =
            sqlx::query_as("SELECT type, page_id FROM notes_blocks WHERE id = ?")
                .bind(NESTED_PAGE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            child_page_block,
            ("child_page".to_string(), PAGE_ID.to_string())
        );
        let body_page: String = sqlx::query_scalar("SELECT page_id FROM notes_blocks WHERE id = ?")
            .bind(NESTED_BLOCK_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(body_page, NESTED_PAGE_ID);
    });
}
