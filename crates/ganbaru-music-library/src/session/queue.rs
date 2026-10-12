//! Bounded canonical queue snapshots and device-local source resolution.

use super::{models::*, policy::SessionPolicy};
use crate::{
    MusicItemAvailability, MusicLibraryError, MusicLibraryResult, MusicRepeatMode, MusicWeight,
};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};
use std::collections::{BTreeMap, HashMap, HashSet};

const ITEM_BATCH_SIZE: usize = 400;
const MAX_SKIP_RANGES: usize = 100_000;
const MAX_SOURCE_TEXT: usize = 16_384;

#[derive(sqlx::FromRow)]
struct QueueRow {
    item_id: String,
    identity_key: String,
    source_kind: String,
    media_kind: String,
    youtube_video_id: Option<String>,
    youtube_resolution_state: Option<String>,
    title: String,
    availability: String,
    root_id: Option<String>,
    relative_path: Option<String>,
    membership_id: Option<String>,
    weight: Option<String>,
    enabled: Option<i64>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    volume: Option<f64>,
    rate: Option<f64>,
    snoozed_until: Option<i64>,
    snoozed_indefinitely: i64,
}

fn validation(message: impl Into<String>) -> MusicLibraryError {
    MusicLibraryError::validation("queue", message)
}

fn bounded_text(value: &str, field: &str) -> MusicLibraryResult<()> {
    if value.is_empty() || value.len() > MAX_SOURCE_TEXT || value.contains('\0') {
        return Err(MusicLibraryError::validation(
            field,
            "must be nonempty bounded text without null characters",
        ));
    }
    Ok(())
}

pub fn validate_action_id(value: &str) -> MusicLibraryResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:".contains(&byte))
    {
        return Err(MusicLibraryError::validation(
            "actionId",
            "must be a bounded action identifier",
        ));
    }
    Ok(())
}

fn source_entry(source: SessionSource) -> MusicLibraryResult<SessionQueueEntry> {
    bounded_text(&source.identity, "source.identity")?;
    bounded_text(&source.title, "source.title")?;
    bounded_text(&source.original_input, "source.originalInput")?;
    if source
        .end_ms
        .is_some_and(|end| end <= source.start_ms.unwrap_or(0))
    {
        return Err(validation("source end must follow its start"));
    }
    let backend = match source.kind {
        SourceKind::LocalFile => {
            let path = source
                .path
                .as_deref()
                .ok_or_else(|| validation("local source requires a path"))?;
            bounded_text(path, "source.path")?;
            local_backend(path, None)
        }
        SourceKind::YoutubeVideo | SourceKind::YoutubePlaylist => {
            let identifier = if source.kind == SourceKind::YoutubeVideo {
                source.video_id.as_deref()
            } else {
                source.playlist_id.as_deref()
            };
            let identifier =
                identifier.ok_or_else(|| validation("YouTube source requires an identifier"))?;
            if identifier.len() > 256
                || !identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            {
                return Err(validation("invalid YouTube source identifier"));
            }
            SessionBackend::Browser
        }
    };
    Ok(SessionQueueEntry {
        item_id: None,
        membership_id: None,
        source,
        backend,
        availability: MusicItemAvailability::Available,
        enabled: true,
        weight: MusicWeight::Normal,
        snoozed_until: None,
        snoozed_indefinitely: false,
        embedding_blocked: false,
        bound: true,
        phase_allowed: true,
        skip_ranges: Vec::new(),
        volume: None,
        rate: None,
    })
}

fn local_backend(path: &str, media_kind: Option<&str>) -> SessionBackend {
    #[cfg(target_os = "android")]
    {
        let _ = (path, media_kind);
        SessionBackend::NativeAudio
    }
    #[cfg(not(target_os = "android"))]
    {
        let extension = path
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if media_kind == Some("video")
            || matches!(
                extension.as_str(),
                "avi"
                    | "flv"
                    | "m4v"
                    | "mkv"
                    | "mov"
                    | "mp4"
                    | "mpeg"
                    | "mpg"
                    | "ogv"
                    | "webm"
                    | "wmv"
            )
        {
            SessionBackend::Browser
        } else {
            SessionBackend::NativeAudio
        }
    }
}

fn resolve_path(root: Option<&String>, relative: Option<&str>) -> Option<String> {
    let root = root?;
    let relative = relative?;
    if relative.is_empty()
        || relative.starts_with(['/', '\\'])
        || relative.contains(['\0', ':'])
        || relative
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    #[cfg(target_os = "android")]
    {
        fn encode(value: &str) -> String {
            use std::fmt::Write;
            let mut encoded = String::new();
            for byte in value.bytes() {
                if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
                    encoded.push(char::from(byte));
                } else {
                    let _ = write!(&mut encoded, "%{byte:02X}");
                }
            }
            encoded
        }
        if !root.starts_with("content://") {
            return None;
        }
        Some(format!("ganbaru-saf:{}#{}", encode(root), encode(relative)))
    }
    #[cfg(not(target_os = "android"))]
    {
        Some(
            std::path::Path::new(root)
                .join(relative.replace('\\', "/"))
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn row_entry(
    row: QueueRow,
    roots: &BTreeMap<String, String>,
) -> MusicLibraryResult<SessionQueueEntry> {
    let path = resolve_path(
        row.root_id.as_ref().and_then(|root| roots.get(root)),
        row.relative_path.as_deref(),
    );
    let local = row.source_kind == "local-file";
    let input = if local {
        path.clone().unwrap_or_default()
    } else {
        format!(
            "https://www.youtube.com/watch?v={}",
            row.youtube_video_id.as_deref().unwrap_or_default()
        )
    };
    let backend = if local {
        local_backend(path.as_deref().unwrap_or_default(), Some(&row.media_kind))
    } else {
        SessionBackend::Browser
    };
    let availability =
        MusicItemAvailability::try_from(row.availability.as_str()).map_err(validation)?;
    let weight =
        MusicWeight::try_from(row.weight.as_deref().unwrap_or("normal")).map_err(validation)?;
    let bound = !local || path.is_some();
    Ok(SessionQueueEntry {
        item_id: Some(row.item_id),
        membership_id: row.membership_id,
        source: SessionSource {
            kind: if local {
                SourceKind::LocalFile
            } else {
                SourceKind::YoutubeVideo
            },
            identity: row.identity_key,
            original_input: input,
            title: row.title,
            path,
            artwork_path: None,
            video_id: row.youtube_video_id,
            playlist_id: None,
            start_ms: row.start_ms.and_then(|value| u64::try_from(value).ok()),
            end_ms: row.end_ms.and_then(|value| u64::try_from(value).ok()),
        },
        backend,
        availability,
        enabled: row.enabled.unwrap_or(1) != 0,
        weight,
        snoozed_until: row.snoozed_until,
        snoozed_indefinitely: row.snoozed_indefinitely != 0,
        embedding_blocked: row.youtube_resolution_state.as_deref() == Some("embedding-blocked"),
        bound,
        phase_allowed: true,
        skip_ranges: Vec::new(),
        volume: row.volume,
        rate: row.rate,
    })
}

fn row_query<'a>(playlist_id: Option<&'a str>, now_ms: i64) -> QueryBuilder<'a, Sqlite> {
    let mut query = QueryBuilder::new(
        "SELECT item.id AS item_id, item.identity_key, item.source_kind, item.media_kind,
         item.youtube_video_id, item.youtube_resolution_state,
         COALESCE(NULLIF(item.title_override, ''), item.original_title) AS title, item.availability,
         (SELECT root_id FROM music_local_locations WHERE item_id = item.id AND availability = 'available'
          ORDER BY updated_at_ms DESC, id LIMIT 1) AS root_id,
         (SELECT relative_path FROM music_local_locations WHERE item_id = item.id AND availability = 'available'
          ORDER BY updated_at_ms DESC, id LIMIT 1) AS relative_path,
         membership.id AS membership_id, membership.weight, membership.enabled,
         membership.start_ms, membership.end_ms, membership.volume, membership.rate,
         (SELECT MAX(ends_at_ms) FROM music_snoozes WHERE item_id = item.id AND starts_at_ms <= ");
    query
        .push_bind(now_ms)
        .push(" AND ends_at_ms > ")
        .push_bind(now_ms);
    query
        .push(" AND (scope = 'all-playlists' OR playlist_id = ")
        .push_bind(playlist_id)
        .push(
            ")) AS snoozed_until,
         EXISTS(SELECT 1 FROM music_snoozes WHERE item_id = item.id AND starts_at_ms <= ",
        );
    query
        .push_bind(now_ms)
        .push(" AND ends_at_ms IS NULL AND (scope = 'all-playlists' OR playlist_id = ")
        .push_bind(playlist_id);
    query.push(")) AS snoozed_indefinitely FROM music_library_items AS item
         LEFT JOIN music_playlist_memberships AS membership ON membership.item_id = item.id AND membership.playlist_id = ").push_bind(playlist_id);
    query
}

/// Reads each queue from one caller-owned SQLite snapshot, with bounded children.
pub async fn load_queue(
    connection: &mut SqliteConnection,
    roots: &BTreeMap<String, String>,
    definition: &SessionQueueIntent,
    state: &mut SessionPolicy,
    now_ms: i64,
) -> MusicLibraryResult<Option<String>> {
    state.playlist_id = None;
    let (mut entries, selected) = match definition {
        SessionQueueIntent::Sources {
            sources,
            selected_index,
            name,
        } => {
            if sources.is_empty() || sources.len() > MAX_QUEUE_ENTRIES {
                return Err(validation("source queue size is outside its limit"));
            }
            bounded_text(name, "queue.name")?;
            if selected_index.is_some_and(|index| index >= sources.len()) {
                return Err(validation("selected source is outside the queue"));
            }
            let entries = sources
                .iter()
                .cloned()
                .map(source_entry)
                .collect::<MusicLibraryResult<Vec<_>>>()?;
            state.queue_name = name.clone();
            (
                entries,
                selected_index.map(|index| sources[index].identity.clone()),
            )
        }
        SessionQueueIntent::SavedPlaylist {
            playlist_id,
            explicit_item_id,
            ..
        } => {
            bounded_text(playlist_id, "playlistId")?;
            let (name, shuffle, mix, repeat): (String, i64, i64, String) = sqlx::query_as(
                "SELECT name, shuffle_enabled, mix_enabled, repeat_mode FROM music_playlists WHERE id = ?")
                .bind(playlist_id).fetch_optional(&mut *connection).await
                .map_err(|error| MusicLibraryError::database("read session playlist", error))?
                .ok_or_else(|| MusicLibraryError::not_found("playlist", playlist_id))?;
            state.queue_name = name;
            state.playlist_id = Some(playlist_id.clone());
            state.order = if mix != 0 {
                PlaybackOrder::Mix
            } else if shuffle != 0 {
                PlaybackOrder::Shuffle
            } else {
                PlaybackOrder::InOrder
            };
            state.repeat_mode = MusicRepeatMode::try_from(repeat.as_str()).map_err(validation)?;
            let mut query = row_query(Some(playlist_id), now_ms);
            query
                .push(" WHERE membership.playlist_id = ")
                .push_bind(playlist_id)
                .push(" ORDER BY membership.position, membership.id LIMIT ")
                .push_bind((MAX_QUEUE_ENTRIES + 1) as i64);
            let rows = query
                .build_query_as::<QueueRow>()
                .fetch_all(&mut *connection)
                .await
                .map_err(|error| MusicLibraryError::database("read session memberships", error))?;
            if rows.len() > MAX_QUEUE_ENTRIES {
                return Err(validation("playlist exceeds the native queue limit"));
            }
            let entries = rows
                .into_iter()
                .map(|row| row_entry(row, roots))
                .collect::<MusicLibraryResult<Vec<_>>>()?;
            (entries, explicit_item_id.clone())
        }
        SessionQueueIntent::LibraryItems {
            item_ids,
            selected_item_id,
            name,
        } => {
            bounded_text(name, "queue.name")?;
            state.queue_name = name.clone();
            (
                load_items(connection, roots, item_ids, now_ms).await?,
                selected_item_id.clone(),
            )
        }
        SessionQueueIntent::ReviewItem { item_id } => {
            state.owner = SessionOwner::Review;
            (
                load_items(connection, roots, std::slice::from_ref(item_id), now_ms).await?,
                Some(item_id.clone()),
            )
        }
    };
    if entries.is_empty() {
        return Err(validation("empty queues do not replace active playback"));
    }
    if let Some(playlist_id) = &state.playlist_id {
        let ranges: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT skip.membership_id, skip.start_ms, skip.end_ms FROM music_membership_skip_ranges AS skip
             JOIN music_playlist_memberships AS membership ON membership.id = skip.membership_id
             WHERE membership.playlist_id = ? ORDER BY skip.membership_id, skip.sort_order LIMIT ?")
            .bind(playlist_id).bind((MAX_SKIP_RANGES + 1) as i64).fetch_all(&mut *connection).await
            .map_err(|error| MusicLibraryError::database("read session skip ranges", error))?;
        if ranges.len() > MAX_SKIP_RANGES {
            return Err(validation(
                "playlist skip ranges exceed the native queue limit",
            ));
        }
        let mut by_membership: HashMap<String, Vec<SessionSkipRange>> = HashMap::new();
        for (membership, start, end) in ranges {
            if let (Ok(start_ms), Ok(end_ms)) = (u64::try_from(start), u64::try_from(end)) {
                by_membership
                    .entry(membership)
                    .or_default()
                    .push(SessionSkipRange { start_ms, end_ms });
            }
        }
        for entry in &mut entries {
            entry.skip_ranges = entry
                .membership_id
                .as_ref()
                .and_then(|id| by_membership.remove(id))
                .unwrap_or_default();
        }
    }
    state.queue = std::sync::Arc::new(entries);
    state.definition = Some(definition.clone());
    state.queue_revision += 1;
    Ok(selected)
}

async fn load_items(
    connection: &mut SqliteConnection,
    roots: &BTreeMap<String, String>,
    ids: &[String],
    now_ms: i64,
) -> MusicLibraryResult<Vec<SessionQueueEntry>> {
    if ids.is_empty() || ids.len() > MAX_QUEUE_ENTRIES {
        return Err(validation("library queue size is outside its limit"));
    }
    let mut unique = HashSet::new();
    for id in ids {
        bounded_text(id, "itemId")?;
        if !unique.insert(id) {
            return Err(validation("queue item IDs must be unique"));
        }
    }
    let mut by_id = HashMap::new();
    for batch in ids.chunks(ITEM_BATCH_SIZE) {
        let mut query = row_query(None, now_ms);
        query.push(" WHERE item.id IN (");
        let mut separated = query.separated(", ");
        for id in batch {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        let rows = query
            .build_query_as::<QueueRow>()
            .fetch_all(&mut *connection)
            .await
            .map_err(|error| MusicLibraryError::database("read session items", error))?;
        for row in rows {
            by_id.insert(row.item_id.clone(), row_entry(row, roots)?);
        }
    }
    ids.iter()
        .map(|id| {
            by_id
                .remove(id)
                .ok_or_else(|| MusicLibraryError::not_found("music item", id))
        })
        .collect()
}
