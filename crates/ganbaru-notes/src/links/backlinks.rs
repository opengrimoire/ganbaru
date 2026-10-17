use crate::links::{LocalLinkResolver, canonical_notes_id};
use crate::models::{NoteBacklinkDto, NoteBacklinkIndexedInput, NoteBlockRow};
use crate::reads;
use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::HashSet;

const FINGERPRINT_KEY: &str = "source_fingerprint";

#[derive(Clone, Copy)]
enum ReferenceSourceKind {
    Block,
    Comment,
}

pub async fn list_backlinks(
    pool: &SqlitePool,
    page_id: &str,
) -> Result<Vec<NoteBacklinkDto>, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    if !reads::page_exists(pool, page_id).await? {
        return Err("notes page not found".to_string());
    }
    ensure_index_current(pool).await?;

    let rows = sqlx::query_as::<_, BacklinkResultRow>(
        "SELECT
            idx.id,
            idx.source_type,
            idx.source_page_id,
            idx.source_block_id,
            idx.reference_type,
            idx.snippet,
            idx.created_time,
            idx.last_edited_time,
            block.type AS source_block_type
         FROM notes_backlink_index AS idx
         JOIN notes_pages AS source_page ON source_page.id = idx.source_page_id
         LEFT JOIN notes_blocks AS block ON block.id = idx.source_block_id
         LEFT JOIN notes_comments AS comment ON comment.id = idx.source_comment_id
         LEFT JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         LEFT JOIN notes_blocks AS thread_block ON thread_block.id = thread.parent_block_id
         WHERE idx.target_type = 'page'
           AND idx.target_id = ?
           AND source_page.in_trash = 0
           AND source_page.archived = 0
           AND (
               source_page.parent_type != 'data_source_id'
               OR EXISTS (
                   SELECT 1
                   FROM notes_data_sources AS data_source
                   JOIN notes_databases AS database ON database.id = data_source.database_id
                   WHERE data_source.id = source_page.parent_data_source_id
                     AND data_source.in_trash = 0
                     AND database.in_trash = 0
               )
           )
           AND (
               idx.source_block_id IS NULL
               OR (block.id IS NOT NULL AND block.in_trash = 0)
           )
           AND (
               idx.source_comment_id IS NULL
               OR (
                   comment.id IS NOT NULL
                   AND comment.deleted_at IS NULL
                   AND thread.id IS NOT NULL
                   AND (
                       thread.parent_block_id IS NULL
                       OR (thread_block.id IS NOT NULL AND thread_block.in_trash = 0)
                   )
               )
           )
         ORDER BY idx.last_edited_time DESC, idx.reference_type ASC, idx.id ASC",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes backlinks from index: {e}"))?;

    let mut backlinks = Vec::new();
    for row in rows {
        let source_page = reads::get_page(pool, &row.source_page_id, false).await?;
        let source_page_id = row.source_page_id.clone();
        let source_block_id = row
            .source_block_id
            .clone()
            .unwrap_or_else(|| source_page_id.clone());
        let source_block_type = match row.source_type.as_str() {
            "database_relation" => "database_relation".to_string(),
            "comment" => "comment".to_string(),
            _ => row
                .source_block_type
                .clone()
                .unwrap_or_else(|| row.source_type.clone()),
        };
        backlinks.push(NoteBacklinkDto::indexed(NoteBacklinkIndexedInput {
            source_page,
            id: row.id,
            source_block_id,
            source_block_type,
            reference_type: row.reference_type,
            snippet: row.snippet,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        }));
    }
    Ok(backlinks)
}

pub async fn rebuild_index(pool: &SqlitePool) -> Result<i64, String> {
    let fingerprint = backlink_source_fingerprint(pool).await?;
    let entries = build_index_entries(pool).await?;
    let entry_count = entries.len() as i64;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes backlink index rebuild: {e}"))?;

    sqlx::query("DELETE FROM notes_backlink_index")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes backlink index rows: {e}"))?;

    for entry in &entries {
        sqlx::query(
            "INSERT INTO notes_backlink_index (
                id,
                target_type,
                target_id,
                target_object_type,
                source_type,
                source_page_id,
                source_block_id,
                source_comment_id,
                source_property_id,
                source_property_name,
                reference_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.target_type)
        .bind(&entry.target_id)
        .bind(&entry.target_object_type)
        .bind(&entry.source_type)
        .bind(&entry.source_page_id)
        .bind(&entry.source_block_id)
        .bind(&entry.source_comment_id)
        .bind(&entry.source_property_id)
        .bind(&entry.source_property_name)
        .bind(&entry.reference_type)
        .bind(&entry.snippet)
        .bind(&entry.created_time)
        .bind(&entry.last_edited_time)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("insert notes backlink index row: {e}"))?;
    }

    sqlx::query(
        "INSERT INTO notes_backlink_index_state (key, value, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at",
    )
    .bind(FINGERPRINT_KEY)
    .bind(&fingerprint.value)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record notes backlink index fingerprint: {e}"))?;

    tx.commit()
        .await
        .map_err(|e| format!("commit notes backlink index rebuild: {e}"))?;
    Ok(entry_count)
}

async fn ensure_index_current(pool: &SqlitePool) -> Result<(), String> {
    let fingerprint = backlink_source_fingerprint(pool).await?;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT value FROM notes_backlink_index_state WHERE key = ?")
            .bind(FINGERPRINT_KEY)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes backlink index fingerprint: {e}"))?;
    let index_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_backlink_index")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("count notes backlink index rows: {e}"))?;

    if stored.as_deref() != Some(fingerprint.value.as_str())
        || (fingerprint.source_rows > 0 && index_rows == 0)
    {
        rebuild_index(pool).await?;
    }
    Ok(())
}

async fn build_index_entries(pool: &SqlitePool) -> Result<Vec<BacklinkIndexEntry>, String> {
    let mut entries = Vec::new();
    let link_resolver = crate::links::local_link_resolver(pool).await?;
    append_block_entries(pool, &link_resolver, &mut entries).await?;
    append_comment_entries(pool, &link_resolver, &mut entries).await?;
    append_relation_entries(pool, &mut entries).await?;
    Ok(entries)
}

async fn append_block_entries(
    pool: &SqlitePool,
    link_resolver: &LocalLinkResolver,
    entries: &mut Vec<BacklinkIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, NoteBlockRow>(
        "SELECT
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            has_children,
            in_trash,
            type AS block_type,
            payload,
            plain_text,
            sort_order,
            source_provider,
            source_object_id,
            source_last_edited_time,
            created_time,
            last_edited_time
         FROM notes_blocks",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes blocks for backlink index: {e}"))?;

    for row in rows {
        if row.block_type == "child_page" && is_notes_uuid(&row.id) {
            entries.push(BacklinkIndexEntry::from_source(
                SourceBacklinkEntry {
                    source_type: "block",
                    source_page_id: &row.page_id,
                    source_block_id: Some(&row.id),
                    source_comment_id: None,
                    source_property_id: None,
                    source_property_name: "",
                    snippet: backlink_snippet(&row.plain_text, &row.block_type),
                    created_time: &row.created_time,
                    last_edited_time: &row.last_edited_time,
                },
                ExtractedReference::page(&row.id, "child_page"),
            ));
        }

        let payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse notes backlink block payload: {e}"))?;
        let references = extract_references(&payload, ReferenceSourceKind::Block, link_resolver);
        push_source_references(
            entries,
            SourceBacklinkEntry {
                source_type: "block",
                source_page_id: &row.page_id,
                source_block_id: Some(&row.id),
                source_comment_id: None,
                source_property_id: None,
                source_property_name: "",
                snippet: backlink_snippet(&row.plain_text, &row.block_type),
                created_time: &row.created_time,
                last_edited_time: &row.last_edited_time,
            },
            references,
        );
    }
    Ok(())
}

async fn append_comment_entries(
    pool: &SqlitePool,
    link_resolver: &LocalLinkResolver,
    entries: &mut Vec<BacklinkIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, CommentBacklinkSourceRow>(
        "SELECT
            comment.id,
            comment.rich_text,
            comment.plain_text,
            comment.created_time,
            comment.last_edited_time,
            thread.page_id,
            thread.parent_block_id
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes comments for backlink index: {e}"))?;

    for row in rows {
        let rich_text: Value = serde_json::from_str(&row.rich_text)
            .map_err(|e| format!("parse notes backlink comment rich_text: {e}"))?;
        let references =
            extract_references(&rich_text, ReferenceSourceKind::Comment, link_resolver);
        push_source_references(
            entries,
            SourceBacklinkEntry {
                source_type: "comment",
                source_page_id: &row.page_id,
                source_block_id: row.parent_block_id.as_deref(),
                source_comment_id: Some(&row.id),
                source_property_id: None,
                source_property_name: "",
                snippet: comment_snippet(&row.plain_text),
                created_time: &row.created_time,
                last_edited_time: &row.last_edited_time,
            },
            references,
        );
    }
    Ok(())
}

async fn append_relation_entries(
    pool: &SqlitePool,
    entries: &mut Vec<BacklinkIndexEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, RelationBacklinkSourceRow>(
        "SELECT
            link.source_page_id,
            link.source_property_id,
            link.source_property_name,
            link.target_page_id,
            link.created_time,
            source_page.last_edited_time
         FROM notes_data_source_relation_links AS link
         JOIN notes_pages AS source_page ON source_page.id = link.source_page_id
         JOIN notes_pages AS target_page ON target_page.id = link.target_page_id
         JOIN notes_data_sources AS source_data_source
              ON source_data_source.id = link.source_data_source_id
         JOIN notes_databases AS source_database
              ON source_database.id = source_data_source.database_id
         JOIN notes_data_sources AS target_data_source
              ON target_data_source.id = link.target_data_source_id
         JOIN notes_databases AS target_database
              ON target_database.id = target_data_source.database_id
         WHERE source_page.in_trash = 0
           AND source_page.archived = 0
           AND target_page.in_trash = 0
           AND target_page.archived = 0
           AND source_data_source.in_trash = 0
           AND target_data_source.in_trash = 0
           AND source_database.in_trash = 0
           AND target_database.in_trash = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes relation links for backlink index: {e}"))?;

    for row in rows {
        entries.push(BacklinkIndexEntry::from_source(
            SourceBacklinkEntry {
                source_type: "database_relation",
                source_page_id: &row.source_page_id,
                source_block_id: None,
                source_comment_id: None,
                source_property_id: Some(&row.source_property_id),
                source_property_name: &row.source_property_name,
                snippet: relation_snippet(&row.source_property_name),
                created_time: &row.created_time,
                last_edited_time: &row.last_edited_time,
            },
            ExtractedReference::page(&row.target_page_id, "database_relation"),
        ));
    }
    Ok(())
}

fn push_source_references(
    entries: &mut Vec<BacklinkIndexEntry>,
    source: SourceBacklinkEntry<'_>,
    references: Vec<ExtractedReference>,
) {
    let mut seen = HashSet::new();
    for reference in references {
        let key = reference.dedup_key();
        if seen.insert(key) {
            entries.push(BacklinkIndexEntry::from_source(source.clone(), reference));
        }
    }
}

fn extract_references(
    value: &Value,
    source: ReferenceSourceKind,
    link_resolver: &LocalLinkResolver,
) -> Vec<ExtractedReference> {
    let mut references = Vec::new();
    collect_references(value, source, link_resolver, &mut references);
    references
}

fn collect_references(
    value: &Value,
    source: ReferenceSourceKind,
    link_resolver: &LocalLinkResolver,
    references: &mut Vec<ExtractedReference>,
) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = mention_reference(object, source) {
                references.push(reference);
                return;
            }
            for field in ["href", "url"] {
                if let Some(url) = object.get(field).and_then(Value::as_str) {
                    push_notes_link_references(references, url, source, link_resolver);
                }
            }
            if let Some(url) = object
                .get("link")
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
            {
                push_notes_link_references(references, url, source, link_resolver);
            }
            for nested in object.values() {
                collect_references(nested, source, link_resolver, references);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_references(nested, source, link_resolver, references);
            }
        }
        _ => {}
    }
}

fn mention_reference(
    object: &serde_json::Map<String, Value>,
    source: ReferenceSourceKind,
) -> Option<ExtractedReference> {
    let mention = object.get("mention")?.as_object()?;
    let mention_type = mention.get("type")?.as_str()?;
    match mention_type {
        "page" => {
            let page_id = mention
                .get("page")?
                .get("id")?
                .as_str()
                .and_then(canonical_notes_id)?;
            Some(ExtractedReference::page(
                &page_id,
                match source {
                    ReferenceSourceKind::Block => "page_mention",
                    ReferenceSourceKind::Comment => "comment_mention",
                },
            ))
        }
        "database" => {
            let database_id = mention
                .get("database")?
                .get("id")?
                .as_str()
                .and_then(canonical_notes_id)?;
            Some(ExtractedReference {
                target_type: "database".to_string(),
                target_id: database_id,
                target_object_type: None,
                reference_type: "database_mention".to_string(),
            })
        }
        "ganbaru_object" => {
            let object = mention.get("ganbaru_object")?.as_object()?;
            let object_type = object.get("type")?.as_str()?.trim();
            let object_id = object.get("id")?.as_str()?.trim();
            if object_type.is_empty() || object_id.is_empty() {
                return None;
            }
            Some(ExtractedReference {
                target_type: "local_object".to_string(),
                target_id: object_id.to_string(),
                target_object_type: Some(object_type.to_string()),
                reference_type: "local_object_mention".to_string(),
            })
        }
        _ => None,
    }
}

fn push_notes_link_references(
    references: &mut Vec<ExtractedReference>,
    url: &str,
    source: ReferenceSourceKind,
    link_resolver: &LocalLinkResolver,
) {
    for page_id in crate::links::page_ids_from_local_notes_url(url, link_resolver) {
        references.push(ExtractedReference::page(
            &page_id,
            match source {
                ReferenceSourceKind::Block => "link",
                ReferenceSourceKind::Comment => "comment_link",
            },
        ));
    }
}

fn is_notes_uuid(value: &str) -> bool {
    require_uuid(value, "id").is_ok()
}

fn backlink_snippet(plain_text: &str, block_type: &str) -> String {
    let trimmed = plain_text.trim();
    if trimmed.is_empty() {
        return block_type.to_string();
    }
    trimmed.chars().take(160).collect()
}

fn comment_snippet(plain_text: &str) -> String {
    let trimmed = plain_text.trim();
    if trimmed.is_empty() {
        return "Comment".to_string();
    }
    trimmed.chars().take(160).collect()
}

fn relation_snippet(property_name: &str) -> String {
    if property_name.trim().is_empty() {
        "Database relation".to_string()
    } else {
        property_name.trim().to_string()
    }
}

async fn backlink_source_fingerprint(
    pool: &SqlitePool,
) -> Result<BacklinkSourceFingerprint, String> {
    let rows = sqlx::query(
        "SELECT 'pages' AS source, COUNT(*) AS source_count,
            COALESCE(MAX(last_edited_time), '') AS source_marker
         FROM notes_pages
         UNION ALL
         SELECT 'blocks', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_blocks
         UNION ALL
         SELECT 'comment_threads', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_comment_threads
         UNION ALL
         SELECT 'comments', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_comments
         UNION ALL
         SELECT 'page_aliases', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_page_aliases
         UNION ALL
         SELECT 'relation_links', COUNT(*), COALESCE(MAX(created_time), '')
         FROM notes_data_source_relation_links
         UNION ALL
         SELECT 'data_sources', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_data_sources
         UNION ALL
         SELECT 'databases', COUNT(*), COALESCE(MAX(last_edited_time), '')
         FROM notes_databases",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("compute notes backlink source fingerprint: {e}"))?;

    let mut source_rows = 0;
    let mut value = String::new();
    for row in rows {
        let source: String = row.get("source");
        let source_count: i64 = row.get("source_count");
        let source_marker: String = row.get("source_marker");
        source_rows += source_count;
        value.push_str(&source);
        value.push(':');
        value.push_str(&source_count.to_string());
        value.push(':');
        value.push_str(&source_marker);
        value.push(';');
    }
    Ok(BacklinkSourceFingerprint { value, source_rows })
}

#[derive(Clone)]
struct SourceBacklinkEntry<'a> {
    source_type: &'a str,
    source_page_id: &'a str,
    source_block_id: Option<&'a str>,
    source_comment_id: Option<&'a str>,
    source_property_id: Option<&'a str>,
    source_property_name: &'a str,
    snippet: String,
    created_time: &'a str,
    last_edited_time: &'a str,
}

struct ExtractedReference {
    target_type: String,
    target_id: String,
    target_object_type: Option<String>,
    reference_type: String,
}

impl ExtractedReference {
    fn page(page_id: &str, reference_type: &str) -> Self {
        Self {
            target_type: "page".to_string(),
            target_id: page_id.to_string(),
            target_object_type: None,
            reference_type: reference_type.to_string(),
        }
    }

    fn dedup_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.target_type,
            self.target_object_type.as_deref().unwrap_or(""),
            self.target_id,
            self.reference_type
        )
    }
}

struct BacklinkIndexEntry {
    id: String,
    target_type: String,
    target_id: String,
    target_object_type: Option<String>,
    source_type: String,
    source_page_id: String,
    source_block_id: Option<String>,
    source_comment_id: Option<String>,
    source_property_id: Option<String>,
    source_property_name: String,
    reference_type: String,
    snippet: String,
    created_time: String,
    last_edited_time: String,
}

impl BacklinkIndexEntry {
    fn from_source(source: SourceBacklinkEntry<'_>, reference: ExtractedReference) -> Self {
        let source_id = source
            .source_comment_id
            .or(source.source_block_id)
            .or(source.source_property_id)
            .unwrap_or(source.source_page_id);
        let id = format!(
            "{}:{}:{}:{}:{}:{}",
            source.source_type,
            source_id,
            reference.target_type,
            reference.target_object_type.as_deref().unwrap_or(""),
            reference.target_id,
            reference.reference_type
        );
        Self {
            id,
            target_type: reference.target_type,
            target_id: reference.target_id,
            target_object_type: reference.target_object_type,
            source_type: source.source_type.to_string(),
            source_page_id: source.source_page_id.to_string(),
            source_block_id: source.source_block_id.map(str::to_string),
            source_comment_id: source.source_comment_id.map(str::to_string),
            source_property_id: source.source_property_id.map(str::to_string),
            source_property_name: source.source_property_name.to_string(),
            reference_type: reference.reference_type,
            snippet: source.snippet,
            created_time: source.created_time.to_string(),
            last_edited_time: source.last_edited_time.to_string(),
        }
    }
}

struct BacklinkSourceFingerprint {
    value: String,
    source_rows: i64,
}

#[derive(FromRow)]
struct BacklinkResultRow {
    id: String,
    source_type: String,
    source_page_id: String,
    source_block_id: Option<String>,
    reference_type: String,
    snippet: String,
    created_time: String,
    last_edited_time: String,
    source_block_type: Option<String>,
}

#[derive(FromRow)]
struct CommentBacklinkSourceRow {
    id: String,
    rich_text: String,
    plain_text: String,
    created_time: String,
    last_edited_time: String,
    page_id: String,
    parent_block_id: Option<String>,
}

#[derive(FromRow)]
struct RelationBacklinkSourceRow {
    source_page_id: String,
    source_property_id: String,
    source_property_name: String,
    target_page_id: String,
    created_time: String,
    last_edited_time: String,
}
