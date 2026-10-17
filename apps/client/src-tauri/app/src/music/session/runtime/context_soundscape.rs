//! Independent background-layer effects prepared in the accepted soundtrack transaction.

use super::context::ContextActivation;
use super::*;
use crate::music::soundscape::{GeneratedNoiseKind, SoundscapeStartRequest};
use std::sync::{Arc, LazyLock, atomic::AtomicBool};
use std::time::Instant;

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_SOUND_SOURCE_ID_BYTES: i64 = 200;
const MAX_SOUND_PATH_BYTES: i64 = 16_384;
static DELIVERY_GATE: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));

#[derive(sqlx::FromRow)]
struct AcceptedSoundscapeSource {
    generated_kind: Option<String>,
    absolute_path: Option<String>,
    volume: f64,
    generated_level: Option<f64>,
    local_level: Option<f64>,
}

pub(super) struct PreparedSoundscape {
    action: Option<Option<SoundscapeStartRequest>>,
    version: Option<i64>,
    pub missing: bool,
}

/// Read only the selected source, with a SQL size predicate before path allocation.
pub(super) async fn prepare(
    connection: &mut sqlx::SqliteConnection,
    device_id: &str,
    behavior: &str,
    soundscape_id: Option<&str>,
) -> MusicLibraryResult<PreparedSoundscape> {
    use ganbaru_music::assignments::MusicSoundscapeBehavior;
    let behavior = MusicSoundscapeBehavior::try_from(behavior)
        .map_err(|error| MusicLibraryError::validation("assignment.soundscapeBehavior", error))?;
    let empty = |missing| PreparedSoundscape {
        action: None,
        version: None,
        missing,
    };
    if matches!(
        behavior,
        MusicSoundscapeBehavior::Inherit | MusicSoundscapeBehavior::KeepCurrentSoundscape
    ) {
        return Ok(empty(false));
    }
    let request = if behavior == MusicSoundscapeBehavior::PlaySelected {
        let Some(id) = soundscape_id else {
            return Ok(empty(true));
        };
        let source: Option<AcceptedSoundscapeSource> = sqlx::query_as(
            "SELECT s.generated_kind, l.absolute_path, st.volume, st.generated_level, st.local_level
             FROM music_soundscapes s
             JOIN music_soundscape_state st ON st.singleton = 1
             LEFT JOIN music_soundscape_locations l ON l.soundscape_id = s.id AND l.device_id = ?
             WHERE s.id = ? AND length(CAST(s.id AS BLOB)) <= ?
               AND (l.absolute_path IS NULL OR length(CAST(l.absolute_path AS BLOB)) <= ?)
               AND CASE WHEN s.source_kind = 'local-loop' THEN l.availability ELSE s.availability END = 'available'",
        ).bind(device_id).bind(id).bind(MAX_SOUND_SOURCE_ID_BYTES).bind(MAX_SOUND_PATH_BYTES).fetch_optional(&mut *connection).await
            .map_err(|error| MusicLibraryError::database("read accepted background sound", error))?;
        let Some(source) = source else {
            return Ok(empty(true));
        };
        let generated_kind = match source.generated_kind.as_deref() {
            Some("white") => Some(GeneratedNoiseKind::White),
            Some("pink") => Some(GeneratedNoiseKind::Pink),
            Some("brown") => Some(GeneratedNoiseKind::Brown),
            None => None,
            Some(_) => {
                return Err(MusicLibraryError::validation(
                    "soundscape.generatedKind",
                    "unknown generated noise",
                ));
            }
        };
        Some(SoundscapeStartRequest {
            source_id: id.into(),
            generated_kind,
            local_path: source.absolute_path,
            level: if generated_kind.is_some() {
                source.generated_level
            } else {
                source.local_level
            }
            .unwrap_or(1.0),
            extra_sources: Vec::new(),
            volume: source.volume,
        })
    } else {
        None
    };
    if let Some(request) = &request {
        sqlx::query("DELETE FROM music_soundscape_active_selections")
            .execute(&mut *connection)
            .await
            .map_err(|error| {
                MusicLibraryError::database("replace automatic background selection", error)
            })?;
        sqlx::query(
            "INSERT INTO music_soundscape_active_selections(position, soundscape_id) VALUES (0, ?)",
        )
        .bind(&request.source_id)
        .execute(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("select accepted background sound", error))?;
    }
    let version: i64 = sqlx::query_scalar(
        "UPDATE music_soundscape_state
         SET active_soundscape_id = CASE WHEN ? THEN ? ELSE active_soundscape_id END,
             desired_playing = ?, automatic_intent = 1, version = version + 1, updated_at_ms = MAX(updated_at_ms + 1, ?)
         WHERE singleton = 1 RETURNING version",
    )
    .bind(request.is_some())
    .bind(request.as_ref().map(|request| &request.source_id))
    .bind(request.is_some())
    .bind(now_ms())
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("persist accepted background sound", error))?;
    Ok(PreparedSoundscape {
        action: Some(request),
        version: Some(version),
        missing: false,
    })
}

impl Owner {
    /// Stop background output before the vault coordinator can release its database.
    pub(super) async fn stop_context_soundscape(&self) -> MusicLibraryResult<()> {
        let delivery_permit = DELIVERY_GATE.clone().try_acquire_owned().map_err(|_| {
            MusicLibraryError::conflict("Background output is still draining before vault handoff")
        })?;
        let app = self.app.clone();
        let worker = tauri::async_runtime::spawn_blocking(move || {
            let _delivery_permit = delivery_permit;
            crate::music::soundscape::music_soundscape_stop(
                app.state::<crate::music::soundscape::SoundscapeEngineState>(),
            )
            .map(|_| ())
            .map_err(|error| {
                MusicLibraryError::runtime("stop vault background output", format!("{error:?}"))
            })
        });
        tokio::time::timeout(DELIVERY_TIMEOUT, worker)
            .await
            .map_err(|_| {
                MusicLibraryError::runtime(
                    "background output quiescence",
                    "acknowledgement timed out; the retained worker must drain",
                )
            })?
            .map_err(|error| {
                MusicLibraryError::runtime("receive background output quiescence", error)
            })?
    }

    pub(super) async fn deliver_context_soundscape(
        &mut self,
        prepared: PreparedSoundscape,
        activation: ContextActivation,
        permit: crate::vault::ownership::ManagedVaultWritePermit,
    ) -> MusicLibraryResult<()> {
        let Some(request) = prepared.action else {
            return Ok(());
        };
        let delivery_permit = DELIVERY_GATE.clone().try_acquire_owned().map_err(|_| {
            MusicLibraryError::conflict("An earlier background sound delivery is still draining")
        })?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let accepted_version = prepared.version;
        let app = self.app.clone();
        let pool = self.pool.as_ref().expect("initialized music pool").clone();
        let lifecycle = self.lifecycle.clone();
        let revision = lifecycle.current().revision;
        let deadline = Instant::now() + DELIVERY_TIMEOUT;
        let focus_proof = match &activation.authority {
            super::context::AssignmentAuthority::Focus(_) => Some(
                crate::pomodoro::native_runtime::capture_music_delivery(&app).ok_or_else(|| {
                    MusicLibraryError::conflict(
                        "Committed background Focus authority is unavailable",
                    )
                })?,
            ),
            super::context::AssignmentAuthority::Calendar { .. } => None,
        };
        let worker = tauri::async_runtime::spawn_blocking(move || {
            // Keep both permits through actual engine acknowledgement, even after a timeout.
            let _write_permit = permit;
            let _delivery_permit = delivery_permit;
            let engine = app.state::<crate::music::soundscape::SoundscapeEngineState>();
            let guard_app = app.clone();
            crate::music::soundscape::apply_automatic(
                engine,
                request,
                Box::new(move || {
                    if worker_cancelled.load(Ordering::Acquire)
                        || Instant::now() >= deadline
                        || !lifecycle.matches(revision, LifecycleIntent::Active)
                    {
                        return false;
                    }
                    match activation.current(&guard_app) {
                        Ok(true) => {}
                        Ok(false) => return false,
                        Err(error) => {
                            eprintln!("background sound authority: {error}");
                            return false;
                        }
                    }
                    if focus_proof.as_ref().is_some_and(|(proof, deadline)| {
                        !crate::pomodoro::native_runtime::music_delivery_is_current(
                            &guard_app, proof, *deadline,
                        )
                    }) {
                        return false;
                    }
                    // A later explicit background-layer choice supersedes the prepared effect.
                    match tauri::async_runtime::block_on(async {
                        sqlx::query_scalar::<_, i64>(
                            "SELECT version FROM music_soundscape_state WHERE singleton = 1",
                        )
                        .fetch_optional(&pool)
                        .await
                    }) {
                        Ok(version) => version == accepted_version,
                        Err(error) => {
                            eprintln!("background sound state authority: {error}");
                            false
                        }
                    }
                }),
            )
            .map_err(|error| {
                MusicLibraryError::runtime(
                    "execute accepted background sound",
                    format!("{error:?}"),
                )
            })
        });
        let result = match tokio::time::timeout(DELIVERY_TIMEOUT, worker).await {
            Ok(result) => result.map_err(|error| {
                MusicLibraryError::runtime("receive background sound delivery", error)
            })?,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(MusicLibraryError::runtime(
                    "background sound delivery",
                    "native acknowledgement timed out; the retained worker must drain",
                ))
            }
        };
        if result.is_err() {
            let _permit = self.write_permit().await?;
            sqlx::query("UPDATE music_soundscape_state SET desired_playing = 0, version = version + 1, updated_at_ms = MAX(updated_at_ms + 1, ?) WHERE singleton = 1 AND version = ?")
                .bind(now_ms()).bind(accepted_version).execute(self.pool.as_ref().expect("initialized music pool")).await
                .map_err(|error| MusicLibraryError::database("revoke failed automatic background sound", error))?;
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests;
