//! Listing and resolving Notes page conflicts through the domain commands between a hub and a
//! phone.

use std::collections::HashMap;

use super::projects_recovery::Pair;
use super::{HUB_DEVICE, NOW_MS, PHONE_DEVICE, Replica, VAULT_ID, block_on};
use ganbaru_notes::conflicts::{
    NotesConflictError, NotesConflictResolution, conflicts_from_pool, resolve_from_pool,
};
use ganbaru_sync::DeviceNames;
use serde_json::{Value, json};

const PAGE: &str = "00000000-0000-4000-8000-0000000000a1";
const OTHER_PAGE: &str = "00000000-0000-4000-8000-0000000000a2";
const BLOCK: &str = "00000000-0000-4000-8000-0000000000b1";
const CREATED: &str = "2026-10-28T00:00:00.000Z";

/// Stored block text holding one plain run of `content`.
fn block_text(content: &str) -> String {
    json!({
        "rich_text": [{
            "type": "text",
            "text": { "content": content, "link": null },
            "annotations": {
                "bold": false,
                "italic": false,
                "strikethrough": false,
                "underline": false,
                "code": false,
                "color": "default",
            },
            "plain_text": content,
            "href": null,
        }]
    })
    .to_string()
}

/// A workspace page holding one paragraph, and an empty second page.
fn page_sql() -> String {
    format!(
        "INSERT INTO notes_pages (id, parent_type, title, created_time, last_edited_time)
         VALUES ('{PAGE}', 'workspace', 'Shared', '{CREATED}', '{CREATED}'),
                ('{OTHER_PAGE}', 'workspace', 'Other', '{CREATED}', '{CREATED}');
         INSERT INTO notes_blocks
             (id, page_id, parent_type, parent_page_id, type, text, plain_text, order_key,
              created_time, last_edited_time)
         VALUES ('{BLOCK}', '{PAGE}', 'page_id', '{PAGE}', 'paragraph', '{text}', 'Base', 'a0',
                 '{CREATED}', '{CREATED}');",
        text = block_text("Base"),
    )
}

fn edit_block_sql(content: &str) -> String {
    format!(
        "UPDATE notes_blocks SET text = '{text}', plain_text = '{content}' WHERE id = '{BLOCK}'",
        text = block_text(content),
    )
}

/// The Notes conflicts of the shared page on `replica`, as the frontend receives them.
async fn conflicts(replica: &Replica, device: &str) -> Value {
    let read = conflicts_from_pool(
        &replica.pool,
        VAULT_ID,
        &DeviceNames::new(device, HashMap::new()),
        PAGE,
    )
    .await
    .expect("read conflicts");
    serde_json::to_value(read).expect("serialize conflicts")
}

fn request(page_id: &str, choice: Value) -> NotesConflictResolution {
    serde_json::from_value(json!({
        "pageId": page_id,
        "target": "block_text",
        "id": BLOCK,
        "choice": choice,
    }))
    .expect("resolution request")
}

/// Fills the block's hook-derived page on a replica that received it, as the Notes apply hook
/// does in the app; this harness applies without domain hooks. The column is not captured, so
/// the write stays local.
async fn fill_block_page(replica: &Replica) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE notes_blocks SET page_id = '{PAGE}' WHERE id = '{BLOCK}';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;"
    )))
    .execute(&replica.pool)
    .await
    .expect("fill block page");
}

/// A pair sharing the page, with the paragraph edited concurrently on both devices.
async fn pair_with_block_conflict(label: &str) -> Pair {
    let mut pair = Pair::new(label).await;
    pair.hub_edit(&page_sql()).await;
    pair.converge().await;
    fill_block_page(&pair.phone).await;
    pair.hub_edit(&edit_block_sql("Hub text")).await;
    pair.phone_edit(&edit_block_sql("Phone text")).await;
    pair.converge().await;
    pair
}

/// The single block text conflict, the same on both replicas.
async fn single_conflict(pair: &Pair) -> Value {
    let hub = conflicts(&pair.hub, HUB_DEVICE).await;
    let phone = conflicts(&pair.phone, PHONE_DEVICE).await;
    let list = phone["conflicts"].as_array().expect("conflicts");
    assert_eq!(list.len(), 1, "{phone}");
    assert_eq!(hub["conflicts"].as_array().map(Vec::len), Some(1), "{hub}");
    let conflict = list[0].clone();
    assert_eq!(conflict["target"], "block_text");
    assert_eq!(conflict["id"], BLOCK);
    let displayed = |read: &Value| {
        read["conflicts"][0]["versions"]
            .as_array()
            .expect("versions")
            .iter()
            .find(|version| version["displayed"] == true)
            .map(|version| version["version"].clone())
    };
    assert_eq!(displayed(&hub), displayed(&phone));
    conflict
}

#[test]
fn a_block_text_conflict_resolved_with_the_hidden_version_writes_it_everywhere() {
    block_on(async {
        let mut pair = pair_with_block_conflict("notes-conflict-block-version").await;
        let conflict = single_conflict(&pair).await;
        let versions = conflict["versions"].as_array().expect("versions");
        let mut texts = versions
            .iter()
            .map(|version| version["text"].as_str().expect("text").to_string())
            .collect::<Vec<_>>();
        texts.sort();
        assert_eq!(texts, ["Hub text", "Phone text"]);
        let hidden = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");
        let chosen = hidden["text"].as_str().expect("text").to_string();

        let refused = resolve_from_pool(
            &pair.hub.pool,
            VAULT_ID,
            request(
                OTHER_PAGE,
                json!({ "kind": "version", "version": hidden["version"] }),
            ),
            NOW_MS + 1,
        )
        .await;
        assert!(matches!(refused, Err(NotesConflictError::Resolved(_))));

        resolve_from_pool(
            &pair.hub.pool,
            VAULT_ID,
            request(
                PAGE,
                json!({ "kind": "version", "version": hidden["version"] }),
            ),
            NOW_MS + 1,
        )
        .await
        .expect("resolve conflict");
        pair.hub.seal().await;
        pair.converge().await;

        for (replica, device) in [(&pair.hub, HUB_DEVICE), (&pair.phone, PHONE_DEVICE)] {
            assert_eq!(conflicts(replica, device).await["conflicts"], json!([]));
        }
        let stored = pair
            .texts(format!(
                "SELECT text FROM notes_blocks WHERE id = '{BLOCK}'"
            ))
            .await;
        assert_eq!(stored.len(), 1);
        assert!(stored[0].contains(&chosen), "{stored:?}");
        pair.finish().await;
    });
}

#[test]
fn a_block_text_conflict_kept_as_displayed_closes_everywhere() {
    block_on(async {
        let mut pair = pair_with_block_conflict("notes-conflict-block-displayed").await;
        let conflict = single_conflict(&pair).await;
        let displayed = conflict["versions"]
            .as_array()
            .expect("versions")
            .iter()
            .find(|version| version["displayed"] == true)
            .expect("displayed version")["text"]
            .as_str()
            .expect("text")
            .to_string();

        resolve_from_pool(
            &pair.phone.pool,
            VAULT_ID,
            request(PAGE, json!({ "kind": "displayed" })),
            NOW_MS + 1,
        )
        .await
        .expect("resolve conflict");
        pair.phone.seal().await;
        pair.converge().await;

        for (replica, device) in [(&pair.hub, HUB_DEVICE), (&pair.phone, PHONE_DEVICE)] {
            assert_eq!(conflicts(replica, device).await["conflicts"], json!([]));
        }
        let stored = pair
            .texts(format!(
                "SELECT text FROM notes_blocks WHERE id = '{BLOCK}'"
            ))
            .await;
        assert!(stored[0].contains(&displayed), "{stored:?}");
        pair.finish().await;
    });
}
