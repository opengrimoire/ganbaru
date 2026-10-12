use super::helpers::*;

#[test]
fn collaboration_operations_track_comments_and_suggestions_for_future_sync() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    paragraph_payload("Alpha beta gamma"),
                )],
            },
        )
        .await
        .unwrap();

        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(block_parent(BLOCK_B)),
                discussion_id: None,
                anchor: Some(NoteCommentAnchorCreate {
                    start: 6,
                    end: 10,
                    text: "beta".to_string(),
                    prefix: "Alpha ".to_string(),
                    suffix: " gamma".to_string(),
                }),
                rich_text: vec![rich_text("First note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        let thread_id = thread_value["id"].as_str().unwrap().to_string();

        comments::update_comment(
            &pool,
            COMMENT_A,
            NoteCommentUpdate {
                rich_text: vec![rich_text("Edited note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        comments::resolve_comment_thread(&pool, &thread_id, true)
            .await
            .unwrap();
        comments::resolve_comment_thread(&pool, &thread_id, false)
            .await
            .unwrap();
        comments::delete_comment(&pool, COMMENT_A).await.unwrap();

        suggestions::create_suggestion(
            &pool,
            NoteSuggestionCreate {
                id: SUGGESTION_A.to_string(),
                block_id: BLOCK_B.to_string(),
                range_start: 6,
                range_end: 10,
                original_text: "beta".to_string(),
                proposed_text: "delta".to_string(),
                prefix: "Alpha ".to_string(),
                suffix: " gamma".to_string(),
            },
        )
        .await
        .unwrap();
        suggestions::reject_suggestion(&pool, SUGGESTION_A)
            .await
            .unwrap();

        let rows = sqlx::query(
            "SELECT
                entity_type,
                entity_id,
                operation_type,
                base_version,
                entity_version,
                conflict_policy,
                actor_display_name,
                payload
             FROM notes_collaboration_operations
             WHERE page_id = ?
             ORDER BY sequence ASC",
        )
        .bind(PAGE_A)
        .fetch_all(&pool)
        .await
        .unwrap();
        let operation_types: Vec<String> = rows
            .iter()
            .map(|row| row.try_get::<String, _>("operation_type").unwrap())
            .collect();
        assert_eq!(
            operation_types,
            vec![
                "comment_thread_create",
                "comment_create",
                "comment_update",
                "comment_thread_resolve",
                "comment_thread_reopen",
                "comment_delete",
                "suggestion_create",
                "suggestion_reject",
            ]
        );

        let versions: Vec<(i64, i64, String)> = rows
            .iter()
            .map(|row| {
                (
                    row.try_get::<i64, _>("base_version").unwrap(),
                    row.try_get::<i64, _>("entity_version").unwrap(),
                    row.try_get::<String, _>("conflict_policy").unwrap(),
                )
            })
            .collect();
        assert_eq!(
            versions,
            vec![
                (0, 1, "append_only".to_string()),
                (0, 1, "append_only".to_string()),
                (1, 2, "last_writer_wins".to_string()),
                (1, 2, "state_transition".to_string()),
                (2, 3, "state_transition".to_string()),
                (2, 3, "state_transition".to_string()),
                (0, 1, "append_only".to_string()),
                (1, 2, "state_transition".to_string()),
            ]
        );

        let thread_payload: serde_json::Value =
            serde_json::from_str(&rows[0].try_get::<String, _>("payload").unwrap()).unwrap();
        assert_eq!(thread_payload["anchor"]["text"], "beta");
        assert_eq!(thread_payload["parent"]["block_id"], BLOCK_B);

        let update_payload: serde_json::Value =
            serde_json::from_str(&rows[2].try_get::<String, _>("payload").unwrap()).unwrap();
        assert_eq!(update_payload["rich_text"][0]["plain_text"], "Edited note");

        let reject_payload: serde_json::Value =
            serde_json::from_str(&rows[7].try_get::<String, _>("payload").unwrap()).unwrap();
        assert_eq!(reject_payload["status"], "rejected");
        assert_eq!(reject_payload["original_text"], "beta");
        assert_eq!(reject_payload["proposed_text"], "delta");

        for row in rows {
            let display_name: serde_json::Value =
                serde_json::from_str(&row.try_get::<String, _>("actor_display_name").unwrap())
                    .unwrap();
            assert_eq!(display_name["resolved_name"], "You");
        }
    });
}
