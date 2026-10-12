mod scanner;

use self::scanner::{
    ScannedReference, ScannedTarget, collect_json_references, external_url_reference,
};
use crate::links::LocalLinkResolver;
use serde_json::Value;
use sqlx::{FromRow, Row, SqlitePool};
use std::collections::{HashMap, HashSet};

const FINGERPRINT_KEY: &str = "source_fingerprint";

pub async fn rebuild_index(pool: &SqlitePool) -> Result<i64, String> {
    let fingerprint = source_fingerprint(pool).await?;
    let entries = build_entries(pool).await?;
    let entry_count = entries.len() as i64;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes link facts rebuild: {e}"))?;
    sqlx::query("DELETE FROM notes_link_facts")
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("clear notes link facts: {e}"))?;
    for entry in &entries {
        sqlx::query(
            "INSERT INTO notes_link_facts (
                id,
                source_object_type,
                source_object_id,
                source_page_id,
                source_block_id,
                source_comment_id,
                source_data_source_id,
                source_property_id,
                source_property_name,
                target_object_type,
                target_object_id,
                target_page_id,
                target_block_id,
                target_comment_id,
                target_asset_id,
                target_url,
                link_type,
                snippet,
                created_time,
                last_edited_time
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.id)
        .bind(&entry.source.object_type)
        .bind(&entry.source.object_id)
        .bind(&entry.source.page_id)
        .bind(&entry.source.block_id)
        .bind(&entry.source.comment_id)
        .bind(&entry.source.data_source_id)
        .bind(&entry.source.property_id)
        .bind(&entry.source.property_name)
        .bind(&entry.target.object_type)
        .bind(&entry.target.object_id)
        .bind(&entry.target.page_id)
        .bind(&entry.target.block_id)
        .bind(&entry.target.comment_id)
        .bind(&entry.target.asset_id)
        .bind(&entry.target.url)
        .bind(entry.link_type)
        .bind(&entry.snippet)
        .bind(&entry.created_time)
        .bind(&entry.last_edited_time)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("insert notes link fact: {e}"))?;
    }
    sqlx::query(
        "INSERT INTO notes_link_facts_state (key, value, updated_at)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = excluded.updated_at",
    )
    .bind(FINGERPRINT_KEY)
    .bind(&fingerprint.value)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record notes link facts fingerprint: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes link facts rebuild: {e}"))?;
    Ok(entry_count)
}

async fn build_entries(pool: &SqlitePool) -> Result<Vec<LinkFactEntry>, String> {
    let catalog = TargetCatalog::load(pool).await?;
    let link_resolver = crate::links::local_link_resolver(pool).await?;
    let mut entries = Vec::new();
    append_page_metadata_entries(pool, &mut entries).await?;
    append_block_entries(pool, &catalog, &link_resolver, &mut entries).await?;
    append_property_entries(pool, &catalog, &link_resolver, &mut entries).await?;
    append_comment_entries(pool, &catalog, &link_resolver, &mut entries).await?;
    append_relation_entries(pool, &catalog, &mut entries).await?;
    append_asset_entries(pool, &mut entries).await?;
    deduplicate_entries(entries)
}

async fn append_page_metadata_entries(
    pool: &SqlitePool,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, PageMetadataRow>(
        "SELECT id, parent_type, icon, cover, created_time, last_edited_time
         FROM notes_pages
         WHERE in_trash = 0 AND archived = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes pages for link facts: {e}"))?;
    for row in rows {
        let source = LinkSource::page(&row.id, page_object_type(&row.parent_type));
        for (field, link_type) in [(&row.icon, "page_icon"), (&row.cover, "page_cover")] {
            if field.trim().is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(field)
                .map_err(|e| format!("parse notes page media link fact: {e}"))?;
            if let Some(url) = page_media_external_url(&value) {
                if let Some(reference) = external_url_reference(url, link_type) {
                    push_external_reference(
                        entries,
                        &source,
                        reference,
                        &row.created_time,
                        &row.last_edited_time,
                    );
                }
            }
        }
    }
    Ok(())
}

async fn append_block_entries(
    pool: &SqlitePool,
    catalog: &TargetCatalog,
    link_resolver: &LocalLinkResolver,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, BlockFactRow>(
        "SELECT block.id,
                block.page_id,
                block.type AS block_type,
                block.payload,
                block.plain_text,
                block.created_time,
                block.last_edited_time
         FROM notes_blocks AS block
         JOIN notes_pages AS page ON page.id = block.page_id
         WHERE block.in_trash = 0
           AND page.in_trash = 0
           AND page.archived = 0
           AND (
               page.parent_type != 'data_source_id'
               OR EXISTS (
                   SELECT 1
                   FROM notes_data_sources AS data_source
                   JOIN notes_databases AS database ON database.id = data_source.database_id
                   WHERE data_source.id = page.parent_data_source_id
                     AND data_source.in_trash = 0
                     AND database.in_trash = 0
               )
           )",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes blocks for link facts: {e}"))?;
    for row in rows {
        let source = LinkSource::block(&row.id, &row.page_id);
        if row.block_type == "child_page" {
            if let Some(target) = catalog.page_target(&row.id) {
                entries.push(LinkFactEntry::new(
                    source.clone(),
                    target,
                    "child_page",
                    source_snippet(&row.plain_text, &row.block_type),
                    &row.created_time,
                    &row.last_edited_time,
                ));
            }
        }
        let payload: Value = serde_json::from_str(&row.payload)
            .map_err(|e| format!("parse notes block link facts: {e}"))?;
        push_scanned_many(
            entries,
            &source,
            collect_json_references(&payload, link_resolver),
            catalog,
            &row.created_time,
            &row.last_edited_time,
        );
    }
    Ok(())
}

async fn append_property_entries(
    pool: &SqlitePool,
    catalog: &TargetCatalog,
    link_resolver: &LocalLinkResolver,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, PropertyFactRow>(
        "SELECT page.id AS page_id,
                page.parent_data_source_id AS data_source_id,
                page.properties,
                page.created_time,
                page.last_edited_time
         FROM notes_pages AS page
         JOIN notes_data_sources AS data_source ON data_source.id = page.parent_data_source_id
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE page.parent_type = 'data_source_id'
           AND page.in_trash = 0
           AND page.archived = 0
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes row properties for link facts: {e}"))?;
    for row in rows {
        let properties: Value = serde_json::from_str(&row.properties)
            .map_err(|e| format!("parse notes property link facts: {e}"))?;
        let Some(properties) = properties.as_object() else {
            continue;
        };
        for (name, property) in properties {
            let property_id = property
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(name)
                .trim();
            if property_id.is_empty() {
                continue;
            }
            let source =
                LinkSource::property(&row.data_source_id, property_id, name, Some(&row.page_id));
            push_scanned_many(
                entries,
                &source,
                collect_json_references(property, link_resolver),
                catalog,
                &row.created_time,
                &row.last_edited_time,
            );
        }
    }
    Ok(())
}

async fn append_comment_entries(
    pool: &SqlitePool,
    catalog: &TargetCatalog,
    link_resolver: &LocalLinkResolver,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, CommentFactRow>(
        "SELECT comment.id,
                comment.rich_text,
                comment.attachments,
                comment.created_time,
                comment.last_edited_time,
                thread.page_id
         FROM notes_comments AS comment
         JOIN notes_comment_threads AS thread ON thread.id = comment.thread_id
         JOIN notes_pages AS page ON page.id = thread.page_id
         LEFT JOIN notes_blocks AS block ON block.id = thread.parent_block_id
         WHERE comment.deleted_at IS NULL
           AND page.in_trash = 0
           AND page.archived = 0
           AND (
               page.parent_type != 'data_source_id'
               OR EXISTS (
                   SELECT 1
                   FROM notes_data_sources AS data_source
                   JOIN notes_databases AS database ON database.id = data_source.database_id
                   WHERE data_source.id = page.parent_data_source_id
                     AND data_source.in_trash = 0
                     AND database.in_trash = 0
               )
           )
           AND (thread.parent_block_id IS NULL OR (block.id IS NOT NULL AND block.in_trash = 0))",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes comments for link facts: {e}"))?;
    for row in rows {
        let source = LinkSource::comment(&row.id, &row.page_id);
        for raw in [&row.rich_text, &row.attachments] {
            let value: Value = serde_json::from_str(raw)
                .map_err(|e| format!("parse notes comment link facts: {e}"))?;
            push_scanned_many(
                entries,
                &source,
                collect_json_references(&value, link_resolver),
                catalog,
                &row.created_time,
                &row.last_edited_time,
            );
        }
    }
    Ok(())
}

async fn append_relation_entries(
    pool: &SqlitePool,
    catalog: &TargetCatalog,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, RelationFactRow>(
        "SELECT link.source_page_id,
                link.source_data_source_id,
                link.source_property_id,
                link.source_property_name,
                link.target_page_id,
                link.created_time
         FROM notes_data_source_relation_links AS link
         JOIN notes_pages AS source_page ON source_page.id = link.source_page_id
         JOIN notes_pages AS target_page ON target_page.id = link.target_page_id
         WHERE source_page.in_trash = 0
           AND source_page.archived = 0
           AND target_page.in_trash = 0
           AND target_page.archived = 0
           AND EXISTS (
               SELECT 1
               FROM notes_data_sources AS source_data_source
               JOIN notes_databases AS source_database
                 ON source_database.id = source_data_source.database_id
               WHERE source_data_source.id = link.source_data_source_id
                 AND source_data_source.in_trash = 0
                 AND source_database.in_trash = 0
           )
           AND (
               target_page.parent_type != 'data_source_id'
               OR EXISTS (
                   SELECT 1
                   FROM notes_data_sources AS target_data_source
                   JOIN notes_databases AS target_database
                     ON target_database.id = target_data_source.database_id
                   WHERE target_data_source.id = target_page.parent_data_source_id
                     AND target_data_source.in_trash = 0
                     AND target_database.in_trash = 0
               )
           )",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes relation link facts: {e}"))?;
    for row in rows {
        if let Some(target) = catalog.page_target(&row.target_page_id) {
            entries.push(LinkFactEntry::new(
                LinkSource::property(
                    &row.source_data_source_id,
                    &row.source_property_id,
                    &row.source_property_name,
                    Some(&row.source_page_id),
                ),
                target,
                "database_relation",
                relation_snippet(&row.source_property_name),
                &row.created_time,
                &row.created_time,
            ));
        }
    }
    Ok(())
}

async fn append_asset_entries(
    pool: &SqlitePool,
    entries: &mut Vec<LinkFactEntry>,
) -> Result<(), String> {
    let rows = sqlx::query_as::<_, AssetFactRow>(
        "SELECT reference.asset_id,
                reference.owner_type,
                reference.owner_id,
                reference.page_id,
                reference.block_id,
                reference.comment_id,
                reference.data_source_id,
                reference.property_id,
                reference.role,
                reference.created_at,
                reference.updated_at
         FROM notes_asset_references AS reference
         JOIN notes_assets AS asset ON asset.id = reference.asset_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load notes asset link facts: {e}"))?;
    for row in rows {
        let Some(source) = asset_source(&row) else {
            continue;
        };
        entries.push(LinkFactEntry::new(
            source,
            LinkTarget::file(&row.asset_id),
            asset_link_type(&row.role),
            row.role.clone(),
            &row.created_at,
            &row.updated_at,
        ));
    }
    Ok(())
}

fn push_scanned_many(
    entries: &mut Vec<LinkFactEntry>,
    source: &LinkSource,
    references: Vec<ScannedReference>,
    catalog: &TargetCatalog,
    created_time: &str,
    last_edited_time: &str,
) {
    for reference in references {
        if let Some(target) = scanned_target(catalog, &reference.target) {
            push_scanned(
                entries,
                source,
                reference.with_target(target),
                created_time,
                last_edited_time,
            );
        }
    }
}

fn push_external_reference(
    entries: &mut Vec<LinkFactEntry>,
    source: &LinkSource,
    reference: ScannedReference,
    created_time: &str,
    last_edited_time: &str,
) {
    if let ScannedTarget::ExternalUrl(url) = reference.target {
        push_scanned(
            entries,
            source,
            ResolvedReference {
                target: LinkTarget::external_url(url),
                link_type: reference.link_type,
                label: reference.label,
            },
            created_time,
            last_edited_time,
        );
    }
}

fn push_scanned(
    entries: &mut Vec<LinkFactEntry>,
    source: &LinkSource,
    reference: ResolvedReference,
    created_time: &str,
    last_edited_time: &str,
) {
    entries.push(LinkFactEntry::new(
        source.clone(),
        reference.target,
        reference.link_type,
        reference.label,
        created_time,
        last_edited_time,
    ));
}

fn scanned_target(catalog: &TargetCatalog, target: &ScannedTarget) -> Option<LinkTarget> {
    match target {
        ScannedTarget::Page(page_id) => catalog.page_target(page_id),
        ScannedTarget::Block(block_id) => catalog.block_target(block_id),
        ScannedTarget::Database(database_id) => {
            Some(LinkTarget::simple("database", database_id.clone()))
        }
        ScannedTarget::LocalObject { object_type, id } => {
            Some(LinkTarget::simple(object_type, id.clone()))
        }
        ScannedTarget::ExternalUrl(url) => Some(LinkTarget::external_url(url.clone())),
    }
}

impl ScannedReference {
    fn with_target(self, target: LinkTarget) -> ResolvedReference {
        ResolvedReference {
            target,
            link_type: self.link_type,
            label: self.label,
        }
    }
}

struct ResolvedReference {
    target: LinkTarget,
    link_type: &'static str,
    label: String,
}

fn deduplicate_entries(entries: Vec<LinkFactEntry>) -> Result<Vec<LinkFactEntry>, String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for mut entry in entries {
        let key = entry.dedup_key();
        entry.id = format!("fact:{}", stable_hex(&key));
        if seen.insert(entry.id.clone()) {
            deduped.push(entry);
        }
    }
    Ok(deduped)
}

async fn source_fingerprint(pool: &SqlitePool) -> Result<SourceFingerprint, String> {
    let rows = sqlx::query(
        "SELECT 'pages' AS source, COUNT(*) AS source_count, COALESCE(MAX(last_edited_time), '') AS source_marker FROM notes_pages
         UNION ALL SELECT 'blocks', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_blocks
         UNION ALL SELECT 'comments', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_comments
         UNION ALL SELECT 'comment_threads', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_comment_threads
         UNION ALL SELECT 'data_sources', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_data_sources
         UNION ALL SELECT 'databases', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_databases
         UNION ALL SELECT 'relation_links', COUNT(*), COALESCE(MAX(created_time), '') FROM notes_data_source_relation_links
         UNION ALL SELECT 'assets', COUNT(*), COALESCE(MAX(updated_at), '') FROM notes_assets
         UNION ALL SELECT 'asset_references', COUNT(*), COALESCE(MAX(updated_at), '') FROM notes_asset_references
         UNION ALL SELECT 'page_aliases', COUNT(*), COALESCE(MAX(last_edited_time), '') FROM notes_page_aliases",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("compute notes link facts fingerprint: {e}"))?;
    let mut value = String::new();
    for row in rows {
        let source: String = row.get("source");
        let count: i64 = row.get("source_count");
        let marker: String = row.get("source_marker");
        value.push_str(&source);
        value.push(':');
        value.push_str(&count.to_string());
        value.push(':');
        value.push_str(&marker);
        value.push(';');
    }
    Ok(SourceFingerprint { value })
}

fn page_media_external_url(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    match object.get("type").and_then(Value::as_str)? {
        "external" => object
            .get("external")
            .and_then(|external| external.get("url"))
            .and_then(Value::as_str),
        _ => None,
    }
}

fn asset_source(row: &AssetFactRow) -> Option<LinkSource> {
    match row.owner_type.as_str() {
        "page" => row
            .page_id
            .as_deref()
            .map(|page_id| LinkSource::page(page_id, "page")),
        "block" => Some(LinkSource::block(
            row.block_id.as_deref()?,
            row.page_id.as_deref()?,
        )),
        "comment" => Some(LinkSource::comment(
            row.comment_id.as_deref()?,
            row.page_id.as_deref()?,
        )),
        "data_source_property" => Some(LinkSource::property(
            row.data_source_id.as_deref()?,
            row.property_id.as_deref()?,
            "",
            None,
        )),
        "import" => Some(LinkSource::import(&row.owner_id)),
        _ => None,
    }
}

fn asset_link_type(role: &str) -> &'static str {
    match role {
        "page_icon" => "page_icon",
        "page_cover" => "page_cover",
        "block_file" => "block_file",
        "callout_icon" => "callout_icon",
        "property_file" => "property_file",
        "comment_attachment" => "comment_attachment",
        _ => "import_source",
    }
}

fn page_object_type(parent_type: &str) -> &'static str {
    if parent_type == "data_source_id" {
        "database_row"
    } else {
        "page"
    }
}

fn source_snippet(plain_text: &str, block_type: &str) -> String {
    let trimmed = plain_text.trim();
    if trimmed.is_empty() {
        block_type.to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn relation_snippet(property_name: &str) -> String {
    if property_name.trim().is_empty() {
        "Database relation".to_string()
    } else {
        property_name.trim().chars().take(160).collect()
    }
}

fn stable_hex(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[derive(Clone)]
struct LinkSource {
    object_type: String,
    object_id: String,
    page_id: Option<String>,
    block_id: Option<String>,
    comment_id: Option<String>,
    data_source_id: Option<String>,
    property_id: Option<String>,
    property_name: String,
}

impl LinkSource {
    fn page(page_id: &str, object_type: &str) -> Self {
        Self {
            object_type: object_type.to_string(),
            object_id: page_id.to_string(),
            page_id: Some(page_id.to_string()),
            block_id: None,
            comment_id: None,
            data_source_id: None,
            property_id: None,
            property_name: String::new(),
        }
    }

    fn block(block_id: &str, page_id: &str) -> Self {
        Self {
            object_type: "block".to_string(),
            object_id: block_id.to_string(),
            page_id: Some(page_id.to_string()),
            block_id: Some(block_id.to_string()),
            comment_id: None,
            data_source_id: None,
            property_id: None,
            property_name: String::new(),
        }
    }

    fn comment(comment_id: &str, page_id: &str) -> Self {
        Self {
            object_type: "comment".to_string(),
            object_id: comment_id.to_string(),
            page_id: Some(page_id.to_string()),
            block_id: None,
            comment_id: Some(comment_id.to_string()),
            data_source_id: None,
            property_id: None,
            property_name: String::new(),
        }
    }

    fn property(
        data_source_id: &str,
        property_id: &str,
        property_name: &str,
        page_id: Option<&str>,
    ) -> Self {
        let object_id = match page_id {
            Some(page_id) => format!("{page_id}:{property_id}"),
            None => format!("{data_source_id}:{property_id}"),
        };
        Self {
            object_type: "property".to_string(),
            object_id,
            page_id: page_id.map(str::to_string),
            block_id: None,
            comment_id: None,
            data_source_id: Some(data_source_id.to_string()),
            property_id: Some(property_id.to_string()),
            property_name: property_name.to_string(),
        }
    }

    fn import(owner_id: &str) -> Self {
        Self {
            object_type: "import".to_string(),
            object_id: owner_id.to_string(),
            page_id: None,
            block_id: None,
            comment_id: None,
            data_source_id: None,
            property_id: None,
            property_name: String::new(),
        }
    }
}

#[derive(Clone)]
struct LinkTarget {
    object_type: String,
    object_id: String,
    page_id: Option<String>,
    block_id: Option<String>,
    comment_id: Option<String>,
    asset_id: Option<String>,
    url: Option<String>,
}

impl LinkTarget {
    fn page(page_id: &str, object_type: &str) -> Self {
        Self {
            object_type: object_type.to_string(),
            object_id: page_id.to_string(),
            page_id: Some(page_id.to_string()),
            block_id: None,
            comment_id: None,
            asset_id: None,
            url: None,
        }
    }

    fn block(block_id: &str) -> Self {
        Self {
            object_type: "block".to_string(),
            object_id: block_id.to_string(),
            page_id: None,
            block_id: Some(block_id.to_string()),
            comment_id: None,
            asset_id: None,
            url: None,
        }
    }

    fn file(asset_id: &str) -> Self {
        Self {
            object_type: "file".to_string(),
            object_id: asset_id.to_string(),
            page_id: None,
            block_id: None,
            comment_id: None,
            asset_id: Some(asset_id.to_string()),
            url: None,
        }
    }

    fn external_url(url: String) -> Self {
        Self {
            object_type: "external_url".to_string(),
            object_id: url.clone(),
            page_id: None,
            block_id: None,
            comment_id: None,
            asset_id: None,
            url: Some(url),
        }
    }

    fn simple(object_type: &str, object_id: String) -> Self {
        Self {
            object_type: object_type.to_string(),
            object_id,
            page_id: None,
            block_id: None,
            comment_id: None,
            asset_id: None,
            url: None,
        }
    }
}

struct LinkFactEntry {
    id: String,
    source: LinkSource,
    target: LinkTarget,
    link_type: &'static str,
    snippet: String,
    created_time: String,
    last_edited_time: String,
}

impl LinkFactEntry {
    fn new(
        source: LinkSource,
        target: LinkTarget,
        link_type: &'static str,
        snippet: impl Into<String>,
        created_time: &str,
        last_edited_time: &str,
    ) -> Self {
        Self {
            id: String::new(),
            source,
            target,
            link_type,
            snippet: snippet.into().chars().take(160).collect(),
            created_time: created_time.to_string(),
            last_edited_time: last_edited_time.to_string(),
        }
    }

    fn dedup_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.source.object_type,
            self.source.object_id,
            self.source.property_id.as_deref().unwrap_or_default(),
            self.target.object_type,
            self.target.object_id,
            self.link_type
        )
    }
}

struct TargetCatalog {
    page_types: HashMap<String, String>,
    block_ids: HashSet<String>,
}

impl TargetCatalog {
    async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let page_rows = sqlx::query_as::<_, TargetPageRow>(
            "SELECT page.id, page.parent_type
             FROM notes_pages AS page
             WHERE page.in_trash = 0
               AND page.archived = 0
               AND (
                   page.parent_type != 'data_source_id'
                   OR EXISTS (
                       SELECT 1
                       FROM notes_data_sources AS data_source
                       JOIN notes_databases AS database ON database.id = data_source.database_id
                       WHERE data_source.id = page.parent_data_source_id
                         AND data_source.in_trash = 0
                         AND database.in_trash = 0
                   )
               )",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes link fact page targets: {e}"))?;
        let block_rows = sqlx::query_as::<_, TargetBlockRow>(
            "SELECT block.id
             FROM notes_blocks AS block
             JOIN notes_pages AS page ON page.id = block.page_id
             WHERE block.in_trash = 0
               AND page.in_trash = 0
               AND page.archived = 0",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load notes link fact block targets: {e}"))?;
        Ok(Self {
            page_types: page_rows
                .into_iter()
                .map(|row| (row.id, page_object_type(&row.parent_type).to_string()))
                .collect(),
            block_ids: block_rows.into_iter().map(|row| row.id).collect(),
        })
    }

    fn page_target(&self, page_id: &str) -> Option<LinkTarget> {
        self.page_types
            .get(page_id)
            .map(|object_type| LinkTarget::page(page_id, object_type))
    }

    fn block_target(&self, block_id: &str) -> Option<LinkTarget> {
        self.block_ids
            .contains(block_id)
            .then(|| LinkTarget::block(block_id))
    }
}

struct SourceFingerprint {
    value: String,
}

#[derive(FromRow)]
struct TargetPageRow {
    id: String,
    parent_type: String,
}

#[derive(FromRow)]
struct TargetBlockRow {
    id: String,
}

#[derive(FromRow)]
struct PageMetadataRow {
    id: String,
    parent_type: String,
    icon: String,
    cover: String,
    created_time: String,
    last_edited_time: String,
}

#[derive(FromRow)]
struct BlockFactRow {
    id: String,
    page_id: String,
    block_type: String,
    payload: String,
    plain_text: String,
    created_time: String,
    last_edited_time: String,
}

#[derive(FromRow)]
struct PropertyFactRow {
    page_id: String,
    data_source_id: String,
    properties: String,
    created_time: String,
    last_edited_time: String,
}

#[derive(FromRow)]
struct CommentFactRow {
    id: String,
    rich_text: String,
    attachments: String,
    created_time: String,
    last_edited_time: String,
    page_id: String,
}

#[derive(FromRow)]
struct RelationFactRow {
    source_page_id: String,
    source_data_source_id: String,
    source_property_id: String,
    source_property_name: String,
    target_page_id: String,
    created_time: String,
}

#[derive(FromRow)]
struct AssetFactRow {
    asset_id: String,
    owner_type: String,
    owner_id: String,
    page_id: Option<String>,
    block_id: Option<String>,
    comment_id: Option<String>,
    data_source_id: Option<String>,
    property_id: Option<String>,
    role: String,
    created_at: String,
    updated_at: String,
}
