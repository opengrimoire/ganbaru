use super::*;

#[test]
fn project_restore_removes_later_notes_and_keeps_a_safety_version() {
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
        let later_page_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages WHERE id = ?")
                .bind(LATER_PAGE_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(later_page_count, 0);
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
