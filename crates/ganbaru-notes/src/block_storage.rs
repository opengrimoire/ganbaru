//! Stored shape of Notes blocks.
//!
//! A block's API payload is composed from three stored parts: `text` holds the type's text keys
//! (`rich_text`, `cells`, `caption`), `attrs` holds every other key, and the device-local
//! `notes_block_view_state` row holds view state (`ganbaru_open`, `editing_locked`). A
//! `child_page` block's `title` is never stored on the block; reads take it from the page that
//! shares the block's id. Splitting text from attributes keeps a concurrent type change and a
//! concurrent text edit from overwriting each other.

use serde_json::{Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

/// Payload keys stored in `notes_blocks.text`.
pub const BLOCK_TEXT_KEYS: &[&str] = &["rich_text", "cells", "caption"];
/// Payload key for a toggle's open state, stored per device.
pub const BLOCK_VIEW_OPEN_KEY: &str = "ganbaru_open";
/// Payload key for a database block's editing lock, stored per device.
pub const BLOCK_VIEW_EDITING_LOCKED_KEY: &str = "editing_locked";
/// Largest encoded `text` a block may store.
pub const MAX_BLOCK_TEXT_BYTES: usize = 256 * 1024;
/// Largest encoded `attrs` a block may store.
pub const MAX_BLOCK_ATTRS_BYTES: usize = 64 * 1024;

const CHILD_PAGE_TITLE_KEY: &str = "title";

/// Select list that reads every stored column a [`crate::models::NoteBlockRow`] needs from a
/// `notes_blocks` row aliased as `$alias`.
///
/// The list derives `has_children` with `EXISTS`, the view state from `notes_block_view_state`,
/// and the `child_page` title from the page that shares the block's id.
#[macro_export]
#[doc(hidden)]
macro_rules! note_block_row_columns {
    ($alias:literal) => {
        concat!(
            $alias, ".id, ",
            $alias, ".page_id, ",
            $alias, ".parent_type, ",
            $alias, ".parent_page_id, ",
            $alias, ".parent_block_id, ",
            "EXISTS (SELECT 1 FROM notes_blocks AS has_child WHERE has_child.parent_block_id = ",
            $alias, ".id AND has_child.in_trash = 0) AS has_children, ",
            $alias, ".in_trash, ",
            $alias, ".type AS block_type, ",
            $alias, ".attrs, ",
            $alias, ".text, ",
            "(SELECT view_state.open FROM notes_block_view_state AS view_state WHERE view_state.block_id = ",
            $alias, ".id) AS view_open, ",
            "(SELECT view_state.editing_locked FROM notes_block_view_state AS view_state WHERE view_state.block_id = ",
            $alias, ".id) AS view_editing_locked, ",
            "CASE WHEN ", $alias, ".type = 'child_page' THEN COALESCE((SELECT child.title FROM notes_pages AS child WHERE child.id = ",
            $alias, ".id), '') END AS child_page_title, ",
            $alias, ".plain_text, ",
            $alias, ".order_key, ",
            $alias, ".source_provider, ",
            $alias, ".source_object_id, ",
            $alias, ".source_last_edited_time, ",
            $alias, ".created_time, ",
            $alias, ".last_edited_time"
        )
    };
}

/// SQL expression for the content payload of a `notes_blocks` row aliased as `$alias`: its
/// `attrs` and `text` merged, without view state or a `child_page` title. Scans of links,
/// mentions, and assets read this; API reads use [`note_block_row_columns`].
#[macro_export]
#[doc(hidden)]
macro_rules! note_block_content_sql {
    ($alias:literal) => {
        concat!("json_patch(", $alias, ".attrs, ", $alias, ".text)")
    };
}

/// Device-local view state of one block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockViewState {
    /// Whether a toggle is open, when the payload names it.
    pub open: Option<bool>,
    /// Whether a database block is locked for editing, when the payload names it.
    pub editing_locked: Option<bool>,
}

impl BlockViewState {
    fn is_empty(self) -> bool {
        self.open.is_none() && self.editing_locked.is_none()
    }
}

/// The stored parts of one block payload.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredBlockContent {
    /// Encoded JSON object of the non-text keys.
    pub attrs: String,
    /// Encoded JSON object of the text keys.
    pub text: String,
    /// Device-local view state.
    pub view: BlockViewState,
}

/// Splits an API payload into its stored parts and checks the stored size bounds.
///
/// A `child_page` title is dropped because the page owns it.
pub fn split_block_payload(
    block_type: &str,
    payload: &Value,
) -> Result<StoredBlockContent, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| format!("{block_type} payload must be an object"))?;
    let mut attrs = Map::new();
    let mut text = Map::new();
    let mut view = BlockViewState::default();
    for (key, value) in object {
        match key.as_str() {
            BLOCK_VIEW_OPEN_KEY => {
                view.open = Some(
                    value
                        .as_bool()
                        .ok_or_else(|| format!("{block_type}.{key} must be a boolean"))?,
                );
            }
            BLOCK_VIEW_EDITING_LOCKED_KEY => {
                view.editing_locked = Some(
                    value
                        .as_bool()
                        .ok_or_else(|| format!("{block_type}.{key} must be a boolean"))?,
                );
            }
            CHILD_PAGE_TITLE_KEY if block_type == "child_page" => {}
            key if BLOCK_TEXT_KEYS.contains(&key) => {
                text.insert(key.to_string(), value.clone());
            }
            _ => {
                attrs.insert(key.clone(), value.clone());
            }
        }
    }
    let attrs = Value::Object(attrs).to_string();
    let text = Value::Object(text).to_string();
    check_block_content_bounds(block_type, &attrs, &text)?;
    Ok(StoredBlockContent { attrs, text, view })
}

/// Refuses stored parts above the block bounds.
pub fn check_block_content_bounds(block_type: &str, attrs: &str, text: &str) -> Result<(), String> {
    if text.len() > MAX_BLOCK_TEXT_BYTES {
        return Err(format!(
            "{block_type} text is {} bytes, above the {MAX_BLOCK_TEXT_BYTES} byte limit",
            text.len()
        ));
    }
    if attrs.len() > MAX_BLOCK_ATTRS_BYTES {
        return Err(format!(
            "{block_type} attributes are {} bytes, above the {MAX_BLOCK_ATTRS_BYTES} byte limit",
            attrs.len()
        ));
    }
    Ok(())
}

/// Composes the API payload from the stored parts.
pub fn compose_block_payload(
    block_type: &str,
    attrs: &str,
    text: &str,
    view: BlockViewState,
    child_page_title: Option<&str>,
) -> Result<Value, String> {
    let mut payload = parse_object(attrs, block_type, "attrs")?;
    for (key, value) in parse_object(text, block_type, "text")? {
        payload.insert(key, value);
    }
    if let Some(open) = view.open {
        payload.insert(BLOCK_VIEW_OPEN_KEY.to_string(), Value::Bool(open));
    }
    if let Some(locked) = view.editing_locked {
        payload.insert(
            BLOCK_VIEW_EDITING_LOCKED_KEY.to_string(),
            Value::Bool(locked),
        );
    }
    if block_type == "child_page" {
        payload.insert(
            CHILD_PAGE_TITLE_KEY.to_string(),
            Value::String(child_page_title.unwrap_or_default().to_string()),
        );
    }
    Ok(Value::Object(payload))
}

/// Room left in [`MAX_BLOCK_TEXT_BYTES`] for the `{"rich_text":[...]}` wrapper and the other text
/// keys a block may carry next to its rich text.
const RICH_TEXT_WRAPPER_BYTES: usize = 1024;

/// Splits a rich text array into consecutive chunks whose encoded `text` fits the block bound.
///
/// Items stay whole when they fit. A text item larger than the bound is cut at character
/// boundaries into several text items with the same annotations and link. Any other item larger
/// than the bound is refused, since it has no text to cut.
pub fn split_rich_text_to_bounds(rich_text: &[Value]) -> Result<Vec<Vec<Value>>, String> {
    let budget = MAX_BLOCK_TEXT_BYTES - RICH_TEXT_WRAPPER_BYTES;
    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 0;
    for item in rich_text {
        for piece in split_rich_text_item(item, budget)? {
            let piece_bytes = encoded_len(&piece) + 1;
            if !current.is_empty() && current_bytes + piece_bytes > budget {
                chunks.push(std::mem::take(&mut current));
                current_bytes = 0;
            }
            current_bytes += piece_bytes;
            current.push(piece);
        }
    }
    if !current.is_empty() || chunks.is_empty() {
        chunks.push(current);
    }
    Ok(chunks)
}

fn encoded_len(value: &Value) -> usize {
    value.to_string().len()
}

fn split_rich_text_item(item: &Value, budget: usize) -> Result<Vec<Value>, String> {
    if encoded_len(item) <= budget {
        return Ok(vec![item.clone()]);
    }
    let content = item
        .get("text")
        .and_then(|text| text.get("content"))
        .and_then(Value::as_str)
        .filter(|_| item.get("type").and_then(Value::as_str) == Some("text"))
        .ok_or_else(|| {
            format!("a rich text item is above the {MAX_BLOCK_TEXT_BYTES} byte block text limit")
        })?;
    let mut empty = item.clone();
    empty["text"]["content"] = Value::String(String::new());
    if empty.get("plain_text").is_some() {
        empty["plain_text"] = Value::String(String::new());
    }
    let overhead = encoded_len(&empty);
    if overhead >= budget {
        return Err(format!(
            "a rich text item is above the {MAX_BLOCK_TEXT_BYTES} byte block text limit"
        ));
    }
    // The content is encoded once in `text.content` and once in `plain_text`.
    let segment_budget = (budget - overhead) / 2;
    let mut pieces = Vec::new();
    let mut segment = String::new();
    let mut segment_bytes = 0;
    for character in content.chars() {
        let character_bytes = encoded_char_len(character);
        if !segment.is_empty() && segment_bytes + character_bytes > segment_budget {
            pieces.push(text_piece(&empty, std::mem::take(&mut segment)));
            segment_bytes = 0;
        }
        segment.push(character);
        segment_bytes += character_bytes;
    }
    if !segment.is_empty() {
        pieces.push(text_piece(&empty, segment));
    }
    Ok(pieces)
}

fn encoded_char_len(character: char) -> usize {
    let mut buffer = [0; 4];
    Value::String(character.encode_utf8(&mut buffer).to_string())
        .to_string()
        .len()
        - 2
}

fn text_piece(empty: &Value, segment: String) -> Value {
    let mut piece = empty.clone();
    if piece.get("plain_text").is_some() {
        piece["plain_text"] = Value::String(segment.clone());
    }
    piece["text"]["content"] = Value::String(segment);
    piece
}

fn parse_object(encoded: &str, block_type: &str, part: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str::<Value>(encoded)
        .map_err(|error| format!("parse {block_type} block {part}: {error}"))?
    {
        Value::Object(object) => Ok(object),
        _ => Err(format!("{block_type} block {part} must be an object")),
    }
}

/// Composes the API payload of a row selected with [`note_block_row_columns`].
pub(crate) fn payload_from_row(row: &SqliteRow) -> Result<String, sqlx::Error> {
    let block_type: String = row.try_get("block_type")?;
    let attrs: String = row.try_get("attrs")?;
    let text: String = row.try_get("text")?;
    let view = BlockViewState {
        open: row.try_get::<Option<bool>, _>("view_open")?,
        editing_locked: row.try_get::<Option<bool>, _>("view_editing_locked")?,
    };
    let child_page_title: Option<String> = row.try_get("child_page_title")?;
    compose_block_payload(
        &block_type,
        &attrs,
        &text,
        view,
        child_page_title.as_deref(),
    )
    .map(|payload| payload.to_string())
    .map_err(|error| sqlx::Error::Decode(error.into()))
}

/// Writes a block's view state, removing the row when nothing is set.
pub async fn write_block_view_state(
    conn: &mut SqliteConnection,
    block_id: &str,
    view: BlockViewState,
) -> Result<(), String> {
    if view.is_empty() {
        sqlx::query("DELETE FROM notes_block_view_state WHERE block_id = ?")
            .bind(block_id)
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("clear notes block view state: {error}"))?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notes_block_view_state (block_id, open, editing_locked)
         VALUES (?, ?, ?)
         ON CONFLICT(block_id) DO UPDATE SET
            open = excluded.open,
            editing_locked = excluded.editing_locked",
    )
    .bind(block_id)
    .bind(view.open)
    .bind(view.editing_locked)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("write notes block view state: {error}"))?;
    Ok(())
}

/// Loads a block's stored `attrs` and `text` composed into one payload, without view state or a
/// `child_page` title, for scans of references and assets.
pub async fn load_block_content_payload(
    conn: &mut SqliteConnection,
    block_id: &str,
) -> Result<Value, String> {
    let (block_type, attrs, text): (String, String, String) =
        sqlx::query_as("SELECT type, attrs, text FROM notes_blocks WHERE id = ?")
            .bind(block_id)
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| format!("load notes block content: {error}"))?;
    compose_block_content(&block_type, &attrs, &text)
}

/// Composes a content payload from stored `attrs` and `text`, without view state or a
/// `child_page` title.
pub fn compose_block_content(block_type: &str, attrs: &str, text: &str) -> Result<Value, String> {
    let mut payload = parse_object(attrs, block_type, "attrs")?;
    payload.extend(parse_object(text, block_type, "text")?);
    Ok(Value::Object(payload))
}

/// One block row to insert from an API payload.
pub struct BlockRowInsert<'a> {
    pub id: &'a str,
    pub page_id: &'a str,
    pub parent_type: &'a str,
    pub parent_page_id: Option<&'a str>,
    pub parent_block_id: Option<&'a str>,
    pub block_type: &'a str,
    pub payload: &'a Value,
    pub plain_text: &'a str,
    pub order_key: &'a str,
    pub source_provider: Option<&'a str>,
    pub source_object_id: Option<&'a str>,
    pub source_last_edited_time: Option<&'a str>,
}

/// Inserts one block, splitting its payload into the stored parts and its view state.
pub async fn insert_block_row(
    conn: &mut SqliteConnection,
    row: BlockRowInsert<'_>,
) -> Result<(), String> {
    let content = split_block_payload(row.block_type, row.payload)?;
    sqlx::query(
        "INSERT INTO notes_blocks (
            id, page_id, parent_type, parent_page_id, parent_block_id, type, attrs, text,
            plain_text, order_key, source_provider, source_object_id, source_last_edited_time
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(row.id)
    .bind(row.page_id)
    .bind(row.parent_type)
    .bind(row.parent_page_id)
    .bind(row.parent_block_id)
    .bind(row.block_type)
    .bind(&content.attrs)
    .bind(&content.text)
    .bind(row.plain_text)
    .bind(row.order_key)
    .bind(row.source_provider)
    .bind(row.source_object_id)
    .bind(row.source_last_edited_time)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("insert notes block: {error}"))?;
    write_block_view_state(conn, row.id, content.view).await
}

/// Replaces a block's stored payload parts without changing its plain text or edit time, for
/// rewrites of identities and references inside a payload.
pub async fn replace_block_payload(
    conn: &mut SqliteConnection,
    block_id: &str,
    block_type: &str,
    payload: &Value,
) -> Result<(), String> {
    let content = split_block_payload(block_type, payload)?;
    sqlx::query("UPDATE notes_blocks SET attrs = ?, text = ? WHERE id = ?")
        .bind(&content.attrs)
        .bind(&content.text)
        .bind(block_id)
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("replace notes block payload: {error}"))?;
    write_block_view_state(conn, block_id, content.view).await
}

/// Rewrites a block's stored `attrs` and `text` from a content payload such as one returned by
/// [`load_block_content_payload`], leaving its view state, plain text, and edit time unchanged.
pub async fn rewrite_block_content(
    conn: &mut SqliteConnection,
    block_id: &str,
    block_type: &str,
    payload: &Value,
) -> Result<(), String> {
    let content = split_block_payload(block_type, payload)?;
    sqlx::query("UPDATE notes_blocks SET attrs = ?, text = ? WHERE id = ?")
        .bind(&content.attrs)
        .bind(&content.text)
        .bind(block_id)
        .execute(&mut *conn)
        .await
        .map_err(|error| format!("rewrite notes block content: {error}"))?;
    Ok(())
}

/// Replaces a block's type and content from an API payload. The caller computes `plain_text`
/// from the same payload, and validates it first.
pub async fn write_block_content(
    conn: &mut SqliteConnection,
    block_id: &str,
    block_type: &str,
    payload: &Value,
    plain_text: &str,
) -> Result<(), String> {
    let content = split_block_payload(block_type, payload)?;
    sqlx::query(
        "UPDATE notes_blocks
         SET type = ?, attrs = ?, text = ?, plain_text = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(block_type)
    .bind(&content.attrs)
    .bind(&content.text)
    .bind(plain_text)
    .bind(block_id)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("write notes block content: {error}"))?;
    write_block_view_state(conn, block_id, content.view).await
}

/// Numbers blocks within their sibling groups, in the order the caller visits them.
///
/// Template snapshots store a flat ordinal while page blocks store order keys, so applying a
/// template renumbers each sibling group from its visiting order.
#[derive(Debug, Default)]
pub struct SiblingPositions {
    next: std::collections::HashMap<Option<String>, u32>,
}

impl SiblingPositions {
    /// Zero-based position of the next block under `parent_block_id` (`None` for root blocks).
    pub fn next(&mut self, parent_block_id: Option<&str>) -> u32 {
        let slot = self
            .next
            .entry(parent_block_id.map(str::to_string))
            .or_insert(0);
        let position = *slot;
        *slot += 1;
        position
    }

    /// Order key of the next block under `parent_block_id`.
    pub fn next_order_key(&mut self, parent_block_id: Option<&str>) -> Result<String, String> {
        ganbaru_sync_contracts::OrderKey::rank(self.next(parent_block_id))
            .map(ganbaru_sync_contracts::OrderKey::into_string)
            .map_err(|error| format!("rank notes block order key: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::NOTE_BLOCK_TYPES;
    use serde_json::json;

    fn text_item(content: &str) -> Value {
        json!({
            "type": "text",
            "text": { "content": content, "link": null },
            "annotations": { "bold": true, "color": "red" },
            "plain_text": content,
            "href": null
        })
    }

    /// A payload carrying every stored part: text keys, attributes, view state, and the
    /// `child_page` title.
    fn full_payload(block_type: &str) -> Value {
        let mut payload = json!({
            "rich_text": [text_item("Body")],
            "caption": [text_item("Caption")],
            "cells": [[text_item("A")], [text_item("B")]],
            "color": "blue_background",
            "url": "https://example.com/a",
            "nested": { "ganbaru_open": "attribute, not view state" },
            BLOCK_VIEW_OPEN_KEY: true,
            BLOCK_VIEW_EDITING_LOCKED_KEY: false
        });
        if block_type == "child_page" {
            payload["title"] = json!("Child title");
        }
        payload
    }

    #[test]
    fn every_block_type_round_trips_through_the_stored_parts() {
        assert_eq!(NOTE_BLOCK_TYPES.len(), 37);
        for block_type in NOTE_BLOCK_TYPES {
            let payload = full_payload(block_type);
            let stored = split_block_payload(block_type, &payload).unwrap();
            let text: Value = serde_json::from_str(&stored.text).unwrap();
            let attrs: Value = serde_json::from_str(&stored.attrs).unwrap();
            let mut text_keys: Vec<&str> = text
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            text_keys.sort_unstable();
            assert_eq!(text_keys, ["caption", "cells", "rich_text"], "{block_type}");
            for key in [
                BLOCK_VIEW_OPEN_KEY,
                BLOCK_VIEW_EDITING_LOCKED_KEY,
                "rich_text",
                "title",
            ] {
                assert!(attrs.get(key).is_none(), "{block_type} attrs keep {key}");
            }
            assert_eq!(
                stored.view,
                BlockViewState {
                    open: Some(true),
                    editing_locked: Some(false)
                }
            );
            let title = (*block_type == "child_page").then_some("Child title");
            let composed =
                compose_block_payload(block_type, &stored.attrs, &stored.text, stored.view, title)
                    .unwrap();
            assert_eq!(composed, payload, "{block_type}");
        }
    }

    #[test]
    fn a_payload_without_view_state_or_text_composes_unchanged() {
        for block_type in NOTE_BLOCK_TYPES {
            let mut payload = json!({ "color": "default" });
            if *block_type == "child_page" {
                payload["title"] = json!("");
            }
            let stored = split_block_payload(block_type, &payload).unwrap();
            assert_eq!(stored.text, "{}");
            assert!(stored.view.is_empty());
            let composed =
                compose_block_payload(block_type, &stored.attrs, &stored.text, stored.view, None)
                    .unwrap();
            assert_eq!(composed, payload, "{block_type}");
        }
    }

    #[test]
    fn view_state_keys_must_be_booleans() {
        let error =
            split_block_payload("toggle", &json!({ BLOCK_VIEW_OPEN_KEY: "yes" })).unwrap_err();
        assert!(error.contains("must be a boolean"), "{error}");
    }

    #[test]
    fn stored_parts_are_bounded_at_their_limits() {
        let at_text_limit = "x".repeat(MAX_BLOCK_TEXT_BYTES);
        let at_attrs_limit = "x".repeat(MAX_BLOCK_ATTRS_BYTES);
        check_block_content_bounds("paragraph", &at_attrs_limit, &at_text_limit).unwrap();
        let text_error =
            check_block_content_bounds("paragraph", "{}", &format!("{at_text_limit}x"))
                .unwrap_err();
        assert!(text_error.contains("text is"), "{text_error}");
        let attrs_error =
            check_block_content_bounds("paragraph", &format!("{at_attrs_limit}x"), "{}")
                .unwrap_err();
        assert!(attrs_error.contains("attributes are"), "{attrs_error}");

        let oversized_text = json!({ "rich_text": [text_item(&at_text_limit)] });
        assert!(split_block_payload("paragraph", &oversized_text).is_err());
        let oversized_attrs = json!({ "url": at_attrs_limit });
        assert!(split_block_payload("bookmark", &oversized_attrs).is_err());
    }

    #[test]
    fn oversized_rich_text_splits_into_bounded_chunks_without_losing_characters() {
        let long = "ab\u{e9}\u{1f600}\"".repeat(MAX_BLOCK_TEXT_BYTES / 4);
        let rich_text = vec![text_item("lead "), text_item(&long), text_item(" tail")];
        let chunks = split_rich_text_to_bounds(&rich_text).unwrap();
        assert!(chunks.len() > 1);
        let mut joined = String::new();
        for chunk in &chunks {
            let stored = split_block_payload("paragraph", &json!({ "rich_text": chunk })).unwrap();
            assert!(stored.text.len() <= MAX_BLOCK_TEXT_BYTES);
            for item in chunk {
                assert_eq!(item["annotations"]["bold"], true);
                assert_eq!(item["text"]["content"], item["plain_text"]);
                joined.push_str(item["text"]["content"].as_str().unwrap());
            }
        }
        assert_eq!(joined, format!("lead {long} tail"));
    }

    #[test]
    fn rich_text_within_the_bound_stays_one_chunk() {
        let rich_text = vec![text_item("short")];
        assert_eq!(
            split_rich_text_to_bounds(&rich_text).unwrap(),
            vec![rich_text]
        );
        assert_eq!(
            split_rich_text_to_bounds(&[]).unwrap(),
            vec![Vec::<Value>::new()]
        );
    }

    #[test]
    fn an_oversized_item_without_text_is_refused() {
        let equation = json!({
            "type": "equation",
            "equation": { "expression": "x".repeat(MAX_BLOCK_TEXT_BYTES) },
            "plain_text": ""
        });
        assert!(split_rich_text_to_bounds(&[equation]).is_err());
    }

    #[test]
    fn sibling_positions_number_each_parent_separately() {
        let mut positions = SiblingPositions::default();
        let first = positions.next_order_key(None).unwrap();
        let nested = positions.next_order_key(Some("parent")).unwrap();
        let second = positions.next_order_key(None).unwrap();
        assert!(first < second);
        assert_eq!(first, nested);
        assert!(nested < positions.next_order_key(Some("parent")).unwrap());
    }
}
