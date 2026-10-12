use crate::models::NotePageAliasRow;
use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::{FromRow, SqlitePool};
use std::collections::{HashMap, HashSet};

pub async fn local_link_resolver(pool: &SqlitePool) -> Result<LocalLinkResolver, String> {
    let rows = sqlx::query_as::<_, LocalLinkPageTargetRow>(
        "SELECT page.id,
                page.title
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
    .map_err(|e| format!("load notes local link pages: {e}"))?;
    let aliases = sqlx::query_as::<_, NotePageAliasRow>(
        "SELECT alias.*
         FROM notes_page_aliases AS alias
         JOIN notes_pages AS page ON page.id = alias.page_id
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
    .map_err(|e| format!("load notes page aliases for local links: {e}"))?;

    let mut active_page_ids = HashSet::new();
    let mut title_targets = HashMap::<String, Option<String>>::new();
    for row in rows {
        active_page_ids.insert(row.id.clone());
        if let Some(normalized) = normalize_alias(&row.title) {
            insert_ambiguous_target(&mut title_targets, normalized, row.id);
        }
    }
    let alias_targets = aliases
        .into_iter()
        .map(|row| (row.normalized_alias, row.page_id))
        .collect();
    Ok(LocalLinkResolver {
        active_page_ids,
        alias_targets,
        title_targets,
    })
}

pub fn page_ids_from_local_notes_url(url: &str, resolver: &LocalLinkResolver) -> Vec<String> {
    resolver
        .resolve_url(url)
        .page_id
        .into_iter()
        .collect::<Vec<_>>()
}

pub fn block_id_from_local_notes_url(url: &str) -> Option<String> {
    query_value(url, "block").and_then(|value| canonical_notes_id(&value))
}

pub(super) fn unresolved_candidates(
    value: &Value,
    resolver: &LocalLinkResolver,
) -> Vec<UnresolvedCandidate> {
    let mut candidates = Vec::new();
    collect_local_link_candidates(value, &mut candidates);
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let resolution = resolver.resolve_url(&candidate.raw_url);
            let target = resolution.unresolved_target?;
            Some(UnresolvedCandidate {
                raw_url: candidate.raw_url,
                raw_target: target.raw_target,
                normalized_target: target.normalized_target,
                link_text: candidate.link_text,
            })
        })
        .collect()
}

pub(super) fn normalize_alias(value: &str) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn collect_local_link_candidates(value: &Value, candidates: &mut Vec<LocalLinkCandidate>) {
    match value {
        Value::Object(object) => {
            let text_label = object
                .get("plain_text")
                .or_else(|| object.get("text").and_then(|text| text.get("content")))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if let Some(url) = object
                .get("text")
                .and_then(|text| text.get("link"))
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
            {
                push_local_link_candidate(candidates, url, &text_label);
            }
            for field in ["href", "url"] {
                if let Some(url) = object.get(field).and_then(Value::as_str) {
                    push_local_link_candidate(candidates, url, &text_label);
                }
            }
            if let Some(url) = object
                .get("link")
                .and_then(|link| link.get("url"))
                .and_then(Value::as_str)
            {
                push_local_link_candidate(candidates, url, &text_label);
            }
            for nested in object.values() {
                collect_local_link_candidates(nested, candidates);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_local_link_candidates(item, candidates);
            }
        }
        _ => {}
    }
}

fn push_local_link_candidate(candidates: &mut Vec<LocalLinkCandidate>, url: &str, label: &str) {
    if !url.contains("#notes?") {
        return;
    }
    candidates.push(LocalLinkCandidate {
        raw_url: url.trim().to_string(),
        link_text: label.trim().chars().take(200).collect(),
    });
}

fn insert_ambiguous_target(
    targets: &mut HashMap<String, Option<String>>,
    normalized: String,
    page_id: String,
) {
    match targets.get_mut(&normalized) {
        Some(existing) if existing.as_deref() == Some(page_id.as_str()) => {}
        Some(existing) => *existing = None,
        None => {
            targets.insert(normalized, Some(page_id));
        }
    }
}

pub fn canonical_notes_id(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_matches(|character: char| matches!(character, '"' | '\'' | ')' | '(' | ',' | '.'));
    if trimmed.len() == 36 && require_uuid(trimmed, "id").is_ok() {
        return Some(trimmed.to_ascii_lowercase());
    }
    if trimmed.len() == 32
        && trimmed
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        let lower = trimmed.to_ascii_lowercase();
        let candidate = format!(
            "{}-{}-{}-{}-{}",
            &lower[0..8],
            &lower[8..12],
            &lower[12..16],
            &lower[16..20],
            &lower[20..32]
        );
        if require_uuid(&candidate, "id").is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once("#notes?")?.1;
    for pair in query.split(['&', '#']) {
        let (pair_key, value) = pair.split_once('=')?;
        if pair_key == key {
            return Some(decode_query_component(value));
        }
    }
    None
}

fn decode_query_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]);
                let low = hex_value(bytes[index + 2]);
                if let (Some(high), Some(low)) = (high, low) {
                    output.push(high * 16 + low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub struct LocalLinkResolver {
    active_page_ids: HashSet<String>,
    alias_targets: HashMap<String, String>,
    title_targets: HashMap<String, Option<String>>,
}

impl LocalLinkResolver {
    fn resolve_url(&self, url: &str) -> LocalLinkResolution {
        if !url.contains("#notes?") {
            return LocalLinkResolution::none();
        }
        if let Some(page_value) = query_value(url, "page") {
            if let Some(page_id) = canonical_notes_id(&page_value) {
                if self.active_page_ids.contains(&page_id) {
                    return LocalLinkResolution::page(page_id);
                }
                return LocalLinkResolution::unresolved(page_value);
            }
            if !page_value.trim().is_empty() {
                return self.resolve_named_target(page_value);
            }
        }
        for key in ["alias", "target", "title"] {
            if let Some(value) = query_value(url, key).filter(|value| !value.trim().is_empty()) {
                return self.resolve_named_target(value);
            }
        }
        for token in
            url.split(|character: char| !(character.is_ascii_hexdigit() || character == '-'))
        {
            if let Some(page_id) = canonical_notes_id(token) {
                if self.active_page_ids.contains(&page_id) {
                    return LocalLinkResolution::page(page_id);
                }
                return LocalLinkResolution::unresolved(page_id);
            }
        }
        LocalLinkResolution::none()
    }

    fn resolve_named_target(&self, raw_target: String) -> LocalLinkResolution {
        let Some(normalized) = normalize_alias(&raw_target) else {
            return LocalLinkResolution::none();
        };
        if let Some(page_id) = self.alias_targets.get(&normalized) {
            return LocalLinkResolution::page(page_id.clone());
        }
        if let Some(Some(page_id)) = self.title_targets.get(&normalized) {
            return LocalLinkResolution::page(page_id.clone());
        }
        LocalLinkResolution::unresolved_with_normalized(raw_target, normalized)
    }
}

struct LocalLinkResolution {
    page_id: Option<String>,
    unresolved_target: Option<UnresolvedTarget>,
}

impl LocalLinkResolution {
    fn none() -> Self {
        Self {
            page_id: None,
            unresolved_target: None,
        }
    }

    fn page(page_id: String) -> Self {
        Self {
            page_id: Some(page_id),
            unresolved_target: None,
        }
    }

    fn unresolved(raw_target: String) -> Self {
        match normalize_alias(&raw_target) {
            Some(normalized) => Self::unresolved_with_normalized(raw_target, normalized),
            None => Self::none(),
        }
    }

    fn unresolved_with_normalized(raw_target: String, normalized_target: String) -> Self {
        Self {
            page_id: None,
            unresolved_target: Some(UnresolvedTarget {
                raw_target: raw_target.trim().chars().take(200).collect(),
                normalized_target,
            }),
        }
    }
}

struct UnresolvedTarget {
    raw_target: String,
    normalized_target: String,
}

struct LocalLinkCandidate {
    raw_url: String,
    link_text: String,
}

pub(super) struct UnresolvedCandidate {
    pub(super) raw_url: String,
    pub(super) raw_target: String,
    pub(super) normalized_target: String,
    pub(super) link_text: String,
}

#[derive(FromRow)]
struct LocalLinkPageTargetRow {
    id: String,
    title: String,
}
