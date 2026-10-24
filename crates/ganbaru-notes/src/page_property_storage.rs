//! Stored shape of page properties and relation edges.
//!
//! The API shape of a page's properties is one JSON object keyed by property name. Storage keeps
//! the title in the page `title` column, one `notes_page_property_values` row per stored value,
//! and relations as `notes_relation_edges` rows. Reads compose the object back in SQL; writes diff
//! against the stored rows and touch only the rows that change, so concurrent edits to different
//! properties of one page never overwrite each other.
//!
//! A two-way relation is a pair of relation properties listed in `notes_relation_pairs`: two
//! properties that name each other as their synced property, or one that names a peer which names
//! none. Its edges are stored once, under the side whose `(data_source_id, property_id)` is
//! smaller; a property synced with itself stores each edge from the smaller page id. The
//! `notes_relation_links` view reads both sides, so the two sides of a pair always agree.

use crate::sync_keys::{PROPERTY_VALUES, RELATION_EDGES};
use serde_json::Value;
use sqlx::SqliteConnection;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Property types whose values are never stored as value rows: the title lives in the page row,
/// relations in edges, and the rest are computed on read.
pub const UNSTORED_VALUE_TYPES: &[&str] = &["title", "relation", "formula", "rollup", "button"];

/// SQL expression composing the API properties object of the `notes_pages` row named `page`.
///
/// Every page has its title entry, named after the title property of its data source or `title`
/// outside a data source, with the title as one plain rich text item. Row pages add one entry
/// per stored value whose property still exists, typed with the value's own type, so a value
/// written before a concurrent type change reads with its original type. Relation properties
/// list their related pages in link creation order.
pub(crate) fn page_properties_sql(page: &str) -> String {
    format!(
        "(SELECT json_group_object(composed.name, json(composed.value)) FROM (
            SELECT COALESCE(title_property.name, 'title') AS name,
                   json_object(
                       'id', COALESCE(title_property.property_id, 'title'),
                       'type', 'title',
                       'title', json_array(json_object(
                           'type', 'text',
                           'text', json_object('content', {page}.title, 'link', NULL),
                           'annotations', json_object(
                               'bold', json('false'), 'italic', json('false'),
                               'strikethrough', json('false'), 'underline', json('false'),
                               'code', json('false'), 'color', 'default'),
                           'plain_text', {page}.title,
                           'href', NULL))) AS value
            FROM (SELECT 1) AS anchor
            LEFT JOIN notes_data_source_properties AS title_property
              ON title_property.data_source_id = {page}.parent_data_source_id
             AND title_property.type = 'title'
            UNION ALL
            SELECT value_property.name,
                   json_object(
                       'id', stored_value.property_id,
                       'type', stored_value.value_type,
                       stored_value.value_type, json(stored_value.value))
            FROM notes_page_property_values AS stored_value
            JOIN notes_data_source_properties AS value_property
              ON value_property.data_source_id = {page}.parent_data_source_id
             AND value_property.property_id = stored_value.property_id
            WHERE stored_value.page_id = {page}.id
              AND value_property.type NOT IN ('title', 'relation')
            UNION ALL
            SELECT relation_property.name,
                   json_object(
                       'id', relation_property.property_id,
                       'type', 'relation',
                       'relation', json((
                           SELECT json_group_array(json_object('id', related.target_page_id)
                                  ORDER BY related.created_time, related.target_page_id)
                           FROM (
                               SELECT link.target_page_id, MIN(link.created_time) AS created_time
                               FROM notes_relation_links AS link
                               WHERE link.source_page_id = {page}.id
                                 AND link.source_property_id = relation_property.property_id
                                 AND link.source_data_source_id = relation_property.data_source_id
                               GROUP BY link.target_page_id
                           ) AS related)),
                       'has_more', json('false')) AS value
            FROM notes_data_source_properties AS relation_property
            WHERE relation_property.data_source_id = {page}.parent_data_source_id
              AND relation_property.type = 'relation'
        ) AS composed)"
    )
}

/// Reads the composed properties of `page_id`, or `None` when the page does not exist.
pub async fn load_page_properties(
    connection: &mut SqliteConnection,
    page_id: &str,
) -> Result<Option<Value>, String> {
    let properties: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {} FROM notes_pages AS page WHERE page.id = ?",
        page_properties_sql("page")
    )))
    .bind(page_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|e| format!("load notes page properties: {e}"))?;
    properties
        .map(|properties| {
            serde_json::from_str(&properties).map_err(|e| format!("parse page properties: {e}"))
        })
        .transpose()
}

/// One property of a data source, as value writes resolve it.
struct SchemaProperty {
    property_id: String,
    name: String,
    property_type: String,
}

/// Replaces the stored values of `page_id` with the values in the API object `properties`.
///
/// Entries name their property by `id`, or by key when the id names no property of the page's
/// data source. Entries for the title, relations, computed properties, and unknown properties
/// are ignored: the title is written through the page row and relations through
/// [`replace_relation_targets`]. A stored value whose property the object omits is deleted; a
/// value whose property no longer exists is kept so a concurrent schema change can restore it.
/// Pages outside a data source store no values.
pub async fn write_page_properties(
    connection: &mut SqliteConnection,
    page_id: &str,
    properties: &Value,
) -> Result<(), String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "page properties must be an object".to_string())?;
    let data_source_id: Option<Option<String>> =
        sqlx::query_scalar("SELECT parent_data_source_id FROM notes_pages WHERE id = ?")
            .bind(page_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(|e| format!("load notes page data source: {e}"))?;
    let Some(data_source_id) = data_source_id.flatten() else {
        return Ok(());
    };
    let schema = load_schema(connection, &data_source_id).await?;
    let by_id = schema
        .iter()
        .map(|property| (property.property_id.as_str(), property))
        .collect::<HashMap<_, _>>();
    let by_name = schema
        .iter()
        .map(|property| (property.name.as_str(), property))
        .collect::<HashMap<_, _>>();
    let mut desired = HashMap::<String, (String, Value)>::new();
    for (key, entry) in object {
        let Some(entry_object) = entry.as_object() else {
            continue;
        };
        let property = entry_object
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| by_id.get(id))
            .or_else(|| by_name.get(key.as_str()));
        let Some(property) = property else {
            continue;
        };
        if UNSTORED_VALUE_TYPES.contains(&property.property_type.as_str()) {
            continue;
        }
        let value_type = entry_object
            .get("type")
            .and_then(Value::as_str)
            .filter(|value_type| !value_type.trim().is_empty())
            .unwrap_or(&property.property_type);
        if UNSTORED_VALUE_TYPES.contains(&value_type) {
            continue;
        }
        let value = entry_object.get(value_type).cloned().unwrap_or(Value::Null);
        desired.insert(
            property.property_id.clone(),
            (value_type.to_string(), value),
        );
    }
    let stored = sqlx::query_as::<_, (String, String, String)>(
        "SELECT property_id, value_type, value FROM notes_page_property_values WHERE page_id = ?",
    )
    .bind(page_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes page property values: {e}"))?;
    for (property_id, ..) in &stored {
        let omitted_schema_value = by_id.get(property_id.as_str()).is_some_and(|property| {
            !UNSTORED_VALUE_TYPES.contains(&property.property_type.as_str())
        }) && !desired.contains_key(property_id);
        if omitted_schema_value {
            delete_page_property_value(connection, page_id, property_id).await?;
        }
    }
    let stored_by_id = stored
        .iter()
        .map(|row| (row.0.as_str(), row))
        .collect::<HashMap<_, _>>();
    for (property_id, (value_type, value)) in &desired {
        let unchanged = stored_by_id.get(property_id.as_str()).is_some_and(|row| {
            row.1 == *value_type
                && serde_json::from_str::<Value>(&row.2).ok().as_ref() == Some(value)
        });
        if !unchanged {
            write_page_property_value(connection, page_id, property_id, value_type, value).await?;
        }
    }
    Ok(())
}

async fn load_schema(
    connection: &mut SqliteConnection,
    data_source_id: &str,
) -> Result<Vec<SchemaProperty>, String> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT property_id, name, type FROM notes_data_source_properties
         WHERE data_source_id = ? ORDER BY order_key, property_id",
    )
    .bind(data_source_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes data source property names: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|(property_id, name, property_type)| SchemaProperty {
            property_id,
            name,
            property_type,
        })
        .collect())
}

/// Writes one value row, keeping the row's sync key when it already exists.
pub async fn write_page_property_value(
    connection: &mut SqliteConnection,
    page_id: &str,
    property_id: &str,
    value_type: &str,
    value: &Value,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_page_property_values (page_id, property_id, sync_key, value_type, value)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (page_id, property_id) DO UPDATE SET
           value_type = excluded.value_type,
           value = excluded.value",
    )
    .bind(page_id)
    .bind(property_id)
    .bind(PROPERTY_VALUES.key(&[page_id, property_id])?)
    .bind(value_type)
    .bind(value.to_string())
    .execute(&mut *connection)
    .await
    .map_err(|e| format!("write notes page property value: {e}"))?;
    Ok(())
}

/// Whether a composed property entry carries `property_id` with a stored type other than `schema_type`, along with
/// the payload for its own type.
pub(crate) fn is_mismatched_value(value: &Value, property_id: &str, schema_type: &str) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(stored_type) = object.get("type").and_then(Value::as_str) else {
        return false;
    };
    object.get("id").and_then(Value::as_str) == Some(property_id)
        && stored_type != schema_type
        && object.contains_key(stored_type)
}

/// Deletes one value row.
pub async fn delete_page_property_value(
    connection: &mut SqliteConnection,
    page_id: &str,
    property_id: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM notes_page_property_values WHERE page_id = ? AND property_id = ?")
        .bind(page_id)
        .bind(property_id)
        .execute(&mut *connection)
        .await
        .map_err(|e| format!("delete notes page property value: {e}"))?;
    Ok(())
}

/// Copies every value row of `source_page_id` onto `target_page_id`, replacing its values.
pub async fn copy_page_property_values(
    connection: &mut SqliteConnection,
    source_page_id: &str,
    target_page_id: &str,
) -> Result<(), String> {
    sqlx::query("DELETE FROM notes_page_property_values WHERE page_id = ?")
        .bind(target_page_id)
        .execute(&mut *connection)
        .await
        .map_err(|e| format!("clear copied notes page property values: {e}"))?;
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT property_id, value_type, value FROM notes_page_property_values WHERE page_id = ?",
    )
    .bind(source_page_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes page property values to copy: {e}"))?;
    for (property_id, value_type, value) in rows {
        let value = serde_json::from_str::<Value>(&value)
            .map_err(|e| format!("parse notes page property value: {e}"))?;
        write_page_property_value(
            connection,
            target_page_id,
            &property_id,
            &value_type,
            &value,
        )
        .await?;
    }
    Ok(())
}

/// Links `target_page_id` to every page `source_page_id` links to under the relation properties
/// of `data_source_id`, the data source both rows belong to. A link from the source to itself
/// becomes a link from the copy to the source.
pub async fn copy_relation_links(
    connection: &mut SqliteConnection,
    data_source_id: &str,
    source_page_id: &str,
    target_page_id: &str,
) -> Result<(), String> {
    let links = sqlx::query_as::<_, (String, String)>(
        "SELECT source_property_id, target_page_id
         FROM notes_relation_links
         WHERE source_page_id = ? AND source_data_source_id = ?
         GROUP BY source_property_id, target_page_id
         ORDER BY MIN(created_time), source_property_id, target_page_id",
    )
    .bind(source_page_id)
    .bind(data_source_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes relation links to copy: {e}"))?;
    let mut pairings = HashMap::<String, RelationPairing>::new();
    for (property_id, linked_page_id) in links {
        let pairing = match pairings.get(&property_id) {
            Some(pairing) => pairing.clone(),
            None => {
                let pairing = relation_pairing(connection, data_source_id, &property_id).await?;
                pairings.insert(property_id.clone(), pairing.clone());
                pairing
            }
        };
        if let Some(edge) = stored_edge(&pairing, target_page_id, &property_id, &linked_page_id) {
            insert_edge(connection, &edge, None).await?;
        }
    }
    Ok(())
}

/// The stored orientation of edges written under one relation property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationPairing {
    /// The property is not a relation of the data source.
    Missing,
    /// Edges are stored under the property itself.
    Own,
    /// Edges are stored under the property itself from the smaller page id; the property is
    /// synced with itself.
    SelfSynced,
    /// Edges are stored under the synced property, reversed.
    Peer(String),
}

/// How edges of `(data_source_id, property_id)` are stored.
pub async fn relation_pairing(
    connection: &mut SqliteConnection,
    data_source_id: &str,
    property_id: &str,
) -> Result<RelationPairing, String> {
    let row = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
        "SELECT property.type, pair.peer_data_source_id, pair.peer_property_id
         FROM notes_data_source_properties AS property
         LEFT JOIN notes_relation_pairs AS pair
           ON pair.data_source_id = property.data_source_id
          AND pair.property_id = property.property_id
         WHERE property.data_source_id = ? AND property.property_id = ?",
    )
    .bind(data_source_id)
    .bind(property_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|e| format!("load notes relation pairing: {e}"))?;
    Ok(match row {
        Some((property_type, _, _)) if property_type != "relation" => RelationPairing::Missing,
        None => RelationPairing::Missing,
        Some((_, Some(peer_data_source_id), Some(peer_property_id))) => {
            if peer_data_source_id == data_source_id && peer_property_id == property_id {
                RelationPairing::SelfSynced
            } else if (peer_data_source_id.as_str(), peer_property_id.as_str())
                < (data_source_id, property_id)
            {
                RelationPairing::Peer(peer_property_id)
            } else {
                RelationPairing::Own
            }
        }
        Some(_) => RelationPairing::Own,
    })
}

/// One edge in its stored orientation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoredEdge {
    pub source_page_id: String,
    pub source_property_id: String,
    pub target_page_id: String,
}

/// The stored orientation of the link from `page_id` to `target_page_id` under a property with
/// `pairing`, or `None` when the property is not a relation.
pub fn stored_edge(
    pairing: &RelationPairing,
    page_id: &str,
    property_id: &str,
    target_page_id: &str,
) -> Option<StoredEdge> {
    let edge = |source: &str, property: &str, target: &str| StoredEdge {
        source_page_id: source.to_string(),
        source_property_id: property.to_string(),
        target_page_id: target.to_string(),
    };
    match pairing {
        RelationPairing::Missing => None,
        RelationPairing::Own => Some(edge(page_id, property_id, target_page_id)),
        RelationPairing::SelfSynced if target_page_id < page_id => {
            Some(edge(target_page_id, property_id, page_id))
        }
        RelationPairing::SelfSynced => Some(edge(page_id, property_id, target_page_id)),
        RelationPairing::Peer(peer_property_id) => {
            Some(edge(target_page_id, peer_property_id, page_id))
        }
    }
}

/// The pages linked from `page_id` under `property_id`, on either side of the stored edges, in
/// link creation order.
pub async fn related_page_ids(
    connection: &mut SqliteConnection,
    page_id: &str,
    property_id: &str,
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT link.target_page_id
         FROM notes_relation_links AS link
         JOIN notes_pages AS page ON page.id = link.source_page_id
         WHERE link.source_page_id = ?
           AND link.source_property_id = ?
           AND link.source_data_source_id = page.parent_data_source_id
         GROUP BY link.target_page_id
         ORDER BY MIN(link.created_time), link.target_page_id",
    )
    .bind(page_id)
    .bind(property_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|e| format!("load notes related pages: {e}"))
}

/// Makes `targets` the pages linked from `page_id` under `property_id` of `data_source_id`.
///
/// Removed links delete their edge in either orientation, and added links insert the stored
/// orientation, so the synced property of a two-way relation follows without a second write.
/// Returns the targets that were added and removed.
pub async fn replace_relation_targets(
    connection: &mut SqliteConnection,
    data_source_id: &str,
    page_id: &str,
    property_id: &str,
    targets: &[String],
) -> Result<(Vec<String>, Vec<String>), String> {
    let pairing = relation_pairing(connection, data_source_id, property_id).await?;
    if pairing == RelationPairing::Missing {
        return Err("row property is not a relation".to_string());
    }
    let current = related_page_ids(connection, page_id, property_id).await?;
    let wanted = targets.iter().map(String::as_str).collect::<HashSet<_>>();
    let existing = current.iter().map(String::as_str).collect::<HashSet<_>>();
    let removed = current
        .iter()
        .filter(|target| !wanted.contains(target.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for target in &removed {
        delete_link(connection, &pairing, page_id, property_id, target).await?;
    }
    let mut added = Vec::new();
    for target in targets {
        if existing.contains(target.as_str()) || added.contains(target) {
            continue;
        }
        if let Some(edge) = stored_edge(&pairing, page_id, property_id, target) {
            insert_edge(connection, &edge, None).await?;
        }
        added.push(target.clone());
    }
    Ok((added, removed))
}

async fn delete_link(
    connection: &mut SqliteConnection,
    pairing: &RelationPairing,
    page_id: &str,
    property_id: &str,
    target_page_id: &str,
) -> Result<(), String> {
    // A one-way property reads only its forward edges, so an edge stored the other way under
    // the same property is a separate link and must survive.
    let reverse_property_id = match pairing {
        RelationPairing::Peer(peer_property_id) => peer_property_id.as_str(),
        RelationPairing::SelfSynced => property_id,
        RelationPairing::Own | RelationPairing::Missing => {
            sqlx::query(
                "DELETE FROM notes_relation_edges
                 WHERE source_page_id = ? AND source_property_id = ? AND target_page_id = ?",
            )
            .bind(page_id)
            .bind(property_id)
            .bind(target_page_id)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("delete notes relation edge: {e}"))?;
            return Ok(());
        }
    };
    sqlx::query(
        "DELETE FROM notes_relation_edges
         WHERE (source_page_id = ? AND source_property_id = ? AND target_page_id = ?)
            OR (source_page_id = ? AND source_property_id = ? AND target_page_id = ?)",
    )
    .bind(page_id)
    .bind(property_id)
    .bind(target_page_id)
    .bind(target_page_id)
    .bind(reverse_property_id)
    .bind(page_id)
    .execute(&mut *connection)
    .await
    .map_err(|e| format!("delete notes relation edge: {e}"))?;
    Ok(())
}

/// Inserts one edge with its derived sync key, keeping an existing edge unchanged.
pub async fn insert_edge(
    connection: &mut SqliteConnection,
    edge: &StoredEdge,
    created_time: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO notes_relation_edges
           (source_page_id, source_property_id, target_page_id, sync_key, created_time)
         VALUES (?, ?, ?, ?, COALESCE(?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
    )
    .bind(&edge.source_page_id)
    .bind(&edge.source_property_id)
    .bind(&edge.target_page_id)
    .bind(RELATION_EDGES.key(&[
        &edge.source_page_id,
        &edge.source_property_id,
        &edge.target_page_id,
    ])?)
    .bind(created_time)
    .execute(&mut *connection)
    .await
    .map_err(|e| format!("insert notes relation edge: {e}"))?;
    Ok(())
}

/// One link read from `notes_relation_links`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RelationLink {
    pub source_page_id: String,
    pub source_data_source_id: String,
    pub source_property_id: String,
    pub target_page_id: String,
    pub created_time: String,
}

/// The relation properties whose links a schema write can change, as `(data_source_id,
/// property_id)`.
pub type RelationScope = BTreeSet<(String, String)>;

/// Adds to `scope` every stored relation property that names a property of `scope` as its synced
/// property.
///
/// A pairing can only change for a property of the scope or for a property naming one, so the
/// widened scope covers every link a schema write can move. Run it before the write.
pub async fn extend_scope_with_namers(
    connection: &mut SqliteConnection,
    scope: &mut RelationScope,
) -> Result<(), String> {
    let mut namers = Vec::new();
    for (data_source_id, property_id) in scope.iter() {
        let mut rows = sqlx::query_as::<_, (String, String)>(
            "SELECT data_source_id, property_id
             FROM notes_data_source_properties
             WHERE type = 'relation'
               AND json_extract(config, '$.relation.data_source_id') = ?
               AND json_extract(config, '$.relation.dual_property.synced_property_id') = ?",
        )
        .bind(data_source_id)
        .bind(property_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load notes relation namers: {e}"))?;
        namers.append(&mut rows);
    }
    scope.extend(namers);
    Ok(())
}

/// Reads every link visible under the properties in `scope`.
pub async fn relation_links_in_scope(
    connection: &mut SqliteConnection,
    scope: &RelationScope,
) -> Result<Vec<RelationLink>, String> {
    let mut links = Vec::new();
    for (data_source_id, property_id) in scope {
        let mut rows = sqlx::query_as::<_, RelationLink>(
            "SELECT source_page_id, source_data_source_id, source_property_id, target_page_id,
                    created_time
             FROM notes_relation_links
             WHERE source_data_source_id = ? AND source_property_id = ?",
        )
        .bind(data_source_id)
        .bind(property_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load notes relation links: {e}"))?;
        links.append(&mut rows);
    }
    Ok(links)
}

/// Rewrites the stored edges under `scope` so that `links`, read before a schema write, keep
/// their meaning under the schema now stored.
///
/// A link survives when its property is still a relation whose target data source holds the
/// linked page; it is then stored in the orientation the new pairing gives it. So a two-way
/// relation split by deleting, retyping, or unpairing one side leaves the surviving side with
/// every link it showed, and a retargeted relation drops links to pages of the old target. Only
/// edges that change are written.
pub async fn reconcile_relation_edges(
    connection: &mut SqliteConnection,
    scope: &RelationScope,
    links: &[RelationLink],
) -> Result<(), String> {
    let mut pairings = HashMap::<(String, String), (RelationPairing, Option<String>)>::new();
    for key in scope {
        let pairing = relation_pairing(connection, &key.0, &key.1).await?;
        let target: Option<String> = sqlx::query_scalar(
            "SELECT json_extract(config, '$.relation.data_source_id')
             FROM notes_data_source_properties
             WHERE data_source_id = ? AND property_id = ? AND type = 'relation'",
        )
        .bind(&key.0)
        .bind(&key.1)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|e| format!("load notes relation target data source: {e}"))?
        .flatten();
        pairings.insert(key.clone(), (pairing, target));
    }
    let mut desired = HashMap::<StoredEdge, String>::new();
    for link in links {
        let key = (
            link.source_data_source_id.clone(),
            link.source_property_id.clone(),
        );
        let Some((pairing, Some(target_data_source_id))) = pairings.get(&key) else {
            continue;
        };
        let target_parent: Option<Option<String>> =
            sqlx::query_scalar("SELECT parent_data_source_id FROM notes_pages WHERE id = ?")
                .bind(&link.target_page_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|e| format!("load notes relation target page: {e}"))?;
        if target_parent.flatten().as_deref() != Some(target_data_source_id.as_str()) {
            continue;
        }
        if let Some(edge) = stored_edge(
            pairing,
            &link.source_page_id,
            &link.source_property_id,
            &link.target_page_id,
        ) {
            let created_time = desired
                .entry(edge)
                .or_insert_with(|| link.created_time.clone());
            if link.created_time < *created_time {
                *created_time = link.created_time.clone();
            }
        }
    }
    let mut stored = Vec::new();
    for (data_source_id, property_id) in scope {
        let mut rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT edge.source_page_id, edge.source_property_id, edge.target_page_id
             FROM notes_relation_edges AS edge
             JOIN notes_pages AS page ON page.id = edge.source_page_id
             WHERE page.parent_data_source_id = ? AND edge.source_property_id = ?",
        )
        .bind(data_source_id)
        .bind(property_id)
        .fetch_all(&mut *connection)
        .await
        .map_err(|e| format!("load notes relation edges: {e}"))?;
        stored.append(&mut rows);
    }
    let stored = stored
        .into_iter()
        .map(
            |(source_page_id, source_property_id, target_page_id)| StoredEdge {
                source_page_id,
                source_property_id,
                target_page_id,
            },
        )
        .collect::<HashSet<_>>();
    for edge in &stored {
        if !desired.contains_key(edge) {
            sqlx::query(
                "DELETE FROM notes_relation_edges
                 WHERE source_page_id = ? AND source_property_id = ? AND target_page_id = ?",
            )
            .bind(&edge.source_page_id)
            .bind(&edge.source_property_id)
            .bind(&edge.target_page_id)
            .execute(&mut *connection)
            .await
            .map_err(|e| format!("delete notes relation edge: {e}"))?;
        }
    }
    for (edge, created_time) in &desired {
        if !stored.contains(edge) {
            insert_edge(connection, edge, Some(created_time)).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RelationPairing, StoredEdge, stored_edge};

    fn edge(source: &str, property: &str, target: &str) -> StoredEdge {
        StoredEdge {
            source_page_id: source.to_string(),
            source_property_id: property.to_string(),
            target_page_id: target.to_string(),
        }
    }

    #[test]
    fn stored_edge_follows_the_pairing() {
        assert_eq!(stored_edge(&RelationPairing::Missing, "a", "p", "b"), None);
        assert_eq!(
            stored_edge(&RelationPairing::Own, "b", "p", "a"),
            Some(edge("b", "p", "a"))
        );
        assert_eq!(
            stored_edge(&RelationPairing::SelfSynced, "b", "p", "a"),
            Some(edge("a", "p", "b"))
        );
        assert_eq!(
            stored_edge(&RelationPairing::SelfSynced, "a", "p", "b"),
            Some(edge("a", "p", "b"))
        );
        assert_eq!(
            stored_edge(&RelationPairing::Peer("q".to_string()), "a", "p", "b"),
            Some(edge("b", "q", "a"))
        );
    }
}
