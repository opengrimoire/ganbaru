use super::helpers::*;

#[test]
fn aliases_resolve_local_links_backlinks_and_search() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let alias_url = "http://localhost:1420/?view=notes#notes?alias=Legacy%20Target";
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [linked_rich_text("Legacy Target", alias_url)],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let unresolved = links::list_unresolved_links(&pool, PAGE_A).await.unwrap();
        let unresolved_json = serde_json::to_value(&unresolved).unwrap();
        assert_eq!(unresolved_json.as_array().unwrap().len(), 1);
        assert_eq!(unresolved_json[0]["raw_target"], "Legacy Target");

        links::add_page_alias(
            &pool,
            PAGE_B,
            NotePageAliasCreate {
                id: BLOCK_D.to_string(),
                alias: "Legacy Target".to_string(),
            },
        )
        .await
        .unwrap();

        assert!(
            links::list_unresolved_links(&pool, PAGE_A)
                .await
                .unwrap()
                .is_empty()
        );
        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert!(backlinks_json.as_array().unwrap().iter().any(|backlink| {
            backlink["source_page"]["id"] == PAGE_A
                && backlink["source_block_id"] == BLOCK_C
                && backlink["reference_type"] == "link"
        }));
        let search_results = search::search(&pool, "Legacy Target", Some(10), false)
            .await
            .unwrap();
        let search_json = serde_json::to_value(search_results).unwrap();
        assert!(
            search_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["type"] == "page" && result["page"]["id"] == PAGE_B)
        );
    });
}

#[test]
fn resolving_unresolved_links_rewrites_source_urls() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let alias_url = "http://localhost:1420/?view=notes#notes?target=Missing%20Target";
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [linked_rich_text("Missing Target", alias_url)],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let unresolved = links::list_unresolved_links(&pool, PAGE_A).await.unwrap();
        let unresolved_json = serde_json::to_value(&unresolved).unwrap();
        let link_id = unresolved_json[0]["id"].as_str().unwrap();
        let remaining = links::resolve_unresolved_link(
            &pool,
            link_id,
            NoteUnresolvedLinkResolve {
                target_page_id: PAGE_B.to_string(),
            },
        )
        .await
        .unwrap();

        assert!(remaining.is_empty());
        let payload: String = sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_C)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(payload.contains(&format!("#notes?page={PAGE_B}")));
        assert!(!payload.contains("Missing%20Target"));

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert!(backlinks_json.as_array().unwrap().iter().any(|backlink| {
            backlink["source_page"]["id"] == PAGE_A
                && backlink["source_block_id"] == BLOCK_C
                && backlink["reference_type"] == "link"
        }));
    });
}
