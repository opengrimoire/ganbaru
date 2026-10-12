use super::helpers::*;

#[test]
fn suggestions_preserve_range_content_and_decision_state() {
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

        let suggestion = suggestions::create_suggestion(
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
        let suggestion_value = serde_json::to_value(suggestion).unwrap();
        assert_eq!(suggestion_value["object"], "suggestion");
        assert_eq!(suggestion_value["status"], "open");
        assert_eq!(suggestion_value["page_id"], PAGE_A);
        assert_eq!(suggestion_value["block_id"], BLOCK_B);
        assert_eq!(suggestion_value["range_start"], 6);
        assert_eq!(suggestion_value["range_end"], 10);
        assert_eq!(suggestion_value["original_text"], "beta");
        assert_eq!(suggestion_value["proposed_text"], "delta");
        assert_eq!(suggestion_value["display_name"]["resolved_name"], "You");
        assert!(suggestion_value["created_time"].is_string());

        let listed = suggestions::list_suggestions(&pool, PAGE_A, false)
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);

        let rejected = suggestions::reject_suggestion(&pool, SUGGESTION_A)
            .await
            .unwrap();
        let rejected_value = serde_json::to_value(rejected).unwrap();
        assert_eq!(rejected_value["status"], "rejected");
        assert!(rejected_value["rejected_at"].is_string());
        assert!(rejected_value["rejected_by"]["id"].is_string());
        assert!(
            suggestions::list_suggestions(&pool, PAGE_A, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            suggestions::list_suggestions(&pool, PAGE_A, true)
                .await
                .unwrap()
                .len(),
            1
        );

        let accepted = suggestions::create_suggestion(
            &pool,
            NoteSuggestionCreate {
                id: SUGGESTION_B.to_string(),
                block_id: BLOCK_B.to_string(),
                range_start: 11,
                range_end: 16,
                original_text: "gamma".to_string(),
                proposed_text: "omega".to_string(),
                prefix: " beta ".to_string(),
                suffix: String::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(serde_json::to_value(accepted).unwrap()["status"], "open");
        let accepted = suggestions::accept_suggestion(&pool, SUGGESTION_B)
            .await
            .unwrap();
        let accepted_value = serde_json::to_value(accepted).unwrap();
        assert_eq!(accepted_value["status"], "accepted");
        assert!(accepted_value["accepted_at"].is_string());
        assert!(accepted_value["accepted_by"]["id"].is_string());

        let missing_original = suggestions::create_suggestion(
            &pool,
            NoteSuggestionCreate {
                id: "60606060-6060-4060-8060-606060606060".to_string(),
                block_id: BLOCK_B.to_string(),
                range_start: 0,
                range_end: 7,
                original_text: "missing".to_string(),
                proposed_text: "present".to_string(),
                prefix: String::new(),
                suffix: String::new(),
            },
        )
        .await;
        assert_eq!(
            missing_original.err().as_deref(),
            Some("suggestion original text must exist in the block")
        );
    });
}
