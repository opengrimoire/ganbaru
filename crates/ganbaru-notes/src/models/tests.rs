use super::*;
use serde_json::json;

const PAGE_ID: &str = "11111111-1111-4111-8111-111111111111";
const BLOCK_ID: &str = "22222222-2222-4222-8222-222222222222";
const DATA_SOURCE_ID: &str = "33333333-3333-4333-8333-333333333333";

#[test]
fn parent_variants_keep_their_tagged_json_shape() {
    let cases = [
        (
            NoteParent::Workspace { workspace: true },
            json!({ "type": "workspace", "workspace": true }),
        ),
        (
            NoteParent::PageId {
                page_id: PAGE_ID.to_string(),
            },
            json!({ "type": "page_id", "page_id": PAGE_ID }),
        ),
        (
            NoteParent::BlockId {
                block_id: BLOCK_ID.to_string(),
            },
            json!({ "type": "block_id", "block_id": BLOCK_ID }),
        ),
        (
            NoteParent::DataSourceId {
                data_source_id: DATA_SOURCE_ID.to_string(),
            },
            json!({ "type": "data_source_id", "data_source_id": DATA_SOURCE_ID }),
        ),
    ];

    for (parent, expected) in cases {
        assert_eq!(serde_json::to_value(parent).unwrap(), expected);
    }
}

#[test]
fn block_json_omits_payload_fields_for_other_block_types() {
    let block = NoteBlockDto::new(NoteBlockRow {
        id: BLOCK_ID.to_string(),
        page_id: PAGE_ID.to_string(),
        parent_type: "page_id".to_string(),
        parent_page_id: Some(PAGE_ID.to_string()),
        parent_block_id: None,
        has_children: 0,
        in_trash: 0,
        block_type: "paragraph".to_string(),
        payload: json!({ "rich_text": [], "color": "default" }).to_string(),
        plain_text: String::new(),
        sort_order: 0.0,
        source_provider: None,
        source_object_id: None,
        source_last_edited_time: None,
        created_time: "2026-07-12T00:00:00Z".to_string(),
        last_edited_time: "2026-07-12T00:00:00Z".to_string(),
    })
    .unwrap();

    let value = serde_json::to_value(block).unwrap();
    assert_eq!(value["type"], "paragraph");
    assert_eq!(
        value["paragraph"],
        json!({ "rich_text": [], "color": "default" })
    );
    assert!(value.get("heading_1").is_none());
    assert!(value.get("image").is_none());
    assert!(value.get("unsupported").is_none());
}

#[test]
fn optional_json_value_preserves_unset_null_and_value_states() {
    let unset: NotePageUpdate = serde_json::from_value(json!({})).unwrap();
    let null: NotePageUpdate = serde_json::from_value(json!({ "icon": null })).unwrap();
    let value: NotePageUpdate =
        serde_json::from_value(json!({ "icon": { "type": "emoji", "emoji": "A" } })).unwrap();

    assert_eq!(unset.icon, OptionalJsonValue::Unset);
    assert_eq!(null.icon, OptionalJsonValue::Null);
    assert_eq!(
        value.icon,
        OptionalJsonValue::Value(json!({ "type": "emoji", "emoji": "A" }))
    );
}

#[test]
fn paginated_block_list_keeps_public_boundary_fields() {
    let value = serde_json::to_value(NotePaginatedBlockList::new(
        Vec::new(),
        Some(BLOCK_ID.to_string()),
        true,
    ))
    .unwrap();

    assert_eq!(
        value,
        json!({
            "object": "list",
            "type": "block",
            "block": {},
            "results": [],
            "next_cursor": BLOCK_ID,
            "has_more": true
        })
    );
}
