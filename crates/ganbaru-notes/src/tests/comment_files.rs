use super::helpers::*;

#[test]
fn comment_attachment_asset_references_follow_local_file_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let first_asset =
            "notes/files/f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1.pdf";
        let second_asset =
            "notes/files/f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2.png";
        let first_attachment = local_comment_attachment(
            first_asset,
            "application/pdf",
            84,
            "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1",
            "brief.pdf",
        );
        let second_attachment = local_comment_attachment(
            second_asset,
            "image/png",
            42,
            "f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2",
            "diagram.png",
        );

        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("See attachment")],
                attachments: Some(vec![first_attachment.clone(), first_attachment]),
            },
        )
        .await
        .unwrap();

        let first_reference_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_asset_references
             WHERE asset_id = ?
               AND owner_type = 'comment'
               AND owner_id = ?
               AND page_id = ?
               AND comment_id = ?
               AND role = 'comment_attachment'",
        )
        .bind(first_asset)
        .bind(COMMENT_A)
        .bind(PAGE_A)
        .bind(COMMENT_A)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(first_reference_count, 1);

        comments::update_comment(
            &pool,
            COMMENT_A,
            NoteCommentUpdate {
                rich_text: vec![rich_text("Updated attachment")],
                attachments: Some(vec![second_attachment]),
            },
        )
        .await
        .unwrap();
        let old_reference_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ?")
                .bind(first_asset)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(old_reference_count, 0);

        let replacement_reference_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ?")
                .bind(second_asset)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(replacement_reference_count, 1);

        comments::update_comment(
            &pool,
            COMMENT_A,
            NoteCommentUpdate {
                rich_text: vec![rich_text("Text only edit")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let preserved_reference_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ?")
                .bind(second_asset)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(preserved_reference_count, 1);

        comments::delete_comment(&pool, COMMENT_A).await.unwrap();
        let remaining_references: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_asset_references
             WHERE owner_type = 'comment' AND owner_id = ?",
        )
        .bind(COMMENT_A)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_references, 0);

        let retained_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained_assets, 2);
    });
}

#[test]
fn attachment_comments_may_omit_text_but_plain_comments_may_not() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let asset =
            "notes/files/f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3.png";
        let attachment = local_comment_attachment(
            asset,
            "image/png",
            12,
            "f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3",
            "sketch.png",
        );

        let empty_without_attachment = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: Vec::new(),
                attachments: None,
            },
        )
        .await;
        assert!(empty_without_attachment.is_err());

        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: Vec::new(),
                attachments: Some(vec![attachment.clone()]),
            },
        )
        .await
        .unwrap();
        let thread = serde_json::to_value(thread).unwrap();
        assert_eq!(thread["comments"][0]["rich_text"], serde_json::json!([]));
        assert_eq!(
            thread["comments"][0]["attachments"][0]["name"],
            "sketch.png"
        );

        let removed_attachment = comments::update_comment(
            &pool,
            COMMENT_A,
            NoteCommentUpdate {
                rich_text: Vec::new(),
                attachments: Some(Vec::new()),
            },
        )
        .await;
        assert!(removed_attachment.is_err());

        let captioned = comments::update_comment(
            &pool,
            COMMENT_A,
            NoteCommentUpdate {
                rich_text: vec![rich_text("Caption")],
                attachments: Some(vec![attachment]),
            },
        )
        .await
        .unwrap();
        let captioned = serde_json::to_value(captioned).unwrap();
        assert_eq!(
            captioned["comments"][0]["rich_text"][0]["plain_text"],
            "Caption"
        );
    });
}
