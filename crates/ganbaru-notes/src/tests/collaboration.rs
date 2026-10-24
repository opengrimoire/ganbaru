use super::helpers::*;

async fn set_profile_display_name(pool: &sqlx::SqlitePool, name: &str) {
    sqlx::query(
        "INSERT INTO vault_preferences (key, value, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(local_user::PROFILE_DISPLAY_NAME_PREFERENCE_KEY)
    .bind(serde_json::to_string(name).unwrap())
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn local_authors_store_no_name_and_follow_the_profile_preference() {
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

        let local_user_json =
            serde_json::to_value(local_user::load_local_user(&pool).await.unwrap()).unwrap();
        let local_user_id = local_user_json["id"].as_str().unwrap().to_string();
        assert_eq!(local_user_json["display_name"], "You");
        assert!(local_user_json.get("last_edited_time").is_none());

        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(block_parent(BLOCK_B)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("First note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_id = serde_json::to_value(thread).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        comments::toggle_comment_reaction(&pool, COMMENT_A, "👍")
            .await
            .unwrap();
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

        let stored_names: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT display_name FROM notes_comments
             UNION ALL SELECT display_name FROM notes_comment_reactions
             UNION ALL SELECT display_name FROM notes_suggestions",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(stored_names, vec![None, None, None]);

        set_profile_display_name(&pool, " Alice ").await;
        let listed =
            serde_json::to_value(comments::list_comments(&pool, PAGE_A, false).await.unwrap())
                .unwrap();
        let comment = &listed[0]["comments"][0];
        assert_eq!(comment["created_by"]["id"], local_user_id);
        assert_eq!(comment["display_name"]["resolved_name"], "Alice");
        assert_eq!(comment["reactions"][0]["names"][0], "Alice");
        let suggestion = serde_json::to_value(
            suggestions::list_suggestions(&pool, PAGE_A, false)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(suggestion[0]["display_name"]["resolved_name"], "Alice");
        assert_eq!(
            serde_json::to_value(local_user::load_local_user(&pool).await.unwrap()).unwrap()["display_name"],
            "Alice"
        );

        let imported_name = r#"{"type":"user","resolved_name":"Imported author"}"#;
        sqlx::query("UPDATE notes_comments SET display_name = ? WHERE id = ?")
            .bind(imported_name)
            .bind(COMMENT_A)
            .execute(&pool)
            .await
            .unwrap();
        set_profile_display_name(&pool, "   ").await;
        let resolved = serde_json::to_value(
            comments::resolve_comment_thread(&pool, &thread_id, true)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(resolved["resolved_by"]["id"], local_user_id);
        assert_eq!(
            resolved["comments"][0]["display_name"]["resolved_name"],
            "Imported author"
        );
        assert_eq!(resolved["comments"][0]["reactions"][0]["names"][0], "You");
    });
}
