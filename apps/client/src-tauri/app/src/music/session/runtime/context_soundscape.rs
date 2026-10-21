//! Independent background-layer effects prepared in the accepted soundtrack transaction.

use super::context::ContextActivation;
use super::*;
use crate::music::soundscape::{GeneratedNoiseKind, SoundscapeLayer, SoundscapeStartRequest};
use ganbaru_music_library::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use std::sync::{Arc, LazyLock, atomic::AtomicBool};
use std::time::Instant;

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_SOUND_SOURCE_ID_BYTES: i64 = 200;
const MAX_SOUND_PATH_BYTES: usize = 16_384;
static DELIVERY_GATE: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));

#[derive(sqlx::FromRow)]
struct AcceptedPresetSound {
    soundscape_id: String,
    level: f64,
    generated_kind: Option<String>,
    source_kind: String,
}

pub(super) struct PreparedSoundscape {
    action: Option<Option<SoundscapeStartRequest>>,
    version: Option<i64>,
    pub missing: bool,
}

impl PreparedSoundscape {
    /// Whether the soundscape effect changed state that must be committed and delivered.
    pub(super) fn acts(&self) -> bool {
        self.action.is_some()
    }
}

/// Read only the selected preset, with SQL size predicates before path allocation. A preset plays
/// the sounds still available on this device and reports missing only when none remain.
pub(super) async fn prepare(
    connection: &mut sqlx::SqliteConnection,
    local_paths: &BTreeMap<String, String>,
    behavior: &str,
    preset_id: Option<&str>,
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
        // A phase that chooses nothing stops a preset an earlier phase or event started, so it
        // never carries over, and leaves a soundscape the listener started by hand.
        let automatic_playing: bool = sqlx::query_scalar(
            "SELECT automatic_intent = 1 AND desired_playing = 1
             FROM music_soundscape_state WHERE singleton = 1",
        )
        .fetch_one(&mut *connection)
        .await
        .map_err(|error| MusicLibraryError::database("read accepted soundscape intent", error))?;
        if !automatic_playing {
            return Ok(empty(false));
        }
    }
    let request = if behavior == MusicSoundscapeBehavior::PlaySelected {
        let Some(preset_id) = preset_id else {
            return Ok(empty(true));
        };
        let Some(request) = read_preset(connection, local_paths, preset_id).await? else {
            return Ok(empty(true));
        };
        Some((preset_id, request))
    } else {
        None
    };
    let version: i64 = sqlx::query_scalar(
        "UPDATE music_soundscape_state
         SET active_preset_id = CASE WHEN ? THEN ? ELSE active_preset_id END,
             desired_playing = ?, automatic_intent = 1, version = version + 1, updated_at_ms = MAX(updated_at_ms + 1, ?)
         WHERE singleton = 1 RETURNING version",
    )
    .bind(request.is_some())
    .bind(request.as_ref().map(|(preset_id, _)| *preset_id))
    .bind(request.is_some())
    .bind(now_ms())
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("persist accepted soundscape", error))?;
    Ok(PreparedSoundscape {
        action: Some(request.map(|(_, request)| request)),
        version: Some(version),
        missing: false,
    })
}

/// The preset as one engine request over its sounds available on this device, in playback order.
async fn read_preset(
    connection: &mut sqlx::SqliteConnection,
    local_paths: &BTreeMap<String, String>,
    preset_id: &str,
) -> MusicLibraryResult<Option<SoundscapeStartRequest>> {
    let preset_level: Option<f64> =
        sqlx::query_scalar("SELECT level FROM music_soundscape_presets WHERE id = ?")
            .bind(preset_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(|error| {
                MusicLibraryError::database("read accepted soundscape preset", error)
            })?;
    let Some(preset_level) = preset_level else {
        return Ok(None);
    };
    // A preset holds at most MAX_SOUNDSCAPE_LAYERS sounds, so the limit never hides a playable one.
    let sounds: Vec<AcceptedPresetSound> = sqlx::query_as(
        "SELECT ps.soundscape_id, ps.level, s.generated_kind, s.source_kind
         FROM music_soundscape_preset_sounds ps
         JOIN music_soundscapes s ON s.id = ps.soundscape_id
         WHERE ps.preset_id = ? AND length(CAST(s.id AS BLOB)) <= ?
           AND (s.source_kind = 'local-loop' OR s.availability = 'available')
         ORDER BY ps.position LIMIT ?",
    )
    .bind(preset_id)
    .bind(MAX_SOUND_SOURCE_ID_BYTES)
    .bind(MAX_SOUNDSCAPE_LAYERS as i64)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("read accepted soundscape sounds", error))?;
    let mut layers = sounds
        .into_iter()
        .filter_map(|sound| with_local_path(sound, local_paths))
        .map(layer)
        .collect::<MusicLibraryResult<Vec<_>>>()?
        .into_iter();
    let Some(first) = layers.next() else {
        return Ok(None);
    };
    Ok(Some(SoundscapeStartRequest {
        source_id: first.source_id,
        generated_kind: first.generated_kind,
        local_path: first.local_path,
        level: first.level,
        extra_sources: layers.collect(),
        preset_level,
        keep_playing_layers: false,
    }))
}

/// Pairs a sound with the file it plays on this device. A local loop with no file recorded here,
/// an oversized path, or a file that is gone is skipped, like any other unavailable sound.
fn with_local_path(
    sound: AcceptedPresetSound,
    local_paths: &BTreeMap<String, String>,
) -> Option<(AcceptedPresetSound, Option<String>)> {
    if sound.source_kind != "local-loop" {
        return Some((sound, None));
    }
    let path = local_paths.get(&sound.soundscape_id)?;
    (path.len() <= MAX_SOUND_PATH_BYTES && std::path::Path::new(path).is_file())
        .then(|| (sound, Some(path.clone())))
}

fn layer(
    (sound, local_path): (AcceptedPresetSound, Option<String>),
) -> MusicLibraryResult<SoundscapeLayer> {
    let generated_kind = match sound.generated_kind.as_deref() {
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
    Ok(SoundscapeLayer {
        source_id: sound.soundscape_id,
        generated_kind,
        local_path,
        level: sound.level,
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
            crate::music::soundscape::stop_blocking(&app)
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
            MusicLibraryError::conflict("An earlier soundscape delivery is still draining")
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
            let guard_app = app.clone();
            crate::music::soundscape::apply_automatic(
                &app,
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
                            eprintln!("soundscape authority: {error}");
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
                            eprintln!("soundscape state authority: {error}");
                            false
                        }
                    }
                }),
            )
            .map_err(|error| {
                MusicLibraryError::runtime("execute accepted soundscape", format!("{error:?}"))
            })
        });
        let result = match tokio::time::timeout(DELIVERY_TIMEOUT, worker).await {
            Ok(result) => result.map_err(|error| {
                MusicLibraryError::runtime("receive soundscape delivery", error)
            })?,
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(MusicLibraryError::runtime(
                    "soundscape delivery",
                    "native acknowledgement timed out; the retained worker must drain",
                ))
            }
        };
        if result.is_err() {
            let _permit = self.write_permit().await?;
            sqlx::query("UPDATE music_soundscape_state SET desired_playing = 0, version = version + 1, updated_at_ms = MAX(updated_at_ms + 1, ?) WHERE singleton = 1 AND version = ?")
                .bind(now_ms()).bind(accepted_version).execute(self.pool.as_ref().expect("initialized music pool")).await
                .map_err(|error| MusicLibraryError::database("revoke failed automatic soundscape", error))?;
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests;
