//! Device-scoped portable checkpoints and atomic command/listening receipts.

use super::{
    models::*,
    policy::{SessionPolicy, Transition},
};
use crate::{MusicLibraryError, MusicLibraryResult, record_listening_in_transaction};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};

const MAX_CHECKPOINT_BYTES: usize = 2 * 1024 * 1024;

/// Hashes semantic intent without retaining device paths in the receipt table.
pub fn request_hash(request: &impl serde::Serialize) -> MusicLibraryResult<String> {
    let bytes = serde_json::to_vec(request)
        .map_err(|error| MusicLibraryError::runtime("encode session request", error))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn entry_id(entry: &SessionQueueEntry) -> Option<&str> {
    entry.membership_id.as_deref().or(entry.item_id.as_deref())
}

/// Checkpoints contain database IDs only. Runtime source paths and raw input are excluded.
pub fn checkpoint(state: &SessionPolicy) -> SessionCheckpoint {
    let portable = state
        .definition
        .as_ref()
        .filter(|definition| !matches!(definition, SessionQueueIntent::Sources { .. }));
    let id_at = |index: &usize| {
        state
            .queue
            .get(*index)
            .and_then(entry_id)
            .map(str::to_string)
    };
    SessionCheckpoint {
        session_id: state.session_id.clone(),
        revision: state.revision,
        generation: state.generation,
        queue: portable.cloned(),
        selected_entry_id: portable
            .and_then(|_| state.current_entry().and_then(entry_id))
            .map(str::to_string),
        history: if portable.is_some() {
            state.history.iter().filter_map(id_at).collect()
        } else {
            Vec::new()
        },
        recent_entry_ids: if portable.is_some() {
            state
                .recent
                .iter()
                .filter_map(|identity| {
                    state
                        .queue
                        .iter()
                        .find(|entry| &entry.source.identity == identity)
                        .and_then(entry_id)
                        .map(str::to_string)
                })
                .collect()
        } else {
            Vec::new()
        },
        remaining_shuffle: if portable.is_some() {
            state.shuffle.iter().filter_map(id_at).collect()
        } else {
            Vec::new()
        },
        position_ms: state.position_ms,
        duration_ms: state.duration_ms,
        order: state.order,
        repeat_mode: state.repeat_mode,
        volume: state.volume,
        muted: state.muted,
        rate: state.rate,
        random_state: state.random_state,
    }
}

pub async fn load_checkpoint(
    pool: &SqlitePool,
    device_id: &str,
) -> MusicLibraryResult<Option<SessionCheckpoint>> {
    let encoded: Option<String> = sqlx::query_scalar(
        "SELECT checkpoint_json FROM music_session_checkpoints WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("read music checkpoint", error))?;
    encoded
        .map(|encoded| {
            if encoded.len() > MAX_CHECKPOINT_BYTES {
                return Err(MusicLibraryError::validation(
                    "checkpoint",
                    "stored session exceeds its size limit",
                ));
            }
            let checkpoint: SessionCheckpoint = serde_json::from_str(&encoded)
                .map_err(|error| MusicLibraryError::runtime("decode music checkpoint", error))?;
            if matches!(checkpoint.queue, Some(SessionQueueIntent::Sources { .. }))
                || checkpoint.history.len() > MAX_HISTORY
                || checkpoint.recent_entry_ids.len() > MAX_RECENT
                || checkpoint.remaining_shuffle.len() > MAX_QUEUE_ENTRIES
            {
                return Err(MusicLibraryError::validation(
                    "checkpoint",
                    "stored session contains invalid portable state",
                ));
            }
            Ok(checkpoint)
        })
        .transpose()
}

/// How a restored checkpoint chooses its selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreSelection {
    /// Reselects the saved item at its saved position, for a context this process reopens.
    Saved,
    /// Selects the item after the saved one from its start, so a new launch begins a fresh track.
    Next,
}

/// Rebuilds indices from current canonical rows and always restores paused.
pub fn restore(
    state: &mut SessionPolicy,
    saved: &SessionCheckpoint,
    selection: RestoreSelection,
    now_ms: i64,
) -> Transition {
    let index_of = |id: &str| {
        state
            .queue
            .iter()
            .position(|entry| entry_id(entry) == Some(id))
    };
    let current = saved.selected_entry_id.as_deref().and_then(index_of);
    let history = saved.history.iter().filter_map(|id| index_of(id)).collect();
    let shuffle = saved
        .remaining_shuffle
        .iter()
        .filter_map(|id| index_of(id))
        .collect();
    let recent = saved
        .recent_entry_ids
        .iter()
        .filter_map(|id| index_of(id).map(|index| state.queue[index].source.identity.clone()))
        .collect();
    state.session_id = saved.session_id.clone();
    state.revision = saved.revision;
    state.generation = state.generation.max(saved.generation).saturating_add(1);
    state.volume = saved.volume.clamp(0.0, 1.0);
    state.muted = saved.muted;
    state.rate = saved.rate.clamp(0.25, 2.0);
    state.order = saved.order;
    state.repeat_mode = saved.repeat_mode;
    state.random_state = saved.random_state.max(1);
    let transition = match selection {
        RestoreSelection::Next => {
            state.history = history;
            state.shuffle = shuffle;
            state.recent = recent;
            state.current = current;
            state.advance_restored(now_ms)
        }
        RestoreSelection::Saved => {
            let selected = current.and_then(|index| state.queue[index].item_id.clone());
            let mut transition = state.initial(selected.as_deref(), None, false, now_ms);
            state.history = history;
            state.shuffle = shuffle;
            state.recent = recent;
            if let Some(index) = current.filter(|index| state.current == Some(*index)) {
                let source = &state.queue[index].source;
                let minimum = source.start_ms.unwrap_or(0);
                let maximum = source.end_ms.or(saved.duration_ms);
                state.position_ms = if maximum.is_some_and(|maximum| saved.position_ms >= maximum) {
                    minimum
                } else {
                    saved.position_ms.max(minimum)
                };
                state.duration_ms = saved.duration_ms;
                for effect in &mut transition.effects {
                    if let SessionEffect::Load { position_ms, .. } = effect {
                        *position_ms = state.position_ms;
                    }
                }
            }
            transition
        }
    };
    state.status = if state.current.is_some() {
        SessionStatus::Paused
    } else {
        SessionStatus::Idle
    };
    state.issue = Some(SessionIssue::Restored);
    transition
}

pub async fn receipt(
    connection: &mut SqliteConnection,
    device_id: &str,
    action_id: &str,
    hash: &str,
) -> MusicLibraryResult<bool> {
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT request_hash FROM music_session_receipts WHERE device_id = ? AND action_id = ?",
    )
    .bind(device_id)
    .bind(action_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| MusicLibraryError::database("read music session receipt", error))?;
    match existing {
        Some(existing) if existing != hash => Err(MusicLibraryError::conflict(
            "Music action ID was already used for different intent",
        )),
        Some(_) => Ok(true),
        None => Ok(false),
    }
}

/// Commits resume state, selection counters, and optional idempotency receipt together.
pub async fn commit(
    pool: &SqlitePool,
    device_id: &str,
    state: &SessionPolicy,
    transition: &Transition,
    action: Option<(&str, &str)>,
    now_ms: i64,
) -> MusicLibraryResult<bool> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| MusicLibraryError::database("begin music session transition", error))?;
    let applied = commit_in_transaction(
        &mut transaction,
        device_id,
        state,
        transition,
        action,
        now_ms,
    )
    .await?;
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit music session transition", error))?;
    Ok(applied)
}

/// Saves a transition beside canonical queue reads under the caller's writer transaction.
pub async fn commit_in_transaction(
    connection: &mut SqliteConnection,
    device_id: &str,
    state: &SessionPolicy,
    transition: &Transition,
    action: Option<(&str, &str)>,
    now_ms: i64,
) -> MusicLibraryResult<bool> {
    if let Some((action_id, hash)) = action
        && receipt(connection, device_id, action_id, hash).await?
    {
        return Ok(false);
    }
    let encoded = serde_json::to_string(&checkpoint(state))
        .map_err(|error| MusicLibraryError::runtime("encode music checkpoint", error))?;
    if encoded.len() > MAX_CHECKPOINT_BYTES {
        return Err(MusicLibraryError::validation(
            "queue",
            "music checkpoint exceeds its size limit",
        ));
    }
    sqlx::query("INSERT INTO music_session_checkpoints (device_id, session_id, revision, checkpoint_json, updated_at_ms)
        VALUES (?, ?, ?, ?, ?) ON CONFLICT(device_id) DO UPDATE SET
        session_id = excluded.session_id, revision = excluded.revision,
        checkpoint_json = excluded.checkpoint_json, updated_at_ms = excluded.updated_at_ms")
        .bind(device_id).bind(&state.session_id).bind(state.revision as i64).bind(encoded).bind(now_ms)
        .execute(&mut *connection).await.map_err(|error| MusicLibraryError::database("save music checkpoint", error))?;
    for update in &transition.listening {
        record_listening_in_transaction(connection, update).await?;
    }
    if let Some((action_id, hash)) = action {
        // The receipt result contains no resolved source data and is never replayed as a live session.
        let result = serde_json::json!({"sessionId": state.session_id, "revision": state.revision})
            .to_string();
        sqlx::query("INSERT INTO music_session_receipts (device_id, action_id, request_hash, result_json, committed_at_ms) VALUES (?, ?, ?, ?, ?)")
            .bind(device_id).bind(action_id).bind(hash).bind(result).bind(now_ms).execute(&mut *connection).await
            .map_err(|error| MusicLibraryError::database("record music session receipt", error))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
