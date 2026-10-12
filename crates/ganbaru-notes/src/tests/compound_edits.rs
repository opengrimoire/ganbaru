use super::helpers::*;
use crate::writes::compound::{NoteCompoundEdit, apply_compound_edit};
use serde_json::Value;

const OPERATION: &str = "abababab-abab-4bab-8bab-abababababab";

async fn block_json(pool: &SqlitePool, id: &str) -> Value {
    serde_json::to_value(reads::get_block(pool, id, true).await.unwrap()).unwrap()
}

async fn revision(pool: &SqlitePool, id: &str) -> String {
    reads::get_block_row(pool, id, true)
        .await
        .unwrap()
        .edit_revision()
        .unwrap()
}

async fn split_request(pool: &SqlitePool) -> Value {
    json!({
        "operation_id": OPERATION, "page_id": PAGE_A, "kind": "split",
        "expected_blocks": { BLOCK_A: revision(pool, BLOCK_A).await },
        "operations": [
            { "type": "update", "block_id": BLOCK_A, "update": { "paragraph": paragraph_payload("prefix") } },
            { "type": "append", "request": { "parent": page_parent(PAGE_A), "after": BLOCK_A,
                "children": [{ "id": BLOCK_B, "type": "paragraph", "paragraph": paragraph_payload("suffix") }] } }
        ]
    })
}

async fn apply(pool: &SqlitePool, value: Value) -> Result<Value, String> {
    let request: NoteCompoundEdit = serde_json::from_value(value).unwrap();
    apply_compound_edit(pool, request)
        .await
        .map(|result| serde_json::to_value(result).unwrap())
}

#[test]
fn compound_edit_template_and_button_copy_canonical_unloaded_children_once() {
    crate::test_block_on(async {
        for (kind, payload) in [
            ("template", template_payload("Insert")),
            ("button", button_payload("Insert")),
        ] {
            let pool = migrated_memory_pool().await;
            create_page(&pool, PAGE_A, BLOCK_A).await;
            writes::update_block(&pool, BLOCK_A, block_update(kind, payload))
                .await
                .unwrap();
            writes::append_block_children(
                &pool,
                NoteAppendBlockChildren {
                    parent: block_parent(BLOCK_A),
                    after: None,
                    children: vec![
                        block(BLOCK_B, "paragraph", paragraph_payload("First")),
                        block(BLOCK_C, "paragraph", paragraph_payload("Unloaded root")),
                    ],
                },
            )
            .await
            .unwrap();
            writes::append_block_children(
                &pool,
                NoteAppendBlockChildren {
                    parent: block_parent(BLOCK_B),
                    after: None,
                    children: vec![block(
                        BLOCK_D,
                        "paragraph",
                        paragraph_payload("Unloaded descendant"),
                    )],
                },
            )
            .await
            .unwrap();
            let request = json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "template",
                "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
                "operations": [{ "type": "duplicate_children", "source_block_id": BLOCK_A, "request": {
                    "parent": page_parent(PAGE_A), "after": BLOCK_A, "before": null,
                    "block_ids": [BLOCK_B], "duplicated_block_ids": [{ "source_id": BLOCK_B, "duplicate_id": BLOCK_E }],
                } }],
            });
            let receipt = apply(&pool, request.clone()).await.unwrap();
            assert_eq!(apply(&pool, request).await.unwrap(), receipt);
            let rows = receipt["blocks"].as_array().unwrap();
            let copies: Vec<_> = rows.iter().filter(|row| row["id"] != BLOCK_A).collect();
            assert_eq!(copies.len(), 3);
            let roots: Vec<_> = copies
                .iter()
                .filter(|row| row["parent"]["page_id"] == PAGE_A)
                .collect();
            assert_eq!(roots.len(), 2);
            assert_eq!(roots[0]["id"], BLOCK_E);
            assert_eq!(roots[0]["paragraph"]["rich_text"][0]["plain_text"], "First");
            assert_eq!(
                roots[1]["paragraph"]["rich_text"][0]["plain_text"],
                "Unloaded root"
            );
            let descendant = copies
                .iter()
                .find(|row| row["parent"]["block_id"] == BLOCK_E)
                .unwrap();
            assert_eq!(
                descendant["paragraph"]["rich_text"][0]["plain_text"],
                "Unloaded descendant"
            );
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                count, 7,
                "a lost response cannot insert a second template copy"
            );
        }
    });
}

#[test]
fn compound_edit_template_rejects_hidden_child_pages_and_rolls_back_prior_writes() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("template", template_payload("Original")),
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    paragraph_payload("Hidden parent"),
                )],
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.into(),
                title: "Hidden child page".into(),
                parent: block_parent(BLOCK_B),
                folder_id: None,
                first_block_id: BLOCK_C.into(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        let before = revision(&pool, BLOCK_A).await;
        let error = apply(&pool, json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "template",
            "expected_blocks": { BLOCK_A: before },
            "operations": [
                { "type": "update", "block_id": BLOCK_A, "update": { "template": template_payload("Attempted edit") } },
                { "type": "duplicate_children", "source_block_id": BLOCK_A, "request": {
                    "parent": page_parent(PAGE_A), "after": BLOCK_A, "before": null,
                    "block_ids": [], "duplicated_block_ids": [],
                } },
            ],
        })).await.unwrap_err();
        assert!(error.contains("cannot contain child pages"), "{error}");
        assert_eq!(revision(&pool, BLOCK_A).await, before);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let blocks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(blocks, 4);
    });
}

#[test]
fn compound_edit_template_rejects_an_oversized_canonical_child_graph() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("template", template_payload("Insert")),
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: (0..100)
                    .map(|index| {
                        block(
                            &format!("12345678-1234-4234-8234-{index:012}"),
                            "paragraph",
                            paragraph_payload("Child"),
                        )
                    })
                    .collect(),
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    paragraph_payload("Over the limit"),
                )],
            },
        )
        .await
        .unwrap();
        let error = apply(&pool, json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "template",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "duplicate_children", "source_block_id": BLOCK_A, "request": {
                "parent": page_parent(PAGE_A), "after": BLOCK_A, "before": null,
                "block_ids": [], "duplicated_block_ids": [],
            } }],
        })).await.unwrap_err();
        assert!(error.contains("between 1 and 100"), "{error}");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 102);
        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
    });
}

#[test]
fn compound_edit_rejects_foreign_revision_even_when_no_operation_targets_it() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_C).await;
        let before = revision(&pool, BLOCK_A).await;
        let mut request = split_request(&pool).await;
        request["expected_blocks"][BLOCK_C] = json!(revision(&pool, BLOCK_C).await);
        assert!(apply(&pool, request).await.is_err());
        assert_eq!(revision(&pool, BLOCK_A).await, before);
        assert!(reads::get_block(&pool, BLOCK_B, true).await.is_err());
    });
}

#[test]
fn compound_edit_returns_canonical_hidden_descendants_after_trash() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("hidden"))],
            },
        )
        .await
        .unwrap();
        let child_before = revision(&pool, BLOCK_B).await;
        let result = apply(
            &pool,
            json!({
                "operation_id": OPERATION, "page_id": PAGE_A, "kind": "delete_selection",
                "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
                "operations": [{ "type": "trash", "block_id": BLOCK_A, "in_trash": true }],
            }),
        )
        .await
        .unwrap();
        let child = result["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == BLOCK_B)
            .expect("the native descendant changed even though the editor did not load it");
        assert_eq!(child["in_trash"], true);
        assert_ne!(child["edit_revision"], child_before);
        assert_eq!(child["edit_revision"], revision(&pool, BLOCK_B).await);
    });
}

#[test]
fn compound_edit_returns_the_source_parent_revision_after_moving_its_last_child() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("child"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("anchor"))],
            },
        )
        .await
        .unwrap();
        let before = revision(&pool, BLOCK_A).await;
        let result = apply(&pool, json!({
            "operation_id": OPERATION, "page_id": PAGE_A, "kind": "indent_selection",
            "expected_blocks": { BLOCK_B: revision(&pool, BLOCK_B).await, BLOCK_C: revision(&pool, BLOCK_C).await },
            "operations": [{ "type": "move", "block_id": BLOCK_B,
                "request": { "parent": page_parent(PAGE_A), "after": BLOCK_C, "before": null } }],
        })).await.unwrap();
        let parent = result["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == BLOCK_A)
            .expect(
                "the native source parent must be reconciled without a client-supplied revision",
            );
        assert_eq!(parent["has_children"], false);
        assert_ne!(parent["edit_revision"], before);
        assert_eq!(parent["edit_revision"], revision(&pool, BLOCK_A).await);
    });
}

#[test]
fn compound_edit_rolls_back_every_prefix_and_its_receipt() {
    crate::test_block_on(async {
        for failure_stage in 0..=2 {
            let pool = migrated_memory_pool().await;
            create_page(&pool, PAGE_A, BLOCK_A).await;
            let before =
                serde_json::to_value(reads::get_block(&pool, BLOCK_A, true).await.unwrap())
                    .unwrap();
            let mut request = split_request(&pool).await;
            request["operations"].as_array_mut().unwrap().insert(failure_stage,
                json!({ "type": "update", "block_id": BLOCK_C, "update": { "paragraph": paragraph_payload("missing") } }));
            assert!(apply(&pool, request).await.is_err());
            let after = serde_json::to_value(reads::get_block(&pool, BLOCK_A, true).await.unwrap())
                .unwrap();
            assert_eq!(
                before, after,
                "failure after {failure_stage} completed operations"
            );
            assert!(reads::get_block(&pool, BLOCK_B, true).await.is_err());
            let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_edit_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(receipts, 0);
        }
    });
}

#[test]
fn compound_edit_rejects_stale_and_missing_revisions_and_cross_page_writes() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_C).await;
        let stale = split_request(&pool).await;
        writes::update_block(
            &pool,
            BLOCK_A,
            serde_json::from_value(json!({ "paragraph": paragraph_payload("newer") })).unwrap(),
        )
        .await
        .unwrap();
        assert!(apply(&pool, stale).await.unwrap_err().contains("conflict"));
        let mut missing = split_request(&pool).await;
        missing["expected_blocks"] = json!({});
        assert!(
            apply(&pool, missing)
                .await
                .unwrap_err()
                .contains("requires the revision")
        );
        let mut cross_page = split_request(&pool).await;
        cross_page["operations"][1]["request"]["parent"] = json!(page_parent(PAGE_B));
        assert!(
            apply(&pool, cross_page)
                .await
                .unwrap_err()
                .contains("another page")
        );
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_A, false)
                .await
                .unwrap()
                .plain_text,
            "newer"
        );
    });
}

#[test]
fn compound_edit_returns_original_receipt_after_lost_response_and_newer_edit() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let request = split_request(&pool).await;
        let committed = apply(&pool, request.clone()).await.unwrap();
        writes::update_block(
            &pool,
            BLOCK_A,
            serde_json::from_value(json!({ "paragraph": paragraph_payload("newer") })).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(apply(&pool, request.clone()).await.unwrap(), committed);
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_A, false)
                .await
                .unwrap()
                .plain_text,
            "newer"
        );
        let mut reused = request;
        reused["operations"][0]["update"]["paragraph"] = paragraph_payload("different");
        assert!(
            apply(&pool, reused)
                .await
                .unwrap_err()
                .contains("identity was reused")
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_B)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    });
}

#[test]
fn compound_edit_trash_preserves_unloaded_descendants_and_undo_restores_them() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("hidden"))],
            },
        )
        .await
        .unwrap();
        let request = json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "delete_selection",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "trash", "block_id": BLOCK_A, "in_trash": true }] });
        apply(&pool, request).await.unwrap();
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_B, true)
                .await
                .unwrap()
                .in_trash,
            1
        );
        let undo = json!({ "operation_id": "bcbcbcbc-bcbc-4cbc-8cbc-bcbcbcbcbcbc", "page_id": PAGE_A, "kind": "undo",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "trash", "block_id": BLOCK_A, "in_trash": false }] });
        apply(&pool, undo).await.unwrap();
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_B, false)
                .await
                .unwrap()
                .plain_text,
            "hidden"
        );
    });
}

#[test]
fn compound_edit_requires_every_canonical_column_sibling_before_resizing() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_block(&pool, BLOCK_A, block_update("column_list", json!({})))
            .await
            .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![
                    block(BLOCK_B, "column", column_payload(Some(0.5))),
                    block(BLOCK_C, "column", column_payload(Some(0.5))),
                ],
            },
        )
        .await
        .unwrap();
        let before = block_json(&pool, BLOCK_B).await;
        let mut request = json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "column_layout",
            "expected_blocks": { BLOCK_B: revision(&pool, BLOCK_B).await },
            "operations": [{ "type": "update", "block_id": BLOCK_B, "update": { "column": column_payload(Some(0.4)) } }],
        });
        assert!(
            apply(&pool, request.clone())
                .await
                .unwrap_err()
                .contains("all canonical column siblings")
        );
        assert_eq!(block_json(&pool, BLOCK_B).await, before);
        request["expected_blocks"][BLOCK_C] = json!(revision(&pool, BLOCK_C).await);
        request["operations"].as_array_mut().unwrap().push(json!({ "type": "update", "block_id": BLOCK_C, "update": { "column": column_payload(Some(0.6)) } }));
        apply(&pool, request).await.unwrap();
        assert_eq!(
            block_json(&pool, BLOCK_B).await["column"]["width_ratio"],
            0.4
        );
        assert_eq!(
            block_json(&pool, BLOCK_C).await["column"]["width_ratio"],
            0.6
        );
    });
}

#[test]
fn compound_edit_table_columns_preserve_unloaded_rich_text_and_rollback_every_row() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_block(&pool, BLOCK_A, block_update("table", table_payload(2)))
            .await
            .unwrap();
        let annotation = annotated_rich_text("Hidden", "blue");
        writes::append_block_children(&pool, NoteAppendBlockChildren {
            parent: block_parent(BLOCK_A), after: None,
            children: vec![block(BLOCK_B, "table_row", table_row_payload(&["Visible", "Two"])),
                block(BLOCK_C, "table_row", json!({ "cells": [[annotation.clone()], [rich_text("Tail")]], "ganbaru_indent": 0 }))],
        }).await.unwrap();
        let originals = [
            block_json(&pool, BLOCK_A).await,
            block_json(&pool, BLOCK_B).await,
            block_json(&pool, BLOCK_C).await,
        ];
        let mut request = json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "table_columns",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "table_column", "table_id": BLOCK_A, "index": 1, "insert": true, "empty_row_id": BLOCK_D },
                { "type": "update", "block_id": BLOCK_E, "update": { "paragraph": paragraph_payload("Invalid") } }],
        });
        assert!(apply(&pool, request.clone()).await.is_err());
        for (id, original) in [BLOCK_A, BLOCK_B, BLOCK_C].iter().zip(originals) {
            assert_eq!(block_json(&pool, id).await, original);
        }
        request["operations"].as_array_mut().unwrap().pop();
        let result = apply(&pool, request).await.unwrap();
        assert_eq!(block_json(&pool, BLOCK_A).await["table"]["table_width"], 3);
        let hidden_before = result["before_blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == BLOCK_C)
            .unwrap();
        assert_eq!(
            hidden_before["table_row"]["cells"],
            json!([[annotation.clone()], [rich_text("Tail")]])
        );
        let hidden = block_json(&pool, BLOCK_C).await;
        assert_eq!(
            hidden["table_row"]["cells"],
            json!([[annotation], [], [rich_text("Tail")]])
        );
        assert!(
            result["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == BLOCK_C)
        );
        let undo = json!({ "operation_id": "bcbcbcbc-bcbc-4cbc-8cbc-bcbcbcbcbcbc", "page_id": PAGE_A, "kind": "table_columns",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "table_column", "table_id": BLOCK_A, "index": 1, "insert": false, "empty_row_id": BLOCK_D }],
        });
        apply(&pool, undo).await.unwrap();
        assert_eq!(block_json(&pool, BLOCK_A).await["table"]["table_width"], 2);
        assert_eq!(
            block_json(&pool, BLOCK_C).await["table_row"]["cells"][1],
            json!([rich_text("Tail")])
        );
    });
}

#[test]
fn compound_edit_moves_unloaded_children_before_removing_a_layout_item() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.into()),
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    paragraph_payload("Destination"),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("Hidden child"),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_C),
                after: None,
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Hidden grandchild"),
                )],
            },
        )
        .await
        .unwrap();
        let result = apply(&pool, json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "tab_layout",
            "expected_blocks": { BLOCK_A: revision(&pool, BLOCK_A).await, BLOCK_B: revision(&pool, BLOCK_B).await },
            "operations": [
                { "type": "move_children", "source_block_id": BLOCK_A, "parent": block_parent(BLOCK_B), "after": null },
                { "type": "trash", "block_id": BLOCK_A, "in_trash": true },
            ],
        })).await.unwrap();
        assert_eq!(block_json(&pool, BLOCK_A).await["in_trash"], true);
        assert_eq!(
            block_json(&pool, BLOCK_C).await["parent"]["block_id"],
            BLOCK_B
        );
        assert_eq!(
            block_json(&pool, BLOCK_D).await["parent"]["block_id"],
            BLOCK_C
        );
        assert_eq!(block_json(&pool, BLOCK_D).await["in_trash"], false);
        assert!(
            result["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == BLOCK_D)
        );
    });
}

#[test]
fn compound_edit_receipt_survives_closing_and_reopening_the_database() {
    crate::test_block_on(async {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-notes-compound-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let request = split_request(&pool).await;
        let receipt = apply(&pool, request.clone()).await.unwrap();
        pool.close().await;
        let reopened = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        writes::update_block(
            &reopened,
            BLOCK_A,
            block_update("paragraph", paragraph_payload("Later session")),
        )
        .await
        .unwrap();
        assert_eq!(apply(&reopened, request).await.unwrap(), receipt);
        assert_eq!(
            reads::get_block_row(&reopened, BLOCK_A, false)
                .await
                .unwrap()
                .plain_text,
            "Later session"
        );
        let copies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_B)
            .fetch_one(&reopened)
            .await
            .unwrap();
        assert_eq!(copies, 1);
        reopened.close().await;
        std::fs::remove_file(path).unwrap();
    });
}

#[test]
fn compound_edit_mixed_page_and_database_copies_rollback_as_one_graph() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_C, BLOCK_C).await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.into(),
                title: "Source child".into(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_B.into(),
                after_block_id: Some(BLOCK_A.into()),
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("paragraph", paragraph_payload("Child body")),
        )
        .await
        .unwrap();
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Source database",
            BLOCK_A,
        )
        .await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: COMMENT_A.into(),
                title: "Owned row".into(),
                first_block_id: BLOCK_E.into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        let before_counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM notes_pages), (SELECT COUNT(*) FROM notes_blocks), (SELECT COUNT(*) FROM notes_databases)").fetch_one(&pool).await.unwrap();
        let mut request = json!({ "operation_id": OPERATION, "page_id": PAGE_C, "kind": "paste",
            "expected_blocks": { BLOCK_C: revision(&pool, BLOCK_C).await },
            "operations": [
                { "type": "duplicate", "request": { "block_ids": [PAGE_B], "duplicated_block_ids": [{ "source_id": PAGE_B, "duplicate_id": BLOCK_D }], "parent": page_parent(PAGE_C), "after": BLOCK_C, "before": null } },
                { "type": "copy_database", "request": { "id": DATABASE_B, "source_block_id": DATABASE_A, "parent": page_parent(PAGE_C), "after_block_id": BLOCK_D } },
                { "type": "update", "block_id": BLOCK_F, "update": { "paragraph": paragraph_payload("Invalid") } },
            ],
        });
        assert!(apply(&pool, request.clone()).await.is_err());
        let after_counts = sqlx::query_as::<_, (i64, i64, i64)>("SELECT (SELECT COUNT(*) FROM notes_pages), (SELECT COUNT(*) FROM notes_blocks), (SELECT COUNT(*) FROM notes_databases)").fetch_one(&pool).await.unwrap();
        assert_eq!(before_counts, after_counts);
        assert!(reads::get_block(&pool, BLOCK_D, true).await.is_err());
        request["operations"].as_array_mut().unwrap().pop();
        let result = apply(&pool, request.clone()).await.unwrap();
        assert_eq!(apply(&pool, request).await.unwrap(), result);
        assert_eq!(
            block_json(&pool, BLOCK_D).await["child_page"]["title"],
            "Source child"
        );
        assert_eq!(result["databases"][0]["block"]["id"], DATABASE_B);
        assert_ne!(result["databases"][0]["data_source"]["id"], DATA_SOURCE_A);
        let copied_source = result["databases"][0]["data_source"]["id"]
            .as_str()
            .unwrap();
        let row_title: String =
            sqlx::query_scalar("SELECT title FROM notes_pages WHERE parent_data_source_id = ?")
                .bind(copied_source)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row_title, "Owned row");
    });
}

#[test]
fn compound_edit_explicit_page_move_reverses_only_its_reviewed_graph() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_C).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Moved"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Hidden descendant"),
                )],
            },
        )
        .await
        .unwrap();
        let old_revision = revision(&pool, BLOCK_B).await;
        let moved = apply(&pool, json!({ "operation_id": OPERATION, "page_id": PAGE_A, "kind": "indent_selection",
            "expected_blocks": { BLOCK_B: old_revision, BLOCK_C: revision(&pool, BLOCK_C).await },
            "operations": [{ "type": "move_between_pages", "block_id": BLOCK_B, "source_page_id": PAGE_A, "destination_page_id": PAGE_B,
                "request": { "parent": page_parent(PAGE_B), "after": BLOCK_C, "before": null } }],
        })).await.unwrap();
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_D, false)
                .await
                .unwrap()
                .page_id,
            PAGE_B
        );
        assert!(
            moved["before_blocks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == BLOCK_D)
        );
        writes::update_block(
            &pool,
            BLOCK_C,
            block_update("paragraph", paragraph_payload("Unrelated later edit")),
        )
        .await
        .unwrap();
        let mut undo = json!({ "operation_id": "bcbcbcbc-bcbc-4cbc-8cbc-bcbcbcbcbcbc", "page_id": PAGE_A, "kind": "undo",
            "expected_blocks": { BLOCK_B: old_revision, BLOCK_A: revision(&pool, BLOCK_A).await },
            "operations": [{ "type": "move_between_pages", "block_id": BLOCK_B, "source_page_id": PAGE_B, "destination_page_id": PAGE_A,
                "request": { "parent": block_parent(BLOCK_A), "after": null, "before": null } }],
        });
        assert!(
            apply(&pool, undo.clone())
                .await
                .unwrap_err()
                .contains("conflict")
        );
        undo["expected_blocks"][BLOCK_B] = json!(revision(&pool, BLOCK_B).await);
        let receipt = apply(&pool, undo.clone()).await.unwrap();
        assert_eq!(apply(&pool, undo).await.unwrap(), receipt);
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_D, false)
                .await
                .unwrap()
                .page_id,
            PAGE_A
        );
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_C, false)
                .await
                .unwrap()
                .plain_text,
            "Unrelated later edit"
        );
    });
}
