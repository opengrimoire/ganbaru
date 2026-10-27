use super::helpers::*;

#[test]
fn comments_create_reply_resolve_reopen_and_delete() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Page note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        let thread_id = thread_value["id"].as_str().unwrap().to_string();
        assert_eq!(thread_value["parent"]["type"], "page_id");
        assert_eq!(
            thread_value["comments"][0]["rich_text"][0]["plain_text"],
            "Page note"
        );

        let replied = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_B.to_string(),
                parent: None,
                discussion_id: Some(thread_id.clone()),
                anchor: None,
                rich_text: vec![rich_text("Reply")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let replied_value = serde_json::to_value(replied).unwrap();
        assert_eq!(replied_value["comments"].as_array().unwrap().len(), 2);

        let updated = comments::update_comment(
            &pool,
            COMMENT_B,
            NoteCommentUpdate {
                rich_text: vec![rich_text("Edited reply")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let updated_value = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated_value["comments"][1]["rich_text"][0]["plain_text"],
            "Edited reply"
        );

        let resolved = comments::resolve_comment_thread(&pool, &thread_id, true)
            .await
            .unwrap();
        let resolved_value = serde_json::to_value(resolved).unwrap();
        assert_eq!(resolved_value["status"], "resolved");
        assert!(resolved_value["resolved_at"].is_string());

        assert!(
            comments::list_comments(&pool, PAGE_A, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            comments::list_comments(&pool, PAGE_A, true)
                .await
                .unwrap()
                .len(),
            1
        );

        let reopened = comments::resolve_comment_thread(&pool, &thread_id, false)
            .await
            .unwrap();
        let reopened_value = serde_json::to_value(reopened).unwrap();
        assert_eq!(reopened_value["status"], "open");
        assert!(reopened_value["resolved_at"].is_null());

        let after_delete = comments::delete_comment(&pool, COMMENT_A).await.unwrap();
        let after_delete_value = serde_json::to_value(after_delete).unwrap();
        assert_eq!(after_delete_value["comments"].as_array().unwrap().len(), 1);
        assert_eq!(
            after_delete_value["comments"][0]["rich_text"][0]["plain_text"],
            "Edited reply"
        );
    });
}

#[test]
fn comment_unread_state_tracks_local_identity() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let local_user_json =
            serde_json::to_value(local_user::load_local_user(&pool).await.unwrap()).unwrap();
        let local_user_id = local_user_json["id"].as_str().unwrap().to_string();
        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Remote note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        let thread_id = thread_value["id"].as_str().unwrap().to_string();
        assert_eq!(thread_value["unread"], false);

        sqlx::query("DELETE FROM notes_comment_thread_reads WHERE thread_id = ? AND user_id = ?")
            .bind(&thread_id)
            .bind(&local_user_id)
            .execute(&pool)
            .await
            .unwrap();
        // Authorship and creation time are immutable, so the remote comment replaces the row.
        let (rich_text_json, plain_text): (String, String) =
            sqlx::query_as("SELECT rich_text, plain_text FROM notes_comments WHERE id = ?")
                .bind(COMMENT_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        sqlx::query("DELETE FROM notes_comments WHERE id = ?")
            .bind(COMMENT_A)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO notes_comments (
                 id, thread_id, rich_text, plain_text, created_by, display_name, created_time,
                 last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, '2000-01-01T00:00:00.000Z', '2000-01-01T00:00:00.000Z')",
        )
        .bind(COMMENT_A)
        .bind(&thread_id)
        .bind(rich_text_json)
        .bind(plain_text)
        .bind("99999999-9999-4999-8999-999999999999")
        .bind(json!({"type": "user", "resolved_name": "Reviewer"}).to_string())
        .execute(&pool)
        .await
        .unwrap();

        let unread = comments::list_comments(&pool, PAGE_A, false).await.unwrap();
        let unread_value = serde_json::to_value(unread).unwrap();
        assert_eq!(unread_value[0]["unread"], true);

        let marked = comments::mark_comment_threads_read(
            &pool,
            NoteCommentThreadReadUpdate {
                page_id: PAGE_A.to_string(),
                discussion_ids: vec![thread_id.clone()],
                include_resolved: Some(false),
            },
        )
        .await
        .unwrap();
        let marked_value = serde_json::to_value(marked).unwrap();
        assert_eq!(marked_value[0]["unread"], false);

        let replied = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_B.to_string(),
                parent: None,
                discussion_id: Some(thread_id.clone()),
                anchor: None,
                rich_text: vec![rich_text("Local follow-up")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let replied_value = serde_json::to_value(replied).unwrap();
        assert_eq!(replied_value["unread"], false);

        sqlx::query(
            "UPDATE notes_comments
             SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 second')
             WHERE id = ?",
        )
        .bind(COMMENT_A)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE notes_comment_threads
             SET last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 second')
             WHERE id = ?",
        )
        .bind(&thread_id)
        .execute(&pool)
        .await
        .unwrap();

        let edited = comments::list_comments(&pool, PAGE_A, false).await.unwrap();
        let edited_value = serde_json::to_value(edited).unwrap();
        assert_eq!(edited_value[0]["unread"], true);
    });
}

#[test]
fn block_comments_attach_to_visible_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Anchored"))],
            },
        )
        .await
        .unwrap();

        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_C.to_string(),
                parent: Some(block_parent(BLOCK_B)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Block note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        assert_eq!(thread_value["parent"]["type"], "block_id");
        assert_eq!(thread_value["block_id"], BLOCK_B);

        let threads = comments::list_comments(&pool, PAGE_A, false).await.unwrap();
        assert_eq!(threads.len(), 1);

        writes::trash_block(&pool, BLOCK_B, true).await.unwrap();
        assert!(
            comments::list_comments(&pool, PAGE_A, false)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn inline_comment_anchors_persist_on_block_threads() {
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
                rich_text: vec![rich_text("Inline note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        let thread_id = thread_value["id"].as_str().unwrap();
        assert_eq!(thread_value["parent"]["type"], "block_id");
        assert_eq!(thread_value["anchor"]["type"], "text_range");
        assert_eq!(thread_value["anchor"]["block_id"], BLOCK_B);
        assert_eq!(thread_value["anchor"]["start"], 6);
        assert_eq!(thread_value["anchor"]["end"], 10);
        assert_eq!(thread_value["anchor"]["text"], "beta");

        let listed = comments::list_comments(&pool, PAGE_A, false).await.unwrap();
        let listed_value = serde_json::to_value(listed).unwrap();
        assert_eq!(listed_value[0]["anchor"]["text"], "beta");

        let reply_with_anchor = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_B.to_string(),
                parent: None,
                discussion_id: Some(thread_id.to_string()),
                anchor: Some(NoteCommentAnchorCreate {
                    start: 0,
                    end: 4,
                    text: "beta".to_string(),
                    prefix: String::new(),
                    suffix: String::new(),
                }),
                rich_text: vec![rich_text("Reply")],
                attachments: None,
            },
        )
        .await;
        assert_eq!(
            reply_with_anchor.err().as_deref(),
            Some("inline comment anchors can only start new block comment threads")
        );
    });
}

#[test]
fn block_comments_and_suggestions_follow_a_block_moved_to_another_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_C).await;
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
        comments::create_comment(
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
                rich_text: vec![rich_text("Inline note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        suggestions::create_suggestion(
            &pool,
            NoteSuggestionCreate {
                id: COMMENT_B.to_string(),
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

        writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: page_parent(PAGE_B),
                after: None,
                before: None,
            },
        )
        .await
        .unwrap();

        for table in [
            "notes_comment_threads",
            "notes_comment_thread_anchors",
            "notes_suggestions",
        ] {
            let pages: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT DISTINCT page_id FROM {table}"
            )))
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(pages, vec![PAGE_B.to_string()], "{table}");
        }
    });
}

#[test]
fn comment_reactions_toggle_per_person_and_group_by_emoji() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Looks good")],
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
        let reacted = comments::toggle_comment_reaction(&pool, COMMENT_A, " 🎉 ")
            .await
            .unwrap();
        let reactions = serde_json::to_value(reacted).unwrap()["comments"][0]["reactions"].clone();
        assert_eq!(reactions[0]["emoji"], "👍");
        assert_eq!(reactions[0]["count"], 1);
        assert_eq!(reactions[0]["reacted_by_me"], true);
        assert_eq!(reactions[0]["names"], json!(["You"]));
        assert_eq!(reactions[1]["emoji"], "🎉");

        sqlx::query(
            "INSERT INTO notes_comment_reactions
                 (comment_id, created_by, display_name, emoji, sync_key)
             VALUES (?, 'person-b', '{\"type\":\"user\",\"resolved_name\":\"Bea\"}', '👍', ?)",
        )
        .bind(COMMENT_A)
        .bind(
            crate::sync_keys::COMMENT_REACTIONS
                .key(&[COMMENT_A, "person-b", "👍"])
                .unwrap(),
        )
        .execute(&pool)
        .await
        .unwrap();
        let removed = comments::toggle_comment_reaction(&pool, COMMENT_A, "👍")
            .await
            .unwrap();
        let reactions = serde_json::to_value(removed).unwrap()["comments"][0]["reactions"].clone();
        assert_eq!(reactions[0]["emoji"], "🎉");
        assert_eq!(reactions[1]["emoji"], "👍");
        assert_eq!(reactions[1]["count"], 1);
        assert_eq!(reactions[1]["reacted_by_me"], false);
        assert_eq!(reactions[1]["names"], json!(["Bea"]));

        for invalid in ["", "  ", "ok", "👍 👍", "\u{7}", "12"] {
            assert_eq!(
                comments::toggle_comment_reaction(&pool, COMMENT_A, invalid)
                    .await
                    .err()
                    .as_deref(),
                Some("comment reaction must be one emoji"),
                "{invalid:?} should be rejected"
            );
        }
        comments::toggle_comment_reaction(&pool, COMMENT_A, "1️⃣")
            .await
            .unwrap();

        comments::resolve_comment_thread(&pool, &thread_id, true)
            .await
            .unwrap();
        assert_eq!(
            comments::toggle_comment_reaction(&pool, COMMENT_A, "🎉")
                .await
                .err()
                .as_deref(),
            Some("resolved comment threads cannot receive reactions")
        );
        comments::resolve_comment_thread(&pool, &thread_id, false)
            .await
            .unwrap();

        comments::delete_comment(&pool, COMMENT_A).await.unwrap();
        assert_eq!(
            comments::toggle_comment_reaction(&pool, COMMENT_A, "🎉")
                .await
                .err()
                .as_deref(),
            Some("notes comment not found")
        );
    });
}

#[test]
fn comment_reactions_limit_distinct_emoji_per_comment() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Many reactions")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let emoji = [
            "😀", "😁", "😂", "🤣", "😃", "😄", "😅", "😆", "😉", "😊", "😋", "😎", "😍", "😘",
            "🥰", "😗", "😙", "😚", "🙂", "🤗",
        ];
        for value in emoji {
            comments::toggle_comment_reaction(&pool, COMMENT_A, value)
                .await
                .unwrap();
        }
        assert!(
            comments::toggle_comment_reaction(&pool, COMMENT_A, "🤩")
                .await
                .err()
                .is_some_and(|error| error.contains("limited to 20 different reactions"))
        );
        comments::toggle_comment_reaction(&pool, COMMENT_A, "😀")
            .await
            .unwrap();
        comments::toggle_comment_reaction(&pool, COMMENT_A, "🤩")
            .await
            .unwrap();
    });
}

#[test]
fn comment_thread_mute_is_per_person_state() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Quiet please")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_value = serde_json::to_value(thread).unwrap();
        assert_eq!(thread_value["muted"], false);
        let thread_id = thread_value["id"].as_str().unwrap().to_string();

        let muted = comments::set_comment_thread_muted(&pool, &thread_id, true)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(muted).unwrap()["muted"], true);
        comments::set_comment_thread_muted(&pool, &thread_id, true)
            .await
            .unwrap();
        let listed = comments::list_comments(&pool, PAGE_A, false).await.unwrap();
        assert_eq!(serde_json::to_value(listed).unwrap()[0]["muted"], true);

        let unmuted = comments::set_comment_thread_muted(&pool, &thread_id, false)
            .await
            .unwrap();
        assert_eq!(serde_json::to_value(unmuted).unwrap()["muted"], false);
        assert!(
            comments::set_comment_thread_muted(&pool, PAGE_B, true)
                .await
                .is_err()
        );
    });
}
