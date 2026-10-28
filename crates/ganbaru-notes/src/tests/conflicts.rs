use super::helpers::*;
use crate::conflicts::{
    NotesConflictChoice, NotesConflictError, NotesConflictResolution, NotesConflictTarget,
    block_text_plain_text, conflicts_from_pool, property_value_text, resolve_from_pool,
    write_version_tx,
};
use ganbaru_sync::DeviceNames;
use ganbaru_sync_contracts::{Field, Value as SyncValue};

/// Writes a chosen version in its own transaction, as a resolution does before sealing.
async fn write_chosen(
    pool: &SqlitePool,
    target: NotesConflictTarget,
    page_id: &str,
    id: &str,
    value: &SyncValue,
) -> Result<(), NotesConflictError> {
    let mut tx = pool.begin().await.unwrap();
    write_version_tx(&mut tx, target, page_id, id, value).await?;
    tx.commit().await.unwrap();
    Ok(())
}

async fn stored_block(pool: &SqlitePool) -> (String, String, String) {
    sqlx::query_as("SELECT type, text, plain_text FROM notes_blocks WHERE id = ?")
        .bind(BLOCK_A)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn texts(fields: &[&str]) -> SyncValue {
    SyncValue::new(
        fields
            .iter()
            .map(|field| Field::Text((*field).to_string()))
            .collect(),
    )
    .unwrap()
}

#[test]
fn block_text_previews_join_rich_text_parts_and_table_cells() {
    let text = json!({
        "rich_text": [rich_text("Hello "), rich_text("world")],
        "caption": [rich_text("Caption")],
    })
    .to_string();
    let preview = block_text_plain_text(&text);
    assert!(preview.contains("Hello world"), "{preview}");
    assert!(preview.contains("Caption"), "{preview}");

    let cells = json!({ "cells": [[rich_text("a")], [rich_text("b")]] }).to_string();
    assert_eq!(block_text_plain_text(&cells), "a\tb");
    assert_eq!(block_text_plain_text("not json"), "");
}

#[test]
fn property_value_previews_read_names_and_scalars() {
    assert_eq!(property_value_text(&json!(3)), "3");
    assert_eq!(
        property_value_text(&json!([{ "name": "Red" }, { "name": "Blue" }])),
        "Red, Blue"
    );
    assert_eq!(
        property_value_text(&json!({ "start": "2026-10-27" })),
        "2026-10-27"
    );
    assert_eq!(property_value_text(&json!(null)), "");
}

#[test]
fn a_vault_that_does_not_sync_reads_no_conflicts_and_refuses_resolutions() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let devices = DeviceNames::new("device", std::collections::HashMap::new());
        let read = conflicts_from_pool(&pool, "vault", &devices, PAGE_A)
            .await
            .expect("read conflicts of an unlinked vault");
        let read = serde_json::to_value(read).unwrap();
        assert_eq!(read["pageId"], PAGE_A);
        assert_eq!(read["conflicts"], json!([]));

        let refused = resolve_from_pool(
            &pool,
            "vault",
            NotesConflictResolution {
                page_id: PAGE_A.to_string(),
                target: NotesConflictTarget::BlockText,
                id: BLOCK_A.to_string(),
                choice: NotesConflictChoice::Displayed,
            },
            1,
        )
        .await;
        assert!(matches!(refused, Err(NotesConflictError::Resolved(_))));
    });
}

#[test]
fn a_resolution_finds_each_field_only_on_its_own_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_mismatched_rows(&pool).await;
        let thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Mine")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_id = serde_json::to_value(thread).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let fields = [
            (NotesConflictTarget::PageTitle, PAGE_A, PAGE_A.to_string()),
            (NotesConflictTarget::BlockText, PAGE_A, BLOCK_A.to_string()),
            (
                NotesConflictTarget::PropertyValue,
                PAGE_B,
                "score".to_string(),
            ),
            (NotesConflictTarget::ThreadStatus, PAGE_A, thread_id),
            (
                NotesConflictTarget::CommentBody,
                PAGE_A,
                COMMENT_A.to_string(),
            ),
        ];
        for (target, page_id, id) in fields {
            let resolve = |page_id: &str| {
                resolve_from_pool(
                    &pool,
                    "vault",
                    NotesConflictResolution {
                        page_id: page_id.to_string(),
                        target,
                        id: id.clone(),
                        choice: NotesConflictChoice::Displayed,
                    },
                    1,
                )
            };
            let found = resolve(page_id).await;
            assert!(
                matches!(&found, Err(NotesConflictError::Resolved(message)) if message.contains("does not sync")),
                "{target:?}: {found:?}"
            );
            let other_page = if page_id == PAGE_A { PAGE_C } else { PAGE_A };
            let elsewhere = resolve(other_page).await;
            assert!(
                matches!(&elsewhere, Err(NotesConflictError::Resolved(message)) if message.contains("gone")),
                "{target:?}: {elsewhere:?}"
            );
        }
    });
}

#[test]
fn a_block_version_with_the_same_parts_in_another_order_is_written_as_is() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        sqlx::query("UPDATE notes_blocks SET text = ? WHERE id = ?")
            .bind(json!({ "rich_text": [], "caption": [] }).to_string())
            .bind(BLOCK_A)
            .execute(&pool)
            .await
            .unwrap();
        let version = format!(
            "{{\"caption\":[{}],\"rich_text\":[{}]}}",
            rich_text("Caption"),
            rich_text("Body")
        );

        write_chosen(
            &pool,
            NotesConflictTarget::BlockText,
            PAGE_A,
            BLOCK_A,
            &texts(&[&version]),
        )
        .await
        .unwrap();

        let (_, text, _) = stored_block(&pool).await;
        let text: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(text["rich_text"][0]["plain_text"], "Body", "{text}");
    });
}

#[test]
fn a_chosen_title_and_block_text_are_written_as_local_edits() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        write_chosen(
            &pool,
            NotesConflictTarget::PageTitle,
            PAGE_A,
            PAGE_A,
            &texts(&["Theirs"]),
        )
        .await
        .unwrap();
        let title: String = sqlx::query_scalar("SELECT title FROM notes_pages WHERE id = ?")
            .bind(PAGE_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "Theirs");

        let text = json!({ "rich_text": [rich_text("Their paragraph")] }).to_string();
        write_chosen(
            &pool,
            NotesConflictTarget::BlockText,
            PAGE_A,
            BLOCK_A,
            &texts(&[&text]),
        )
        .await
        .unwrap();
        let plain: String = sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(plain, "Their paragraph");
    });
}

#[test]
fn a_chosen_thread_status_and_comment_body_are_written_as_local_edits() {
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
                rich_text: vec![rich_text("Mine")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let thread_id = serde_json::to_value(thread).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        write_chosen(
            &pool,
            NotesConflictTarget::ThreadStatus,
            PAGE_A,
            &thread_id,
            &texts(&["resolved"]),
        )
        .await
        .unwrap();
        let status: String =
            sqlx::query_scalar("SELECT status FROM notes_comment_threads WHERE id = ?")
                .bind(&thread_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "resolved");

        let body = json!([rich_text("Theirs")]).to_string();
        write_chosen(
            &pool,
            NotesConflictTarget::CommentBody,
            PAGE_A,
            COMMENT_A,
            &texts(&[&body, "Theirs", "[]"]),
        )
        .await
        .unwrap();
        let plain: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_comments WHERE id = ?")
                .bind(COMMENT_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain, "Theirs");
    });
}

#[test]
fn a_block_version_that_no_longer_fits_the_block_type_is_written_as_plain_text() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let cells = json!({ "cells": [[rich_text("left")], [rich_text("right")]] }).to_string();

        write_chosen(
            &pool,
            NotesConflictTarget::BlockText,
            PAGE_A,
            BLOCK_A,
            &texts(&[&cells]),
        )
        .await
        .expect("a table row version falls back to plain text in a paragraph");

        let (block_type, text, plain) = stored_block(&pool).await;
        assert_eq!(block_type, "paragraph");
        assert_eq!(plain, "left\tright");
        let text: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(text.get("cells").is_none(), "{text}");
    });
}

#[test]
fn a_block_version_for_a_block_without_rich_text_is_refused_as_unfit() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        sqlx::query(
            "UPDATE notes_blocks SET type = 'divider', attrs = '{}', text = '{}' WHERE id = ?",
        )
        .bind(BLOCK_A)
        .execute(&pool)
        .await
        .unwrap();
        let version = json!({ "rich_text": [rich_text("Theirs")] }).to_string();

        let refused = write_chosen(
            &pool,
            NotesConflictTarget::BlockText,
            PAGE_A,
            BLOCK_A,
            &texts(&[&version]),
        )
        .await;

        assert!(
            matches!(refused, Err(NotesConflictError::Unfit(_))),
            "{refused:?}"
        );
    });
}

async fn seed_mismatched_rows(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    create_database(
        pool,
        DATABASE_A,
        DATA_SOURCE_A,
        DATABASE_VIEW_A,
        "Tasks",
        BLOCK_A,
    )
    .await;
    let mut schema = stored_schema(pool, DATA_SOURCE_A).await;
    schema["Score"] = json!({"id": "score", "name": "Score", "type": "number", "number": {}});
    store_schema(pool, DATA_SOURCE_A, &schema).await;
    for (page_id, text) in [(PAGE_B, "42"), (PAGE_C, "not a number")] {
        insert_row_page(
            pool,
            page_id,
            DATA_SOURCE_A,
            "Row",
            json!({"Score": {"id": "score", "type": "rich_text", "rich_text": [rich_text(text)]}}),
        )
        .await;
    }
}

async fn stored_score(pool: &SqlitePool, page_id: &str) -> Option<(String, String)> {
    sqlx::query_as(
        "SELECT value_type, value FROM notes_page_property_values
         WHERE page_id = ? AND property_id = 'score'",
    )
    .bind(page_id)
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[test]
fn a_mismatched_value_converts_to_the_property_type_or_its_empty_value() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_mismatched_rows(&pool).await;
        assert_eq!(
            stored_score(&pool, PAGE_B)
                .await
                .map(|(value_type, _)| value_type),
            Some("rich_text".to_string())
        );

        data_sources::settle_mismatched_value(&pool, PAGE_B, "score", true)
            .await
            .unwrap();
        let (value_type, value) = stored_score(&pool, PAGE_B).await.unwrap();
        assert_eq!(value_type, "number");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&value).unwrap(),
            json!(42.0)
        );

        data_sources::settle_mismatched_value(&pool, PAGE_C, "score", true)
            .await
            .unwrap();
        let converted = stored_score(&pool, PAGE_C).await;
        assert!(
            converted
                .as_ref()
                .is_none_or(|(value_type, value)| value_type == "number" && value == "null"),
            "{converted:?}"
        );
    });
}

#[test]
fn a_mismatched_value_preview_tells_when_converting_drops_the_value() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_mismatched_rows(&pool).await;
        let preview = |page_id| data_sources::preview_mismatched_value(&pool, page_id, "score");

        assert!(!preview(PAGE_B).await.unwrap().conversion_loses_data);
        assert!(preview(PAGE_C).await.unwrap().conversion_loses_data);

        let mut schema = stored_schema(&pool, DATA_SOURCE_A).await;
        schema["Score"] = json!({
            "id": "score",
            "name": "Score",
            "type": "select",
            "select": { "options": [{ "id": "kept", "name": "42", "color": "blue" }] },
        });
        store_schema(&pool, DATA_SOURCE_A, &schema).await;
        assert!(!preview(PAGE_B).await.unwrap().conversion_loses_data);
        assert!(preview(PAGE_C).await.unwrap().conversion_loses_data);

        data_sources::settle_mismatched_value(&pool, PAGE_C, "score", false)
            .await
            .unwrap();
        assert!(!preview(PAGE_C).await.unwrap().conversion_loses_data);
    });
}

#[test]
fn a_discarded_mismatched_value_is_removed_and_a_matching_value_is_kept() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_mismatched_rows(&pool).await;
        data_sources::settle_mismatched_value(&pool, PAGE_B, "score", false)
            .await
            .unwrap();
        assert_eq!(stored_score(&pool, PAGE_B).await, None);

        data_sources::settle_mismatched_value(&pool, PAGE_C, "score", true)
            .await
            .unwrap();
        let before = stored_score(&pool, PAGE_C).await;
        data_sources::settle_mismatched_value(&pool, PAGE_C, "score", false)
            .await
            .unwrap();
        assert_eq!(stored_score(&pool, PAGE_C).await, before);

        let refused = data_sources::settle_mismatched_value(&pool, PAGE_A, "score", true).await;
        assert!(refused.is_err());
    });
}
