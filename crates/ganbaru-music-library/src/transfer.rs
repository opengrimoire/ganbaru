//! Native Music transfer preparation and revision-checked, retry-safe import.

use super::transfer_codec::{self as codec, validation};
use super::transfer_read::{ITEM_TEXT_COLUMNS, TransferReadBudget};
use super::*;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTransferSource {
    pub contents: String,
    pub playlist_name: String,
    pub relative_root_id: Option<String>,
    pub selected_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTransferCommit {
    pub action_id: String,
    pub source: MusicTransferSource,
    pub expected_revision: String,
    pub playlist_conflict: MusicImportPlaylistConflict,
    pub replace_item_descriptions: bool,
    pub import_context_assignments: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MusicTransferFormat {
    Json,
    M3u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTransferExport {
    pub playlist_ids: Vec<String>,
    pub format: MusicTransferFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MusicTransferPreview {
    pub format: MusicTransferFormat,
    pub revision: String,
    pub roots: Vec<MusicInterchangeRoot>,
    pub available_roots: Vec<MusicInterchangeRoot>,
    pub bound_root_ids: Vec<String>,
    pub context_assignment_count: usize,
    pub new_playlists: usize,
    pub matched_playlists: usize,
    pub new_items: usize,
    pub matched_items: usize,
    pub duplicate_items: usize,
    pub missing_local_bindings: usize,
    pub conflicts: Vec<String>,
    pub unsupported: Vec<String>,
    pub local_count: usize,
    pub youtube_count: usize,
    pub unsupported_count: usize,
    pub unresolved_local_count: usize,
}

/// Device-local folder bound to a Music library root, as seen by transfer.
#[derive(Clone, Debug, Serialize)]
pub struct TransferBinding {
    pub root_id: String,
    pub folder_path: String,
    pub available: bool,
    pub windows: bool,
}

/// Builds transfer bindings from native device-local root bindings; no frontend path is trusted.
pub fn bindings<'a>(
    roots: impl IntoIterator<Item = (&'a String, &'a String)>,
) -> MusicLibraryResult<Vec<TransferBinding>> {
    let mut bindings = Vec::new();
    for (root_id, path) in roots {
        if bindings.len() >= codec::MAX_PLAYLISTS {
            return Err(validation("Too many Music root bindings"));
        }
        let windows = path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\");
        #[cfg(target_os = "android")]
        let available = path.starts_with("content://");
        #[cfg(not(target_os = "android"))]
        let available = std::path::Path::new(path).is_dir();
        bindings.push(TransferBinding {
            root_id: root_id.clone(),
            folder_path: path.clone(),
            available,
            windows,
        });
    }
    Ok(bindings)
}

fn serialized<T: Serialize>(value: &T) -> MusicLibraryResult<String> {
    serde_json::to_string(value).map_err(|error| validation(error.to_string()))
}

fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

fn import_request(
    document: MusicInterchangeDocument,
    commit: Option<&MusicTransferCommit>,
) -> MusicInterchangeImportRequest {
    MusicInterchangeImportRequest {
        document,
        playlist_conflict: commit.map_or(MusicImportPlaylistConflict::ImportCopy, |request| {
            request.playlist_conflict
        }),
        replace_item_descriptions: commit.is_some_and(|request| request.replace_item_descriptions),
        import_context_assignments: commit
            .is_some_and(|request| request.import_context_assignments),
        imported_at: now_ms(),
    }
}

/// Prepares a bounded preview without returning the source document or library rows over IPC.
pub async fn preview(
    pool: &SqlitePool,
    source: &MusicTransferSource,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<MusicTransferPreview> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music transfer preview", error))?;
    let (preview, _) = prepare(&mut transaction, source, bindings).await?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("finish music transfer preview", error))?;
    Ok(preview)
}

/// Commits only a matching preview and stores its receipt in the same transaction.
pub async fn commit(
    pool: &SqlitePool,
    request: MusicTransferCommit,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<MusicInterchangeImportResult> {
    validate_id(&request.action_id, "actionId")?;
    if request.expected_revision.len() != 64 {
        return Err(validation("A Music import preview is required"));
    }
    codec::check_contents(&request.source.contents)?;
    let request_hash = codec::hash(serialized(&request)?.as_bytes());
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| MusicLibraryError::database("begin prepared music import", error))?;
    if let Some((prior_hash, result)) = sqlx::query_as::<_, (String, String)>(
        "SELECT request_hash, result_json FROM music_transfer_receipts WHERE action_id = ?",
    )
    .bind(&request.action_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("read Music import receipt", error))?
    {
        if prior_hash != request_hash {
            return Err(MusicLibraryError::conflict(
                "The Music import action was reused with different input",
            ));
        }
        return serde_json::from_str(&result)
            .map_err(|error| MusicLibraryError::runtime("decode Music import receipt", error));
    }
    let (preview, document) = prepare(&mut transaction, &request.source, bindings).await?;
    if preview.revision != request.expected_revision {
        return Err(MusicLibraryError::stale("Music import preview", "reviewed"));
    }
    let imported_at = now_ms();
    let mut import = import_request(document, Some(&request));
    import.imported_at = imported_at;
    let operation_id = codec::hash(request.action_id.as_bytes());
    let result =
        super::interchange::import_in_transaction(&mut transaction, &import, &operation_id).await?;
    sqlx::query("INSERT INTO music_transfer_receipts (action_id, request_hash, result_json, committed_at_ms) VALUES (?, ?, ?, ?)")
        .bind(&request.action_id).bind(request_hash).bind(serialized(&result)?).bind(imported_at)
        .execute(&mut *transaction).await.map_err(|error| MusicLibraryError::database("record Music import receipt", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit prepared Music import", error))?;
    Ok(result)
}

/// Finishes the database snapshot before serialization and native output selection.
pub async fn export(
    pool: &SqlitePool,
    request: &MusicTransferExport,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin Music export", error))?;
    let document = super::transfer_export::snapshot(
        &mut transaction,
        &request.playlist_ids,
        now_ms(),
        bindings,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("finish Music export snapshot", error))?;
    match request.format {
        MusicTransferFormat::Json => codec::serialize_json(document),
        MusicTransferFormat::M3u8 => super::transfer_export::m3u(&document, bindings),
    }
}

async fn prepare(
    transaction: &mut Transaction<'_, Sqlite>,
    source: &MusicTransferSource,
    bindings: &[TransferBinding],
) -> MusicLibraryResult<(MusicTransferPreview, MusicInterchangeDocument)> {
    codec::check_contents(&source.contents)?;
    if source.selected_at_ms <= 0 {
        return Err(validation("Music import selection time must be positive"));
    }
    if source.playlist_name.len() > 500 {
        return Err(validation("Imported playlist name is too long"));
    }
    let mut budget = TransferReadBudget::default();
    let roots = budget
        .read::<(String, String)>(
            transaction,
            "SELECT id, name FROM music_local_roots ORDER BY id",
            &["id", "name"],
            &[],
            codec::MAX_PLAYLISTS,
        )
        .await?;
    let available_roots: Vec<_> = roots
        .into_iter()
        .map(|(id, name)| MusicInterchangeRoot { id, name })
        .collect();
    let json = source
        .contents
        .trim_start_matches(|character: char| character.is_whitespace() || character == '\u{feff}')
        .starts_with(['{', '[']);
    let (document, counts) = if json {
        (codec::parse_json(&source.contents)?, [0; 4])
    } else {
        prepare_m3u(transaction, source, bindings, &available_roots, &mut budget).await?
    };
    codec::validate_document(&document)?;
    let playlist_ids: Vec<_> = document
        .playlists
        .iter()
        .map(|playlist| &playlist.id)
        .collect();
    let existing_playlists = budget.read::<(String, String, i64)>(transaction,
        "SELECT id, name, version FROM music_playlists WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id", &["id", "name"], &[&serialized(&playlist_ids)?], codec::MAX_PLAYLISTS).await?;
    let mut identities = HashSet::new();
    let mut duplicate_items = 0;
    for membership in document
        .playlists
        .iter()
        .flat_map(|playlist| &playlist.memberships)
    {
        if !identities.insert(membership.item.identity_key.clone()) {
            duplicate_items += 1;
        }
    }
    let identities: Vec<_> = identities.into_iter().collect();
    let existing_items = budget.read::<(String, String, i64, String, Option<String>)>(transaction,
        "SELECT id, identity_key, version, source_kind, youtube_video_id FROM music_library_items WHERE identity_key IN (SELECT value FROM json_each(?)) ORDER BY id", &["id", "identity_key", "source_kind", "youtube_video_id"], &[&serialized(&identities)?], codec::MAX_MEMBERSHIPS).await?;
    let matched_sources: HashMap<_, _> = existing_items
        .iter()
        .map(|(_, identity, _, kind, video)| (identity, (kind, video)))
        .collect();
    for membership in document
        .playlists
        .iter()
        .flat_map(|playlist| &playlist.memberships)
    {
        let item = &membership.item;
        if let Some((kind, video)) = matched_sources.get(&item.identity_key)
            && (kind.as_str() != item.source_kind.as_ref() || *video != &item.youtube_video_id)
        {
            return Err(validation(
                "An imported identity conflicts with its existing source",
            ));
        }
    }
    let existing_memberships = budget.read::<(String, i64)>(transaction,
        "SELECT id, version FROM music_playlist_memberships WHERE playlist_id IN (SELECT value FROM json_each(?)) ORDER BY id", &["id"], &[&serialized(&playlist_ids)?], codec::MAX_MEMBERSHIPS).await?;
    // Child rows such as snoozes and assignments do not all bump playlist revisions.
    let existing_ids: Vec<_> = existing_playlists
        .iter()
        .map(|(id, _, _)| id.clone())
        .collect();
    let prior_playlists = if existing_ids.is_empty() {
        None
    } else {
        Some(
            super::transfer_export::snapshot_with_budget(
                transaction,
                &existing_ids,
                source.selected_at_ms,
                bindings,
                &mut budget,
            )
            .await?,
        )
    };
    let assignment_revisions = budget.read::<(String, String, String, i64)>(transaction,
        "SELECT owner_kind, owner_id, phase, version FROM music_context_assignments AS current WHERE EXISTS (SELECT 1 FROM json_each(?) AS incoming WHERE current.owner_kind = json_extract(incoming.value, '$.ownerKind') AND current.owner_id = json_extract(incoming.value, '$.ownerId') AND current.phase = json_extract(incoming.value, '$.phase')) ORDER BY owner_kind, owner_id, phase", &["owner_kind", "owner_id", "phase"], &[&serialized(&document.context_assignments)?], codec::MAX_MEMBERSHIPS).await?;
    let revision = codec::hash(&codec::document_bytes(&(
        &document,
        &existing_playlists,
        &existing_items,
        &existing_memberships,
        prior_playlists,
        assignment_revisions,
        bindings,
    ))?);
    let existing_ids: HashSet<_> = existing_playlists.iter().map(|(id, _, _)| id).collect();
    let conflicts: Vec<_> = document
        .playlists
        .iter()
        .filter(|playlist| existing_ids.contains(&playlist.id))
        .map(|playlist| playlist.name.clone())
        .collect();
    let bound_root_ids: Vec<_> = bindings
        .iter()
        .filter(|binding| binding.available)
        .map(|binding| binding.root_id.clone())
        .collect();
    let unsupported: Vec<_> = document
        .warnings
        .iter()
        .filter(|warning| warning.starts_with("Unsupported record skipped:"))
        .collect();
    let unsupported_count = if json { unsupported.len() } else { counts[2] };
    let preview = MusicTransferPreview {
        format: if json {
            MusicTransferFormat::Json
        } else {
            MusicTransferFormat::M3u8
        },
        revision,
        roots: document.roots.clone(),
        available_roots,
        bound_root_ids: bound_root_ids.clone(),
        context_assignment_count: document.context_assignments.len(),
        new_playlists: document.playlists.len() - conflicts.len(),
        matched_playlists: conflicts.len(),
        new_items: identities.len() - existing_items.len(),
        matched_items: existing_items.len(),
        duplicate_items,
        missing_local_bindings: document
            .roots
            .iter()
            .filter(|root| !bound_root_ids.contains(&root.id))
            .count(),
        conflicts,
        unsupported: unsupported
            .into_iter()
            .take(codec::MAX_DIAGNOSTICS)
            .map(|warning| warning.chars().take(codec::MAX_DIAGNOSTIC_CHARS).collect())
            .collect(),
        local_count: counts[0],
        youtube_count: counts[1],
        unsupported_count,
        unresolved_local_count: counts[3],
    };
    Ok((preview, document))
}

#[derive(Clone, Debug, Serialize)]
struct ResolvedLocation {
    root_id: String,
    relative_path: String,
    pattern: String,
    windows: bool,
}

fn path_pattern(path: &str, windows: bool) -> String {
    let mut pattern = String::new();
    for character in path.chars() {
        if matches!(character, '*' | '?' | '[' | ']') {
            pattern.push('?');
        } else if windows
            && character.to_lowercase().to_string() != character.to_uppercase().to_string()
        {
            pattern.push('[');
            pattern.extend(character.to_lowercase());
            pattern.extend(character.to_uppercase());
            pattern.push(']');
        } else {
            pattern.push(character);
        }
    }
    pattern
}

fn resolve_location(
    value: &str,
    selected_root: Option<&str>,
    bindings: &[TransferBinding],
    roots: &[MusicInterchangeRoot],
) -> Option<ResolvedLocation> {
    let normalized = value.replace('\\', "/");
    let mut matches = Vec::new();
    for binding in bindings {
        let root = binding
            .folder_path
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();
        let (candidate, prefix) = if binding.windows {
            (normalized.to_lowercase(), root.to_lowercase())
        } else {
            (normalized.clone(), root.clone())
        };
        if candidate.starts_with(&(prefix + "/")) {
            // Match a normalized root by path components, so Unicode case expansion cannot alter a byte offset.
            let count = root.split('/').count();
            let relative_path = normalized
                .split('/')
                .skip(count)
                .collect::<Vec<_>>()
                .join("/");
            if super::interchange::safe_relative_path(&relative_path)
                && roots.iter().any(|entry| entry.id == binding.root_id)
            {
                matches.push((
                    root.len(),
                    ResolvedLocation {
                        root_id: binding.root_id.clone(),
                        pattern: path_pattern(&relative_path, binding.windows),
                        relative_path,
                        windows: binding.windows,
                    },
                ));
            }
        }
    }
    if let Some((_, location)) = matches.into_iter().max_by_key(|(length, _)| *length) {
        return Some(location);
    }
    let relative_path = normalized.strip_prefix("./").unwrap_or(&normalized);
    if !super::interchange::safe_relative_path(relative_path) {
        return None;
    }
    let root_id = selected_root.filter(|id| roots.iter().any(|root| root.id == *id))?;
    let windows = bindings
        .iter()
        .any(|binding| binding.root_id == root_id && binding.windows);
    Some(ResolvedLocation {
        root_id: root_id.to_string(),
        relative_path: relative_path.to_string(),
        pattern: path_pattern(relative_path, windows),
        windows,
    })
}

async fn prepare_m3u(
    transaction: &mut Transaction<'_, Sqlite>,
    source: &MusicTransferSource,
    bindings: &[TransferBinding],
    roots: &[MusicInterchangeRoot],
    budget: &mut TransferReadBudget,
) -> MusicLibraryResult<(MusicInterchangeDocument, [usize; 4])> {
    let entries = codec::parse_m3u(&source.contents)?;
    let resolved: Vec<_> = entries
        .iter()
        .map(|entry| {
            (!entry.unsupported && entry.video_id.is_none())
                .then(|| {
                    resolve_location(
                        &entry.value,
                        source.relative_root_id.as_deref(),
                        bindings,
                        roots,
                    )
                })
                .flatten()
        })
        .collect();
    let video_ids: Vec<_> = entries
        .iter()
        .filter_map(|entry| entry.video_id.as_ref())
        .collect();
    let locations: Vec<_> = resolved.iter().flatten().collect();
    let candidates = budget.read::<(String, String, String)>(transaction,
        "SELECT DISTINCT location.item_id, location.root_id, replace(location.relative_path, char(92), '/') AS relative_path FROM music_local_locations AS location JOIN json_each(?) AS requested ON location.root_id = json_extract(requested.value, '$.root_id') AND replace(location.relative_path, char(92), '/') GLOB json_extract(requested.value, '$.pattern') ORDER BY location.item_id, location.root_id, location.relative_path", &["item_id", "root_id", "relative_path"], &[&serialized(&locations)?], codec::MAX_MEMBERSHIPS).await?;
    let candidate_ids: Vec<_> = candidates.iter().map(|(id, _, _)| id).collect();
    let item_rows = budget.read::<MusicLibraryItemRow>(transaction,
        "SELECT * FROM music_library_items WHERE id IN (SELECT value FROM json_each(?)) OR youtube_video_id IN (SELECT value FROM json_each(?)) ORDER BY id", ITEM_TEXT_COLUMNS, &[&serialized(&candidate_ids)?, &serialized(&video_ids)?], codec::MAX_MEMBERSHIPS).await?;
    let mut by_id = HashMap::new();
    let mut youtube = HashMap::new();
    for row in item_rows {
        let item = MusicLibraryItem::try_from(row)?;
        if let Some(video) = &item.youtube_video_id {
            youtube.insert(video.clone(), item.id.clone());
        }
        by_id.insert(item.id.clone(), item);
    }
    let signals = budget.read::<(String, String)>(transaction,
        "SELECT item_id, signal FROM music_item_signals WHERE item_id IN (SELECT value FROM json_each(?)) ORDER BY item_id, signal", &["item_id", "signal"], &[&serialized(&candidate_ids)?], codec::MAX_CHILD_ROWS).await?;
    let mut signals_by_item: HashMap<String, Vec<MusicItemSignal>> = HashMap::new();
    for (item, signal) in signals {
        signals_by_item
            .entry(item)
            .or_default()
            .push(MusicItemSignal::try_from(signal.as_str()).map_err(validation)?);
    }
    let mut location_items: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for (item_id, root_id, path) in candidates {
        let windows = bindings
            .iter()
            .any(|binding| binding.root_id == root_id && binding.windows);
        let key = (root_id, if windows { path.to_lowercase() } else { path });
        location_items.entry(key).or_default().insert(item_id);
    }
    let mut memberships = Vec::new();
    let mut used_roots = BTreeMap::new();
    let mut counts = [0; 4];
    let mut warnings = vec![codec::M3U_WARNING.to_string()];
    for (index, (entry, location)) in entries.iter().zip(&resolved).enumerate() {
        if entry.unsupported {
            counts[2] += 1;
            continue;
        }
        let item = if let Some(video_id) = &entry.video_id {
            counts[1] += 1;
            let prior = youtube.get(video_id).and_then(|id| by_id.get(id));
            MusicInterchangeItem {
                identity_key: prior.map_or_else(
                    || format!("youtube:{video_id}"),
                    |item| item.identity_key.clone(),
                ),
                source_kind: MusicLibrarySourceKind::YouTubeVideo,
                youtube_video_id: Some(video_id.clone()),
                title: entry
                    .title
                    .clone()
                    .or_else(|| prior.map(|item| item.original_title.clone()))
                    .unwrap_or_else(|| video_id.clone()),
                artist: prior
                    .map(|item| item.original_artist.clone())
                    .unwrap_or_default(),
                album: prior
                    .map(|item| item.original_album.clone())
                    .unwrap_or_default(),
                duration_ms: prior.and_then(|item| item.duration_ms),
                signals: Vec::new(),
                locations: Vec::new(),
            }
        } else {
            counts[0] += 1;
            let Some(location) = location else {
                counts[3] += 1;
                continue;
            };
            let key = (
                location.root_id.clone(),
                if location.windows {
                    location.relative_path.to_lowercase()
                } else {
                    location.relative_path.clone()
                },
            );
            let matching = location_items.get(&key);
            if matching.is_some_and(|items| items.len() > 1) {
                counts[3] += 1;
                warnings.push(format!(
                    "Unsupported record skipped: M3U8 entry {} has ambiguous local matches",
                    index + 1
                ));
                continue;
            }
            let prior = matching
                .and_then(|items| items.iter().next())
                .and_then(|id| by_id.get(id));
            if let Some(root) = roots.iter().find(|root| root.id == location.root_id) {
                used_roots.insert(root.id.clone(), root.clone());
            }
            MusicInterchangeItem {
                identity_key: prior.map_or_else(
                    || codec::local_identity(&location.root_id, &location.relative_path),
                    |item| item.identity_key.clone(),
                ),
                source_kind: MusicLibrarySourceKind::LocalFile,
                youtube_video_id: None,
                title: entry
                    .title
                    .clone()
                    .or_else(|| prior.map(|item| item.original_title.clone()))
                    .unwrap_or_else(|| {
                        location
                            .relative_path
                            .rsplit('/')
                            .next()
                            .unwrap_or(&location.relative_path)
                            .to_string()
                    }),
                artist: prior
                    .map(|item| item.original_artist.clone())
                    .unwrap_or_default(),
                album: prior
                    .map(|item| item.original_album.clone())
                    .unwrap_or_default(),
                duration_ms: prior.and_then(|item| item.duration_ms),
                signals: prior
                    .and_then(|item| signals_by_item.get(&item.id))
                    .cloned()
                    .unwrap_or_default(),
                locations: vec![MusicInterchangeLocation {
                    root_id: location.root_id.clone(),
                    relative_path: location.relative_path.clone(),
                    availability: MusicLocationAvailability::Missing,
                }],
            }
        };
        memberships.push(MusicInterchangeMembership {
            item,
            position: index as i64,
            weight: MusicWeight::Normal,
            enabled: true,
            start_ms: None,
            end_ms: None,
            volume: None,
            rate: None,
            skip_ranges: Vec::new(),
            snoozes: Vec::new(),
        });
    }
    let name = source.playlist_name.trim();
    let document = MusicInterchangeDocument {
        format: codec::FORMAT.to_string(),
        version: 1,
        exported_at: source.selected_at_ms,
        roots: used_roots.into_values().collect(),
        playlists: vec![MusicInterchangePlaylist {
            id: format!("m3u:{}", codec::hash(serialized(source)?.as_bytes())),
            name: if name.is_empty() {
                "Imported playlist".to_string()
            } else {
                name.to_string()
            },
            icon: "lucide:list-music".to_string(),
            shuffle_enabled: false,
            mix_enabled: false,
            repeat_mode: MusicRepeatMode::All,
            intended_uses: Vec::new(),
            memberships,
        }],
        context_assignments: Vec::new(),
        warnings,
    };
    Ok((document, counts))
}
