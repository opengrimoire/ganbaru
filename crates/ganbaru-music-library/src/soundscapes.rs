use super::*;
use rodio::Decoder;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

/// Maximum number of soundscape layers that can play at once.
pub const MAX_SOUNDSCAPE_LAYERS: usize = 16;

type DefinitionRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    i64,
    i64,
    i64,
);

/// Lists every sound with the file each local loop uses on this device.
///
/// `local_paths` maps sound IDs to absolute paths held in device-local state. The vault never
/// stores them, so a copied vault carries a sound's identity without another device's paths,
/// and a local loop without an entry reads as missing until the user picks its file here.
pub async fn definitions(
    pool: &SqlitePool,
    local_paths: &BTreeMap<String, String>,
) -> MusicLibraryResult<Vec<MusicSoundscapeDefinition>> {
    let rows = sqlx::query_as::<_, DefinitionRow>(
        "SELECT id, source_kind, generated_kind, bundled_identity, name, icon,
                availability, created_at_ms, updated_at_ms, version
         FROM music_soundscapes
         ORDER BY CASE generated_kind
                    WHEN 'brown' THEN 0 WHEN 'pink' THEN 1 WHEN 'white' THEN 2 ELSE 3 END,
                  lower(name), id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscapes", error))?;
    rows.into_iter()
        .map(|row| {
            let mut definition = decode_definition(row)?;
            if definition.source_kind == MusicSoundscapeSourceKind::LocalLoop {
                definition.local_path = local_paths.get(&definition.id).cloned();
                definition.availability = local_path_availability(definition.local_path.as_deref());
            }
            Ok(definition)
        })
        .collect()
}

/// Saves a sound's vault identity. `stored_path` is the file this device already uses for a
/// local loop; a different `local_path` is validated, and the returned definition carries the
/// path the caller keeps in device-local state.
pub async fn upsert(
    pool: &SqlitePool,
    request: MusicSoundscapeWrite,
    stored_path: Option<&str>,
) -> MusicLibraryResult<MusicSoundscapeDefinition> {
    validate_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin soundscape update", error))?;
    let existing = sqlx::query_as::<_, (i64, i64, String)>(
        "SELECT created_at_ms, version, source_kind FROM music_soundscapes WHERE id = ?",
    )
    .bind(&request.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape version", error))?;
    if let Some((_, version, source_kind)) = &existing {
        if request.expected_version != Some(*version) {
            return Err(MusicLibraryError::conflict(
                "the soundscape changed before this update",
            ));
        }
        if source_kind != request.source_kind.as_ref() {
            return Err(MusicLibraryError::validation(
                "sourceKind",
                "cannot change a sound's source kind",
            ));
        }
    } else if request.expected_version.is_some() {
        return Err(MusicLibraryError::not_found("soundscape", &request.id));
    }
    let local_loop = request.source_kind == MusicSoundscapeSourceKind::LocalLoop;
    let stored_path = stored_path.filter(|_| local_loop && existing.is_some());
    let new_location = local_loop
        && request
            .local_path
            .as_deref()
            .is_some_and(|path| stored_path != Some(path));
    if request.source_kind == MusicSoundscapeSourceKind::LocalLoop
        && existing.is_none()
        && !new_location
    {
        return Err(MusicLibraryError::validation(
            "localPath",
            "a new local sound requires an audio file",
        ));
    }
    if new_location {
        validate_local_audio_path(
            request
                .local_path
                .as_deref()
                .expect("new location requires a path"),
        )?;
    }
    let created_at_ms = existing
        .as_ref()
        .map(|row| row.0)
        .unwrap_or(request.updated_at_ms);
    let version = existing.as_ref().map(|row| row.1 + 1).unwrap_or(1);
    sqlx::query(
        "INSERT INTO music_soundscapes
            (id, source_kind, generated_kind, bundled_identity, name, icon, availability,
             created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            source_kind = excluded.source_kind,
            generated_kind = excluded.generated_kind,
            bundled_identity = excluded.bundled_identity,
            name = excluded.name,
            icon = excluded.icon,
            availability = excluded.availability,
            updated_at_ms = excluded.updated_at_ms,
            version = excluded.version",
    )
    .bind(&request.id)
    .bind(request.source_kind.as_ref())
    .bind(
        request
            .generated_kind
            .map(|value| value.as_ref().to_string()),
    )
    .bind(&request.bundled_identity)
    .bind(request.name.trim())
    .bind(&request.icon)
    .bind(MusicSoundscapeAvailability::Available.as_ref())
    .bind(created_at_ms)
    .bind(request.updated_at_ms)
    .bind(version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save soundscape", error))?;
    super::writes::commit(transaction, "commit soundscape update").await?;
    let effective_path = if new_location {
        request.local_path.as_deref()
    } else {
        stored_path
    };
    let local_paths = effective_path
        .map(|path| BTreeMap::from([(request.id.clone(), path.to_string())]))
        .unwrap_or_default();
    definition(pool, &local_paths, &request.id).await
}

pub async fn remove(
    pool: &SqlitePool,
    soundscape_id: &str,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    validate_id(soundscape_id, "soundscapeId")?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin soundscape removal", error))?;
    let result = sqlx::query(
        "DELETE FROM music_soundscapes
         WHERE id = ? AND source_kind = 'local-loop' AND version = ?",
    )
    .bind(soundscape_id)
    .bind(expected_version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("remove soundscape", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the soundscape changed or cannot be removed",
        ));
    }
    // Preset sounds cascade with the sound; an active preset left silent stops wanting playback.
    sqlx::query(
        "UPDATE music_soundscape_state
         SET desired_playing = 0, updated_at_ms = updated_at_ms + 1, version = version + 1
         WHERE singleton = 1 AND desired_playing = 1 AND NOT EXISTS (
             SELECT 1 FROM music_soundscape_preset_sounds
             WHERE preset_id = music_soundscape_state.active_preset_id
         )",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("stop the emptied soundscape preset", error))?;
    super::writes::commit(transaction, "commit soundscape removal").await
}

/// The remembered soundscape preset and whether it should play.
pub async fn state(pool: &SqlitePool) -> MusicLibraryResult<MusicSoundscapeState> {
    let row = sqlx::query_as::<_, (Option<String>, bool, i64, i64, bool)>(
        "SELECT active_preset_id, desired_playing, updated_at_ms, version, automatic_intent
         FROM music_soundscape_state WHERE singleton = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape state", error))?;
    Ok(MusicSoundscapeState {
        automatic_intent: row.4,
        active_preset_id: row.0,
        desired_playing: row.1,
        updated_at_ms: row.2,
        version: row.3,
    })
}

/// Records a manual preset choice under optimistic versioning, clearing automatic intent.
pub async fn update_state(
    pool: &SqlitePool,
    request: MusicSoundscapeStateWrite,
) -> MusicLibraryResult<MusicSoundscapeState> {
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    if let Some(preset_id) = &request.active_preset_id {
        validate_id(preset_id, "activePresetId")?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM music_soundscape_presets WHERE id = ?)",
        )
        .bind(preset_id)
        .fetch_one(pool)
        .await
        .map_err(|error| MusicLibraryError::database("validate active soundscape preset", error))?;
        if !exists {
            return Err(MusicLibraryError::not_found("soundscape preset", preset_id));
        }
    } else if request.desired_playing {
        return Err(MusicLibraryError::validation(
            "desiredPlaying",
            "cannot be enabled without an active soundscape preset",
        ));
    }
    let result = sqlx::query(
        "UPDATE music_soundscape_state
         SET active_preset_id = ?, desired_playing = ?, updated_at_ms = ?, version = version + 1,
             automatic_intent = 0
         WHERE singleton = 1 AND version = ?",
    )
    .bind(&request.active_preset_id)
    .bind(request.desired_playing)
    .bind(request.updated_at_ms)
    .bind(request.expected_version)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("update soundscape state", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the soundscape state changed before this update",
        ));
    }
    state(pool).await
}

async fn definition(
    pool: &SqlitePool,
    local_paths: &BTreeMap<String, String>,
    soundscape_id: &str,
) -> MusicLibraryResult<MusicSoundscapeDefinition> {
    definitions(pool, local_paths)
        .await?
        .into_iter()
        .find(|definition| definition.id == soundscape_id)
        .ok_or_else(|| MusicLibraryError::not_found("soundscape", soundscape_id))
}

fn validate_write(request: &MusicSoundscapeWrite) -> MusicLibraryResult<()> {
    validate_id(&request.id, "id")?;
    if request.name.trim().is_empty() || request.name.chars().count() > 200 {
        return Err(MusicLibraryError::validation(
            "name",
            "must contain between one and 200 characters",
        ));
    }
    super::validate_icon(&request.icon)?;
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    match request.source_kind {
        MusicSoundscapeSourceKind::GeneratedNoise
            if request.generated_kind.is_none()
                || request.bundled_identity.is_some()
                || request.local_path.is_some() =>
        {
            Err(MusicLibraryError::validation(
                "sourceKind",
                "generated noise requires only a generated kind",
            ))
        }
        MusicSoundscapeSourceKind::LocalLoop
            if request.generated_kind.is_some() || request.bundled_identity.is_some() =>
        {
            Err(MusicLibraryError::validation(
                "localPath",
                "a local loop cannot have generated or bundled media",
            ))
        }
        MusicSoundscapeSourceKind::BundledLoop
            if request.generated_kind.is_some()
                || request.local_path.is_some()
                || request
                    .bundled_identity
                    .as_deref()
                    .is_none_or(str::is_empty) =>
        {
            Err(MusicLibraryError::validation(
                "bundledIdentity",
                "a bundled loop requires only a stable bundled identity",
            ))
        }
        _ => Ok(()),
    }
}

fn validate_local_audio_path(value: &str) -> MusicLibraryResult<()> {
    if value.contains('\0') || !Path::new(value).is_absolute() {
        return Err(MusicLibraryError::validation(
            "localPath",
            "must be an absolute path without null bytes",
        ));
    }
    let extension = Path::new(value)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if !matches!(
        extension.as_str(),
        "mp3" | "flac" | "m4a" | "mp4" | "ogg" | "oga" | "wav"
    ) {
        return Err(MusicLibraryError::validation(
            "localPath",
            "must use a supported audio extension",
        ));
    }
    if matches!(extension.as_str(), "m4v" | "mov" | "mkv" | "webm") {
        return Err(MusicLibraryError::validation(
            "localPath",
            "video files cannot be used as soundscape loops",
        ));
    }
    // A FIFO or device would block the open, and `metadata` never does.
    if !std::fs::metadata(value).is_ok_and(|metadata| metadata.is_file()) {
        return Err(MusicLibraryError::validation(
            "localPath",
            "must be an existing regular file",
        ));
    }
    let file = File::open(value).map_err(|error| {
        MusicLibraryError::validation("localPath", format!("cannot open the audio file: {error}"))
    })?;
    Decoder::try_from(file).map_err(|error| {
        MusicLibraryError::validation(
            "localPath",
            format!("cannot decode the audio file: {error}"),
        )
    })?;
    Ok(())
}

fn local_path_availability(value: Option<&str>) -> MusicSoundscapeAvailability {
    let Some(value) = value else {
        return MusicSoundscapeAvailability::Missing;
    };
    if !Path::new(value).is_file() {
        return MusicSoundscapeAvailability::Missing;
    }
    if validate_local_audio_path(value).is_ok() {
        MusicSoundscapeAvailability::Available
    } else {
        MusicSoundscapeAvailability::Unsupported
    }
}

fn decode_definition(row: DefinitionRow) -> MusicLibraryResult<MusicSoundscapeDefinition> {
    Ok(MusicSoundscapeDefinition {
        id: row.0,
        source_kind: MusicSoundscapeSourceKind::try_from(row.1.as_str())
            .map_err(|message| MusicLibraryError::runtime("decode soundscape source", message))?,
        generated_kind: row
            .2
            .as_deref()
            .map(MusicGeneratedNoiseKind::try_from)
            .transpose()
            .map_err(|message| MusicLibraryError::runtime("decode generated noise", message))?,
        bundled_identity: row.3,
        name: row.4,
        icon: row.5,
        availability: MusicSoundscapeAvailability::try_from(row.6.as_str()).map_err(|message| {
            MusicLibraryError::runtime("decode soundscape availability", message)
        })?,
        local_path: None,
        created_at_ms: row.7,
        updated_at_ms: row.8,
        version: row.9,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::pool;

    #[test]
    fn local_loop_validation_rejects_non_audio_and_relative_paths() {
        assert!(validate_local_audio_path("relative/rain.mp3").is_err());
        assert!(validate_local_audio_path("/tmp/rain.webm").is_err());
        assert!(validate_local_audio_path("https://youtube.com/watch?v=abc").is_err());
    }

    #[test]
    fn local_loop_validation_rejects_directories_without_opening_them() {
        let directory = std::env::temp_dir().join(format!(
            "ganbaru-soundscape-directory-{}.mp3",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let result = validate_local_audio_path(&directory.to_string_lossy());
        std::fs::remove_dir_all(&directory).unwrap();

        let error = result.unwrap_err();
        assert_eq!(error.message, "must be an existing regular file");
    }

    #[cfg(unix)]
    #[test]
    fn local_loop_validation_rejects_a_fifo_without_blocking() {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-soundscape-fifo-{}.mp3",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let result = validate_local_audio_path(&path.to_string_lossy());
        std::fs::remove_file(&path).unwrap();

        assert!(result.is_err());
    }

    /// Names every vault table with a text value containing `needle`.
    async fn tables_mentioning(pool: &SqlitePool, needle: &str) -> Vec<String> {
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        let mut found = Vec::new();
        for table in tables {
            let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
                .bind(&table)
                .fetch_all(pool)
                .await
                .unwrap();
            let predicate = columns
                .iter()
                .map(|column| format!("instr(CAST(\"{column}\" AS TEXT), ?1) > 0"))
                .collect::<Vec<_>>()
                .join(" OR ");
            // Names come from the schema catalog, not from input.
            let mentioned: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT EXISTS(SELECT 1 FROM \"{table}\" WHERE {predicate})"
            )))
            .bind(needle)
            .fetch_one(pool)
            .await
            .unwrap();
            if mentioned {
                found.push(table);
            }
        }
        found
    }

    #[test]
    fn soundscape_crud_and_singleton_state_are_versioned() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let generated = definitions(&pool, &BTreeMap::new()).await.unwrap();
            assert_eq!(generated.len(), 3);
            assert!(
                generated
                    .iter()
                    .all(|entry| entry.icon == "lucide:audio-lines")
            );
            assert!(
                generated
                    .iter()
                    .all(|entry| entry.availability == MusicSoundscapeAvailability::Available)
            );

            let path = std::env::temp_dir().join(format!(
                "ganbaru-soundscape-{}-{}.wav",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            ));
            let mut wav = Vec::from(*b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x01\x00\x80\xbb\x00\x00\x00\x77\x01\x00\x02\x00\x10\x00data\x00\x00\x00\x00");
            std::fs::write(&path, &mut wav).unwrap();
            let saved = upsert(
                &pool,
                MusicSoundscapeWrite {
                    id: "local-rain".into(),
                    source_kind: MusicSoundscapeSourceKind::LocalLoop,
                    generated_kind: None,
                    bundled_identity: None,
                    name: "Rain".into(),
                    icon: "lucide:cloud-rain".into(),
                    local_path: Some(path.to_string_lossy().into_owned()),
                    expected_version: None,
                    updated_at_ms: 1_700_000_000_000,
                },
                None,
            )
            .await
            .unwrap();
            assert_eq!(saved.local_path.as_deref(), path.to_str());
            assert_eq!(saved.icon, "lucide:cloud-rain");
            assert_eq!(
                tables_mentioning(&pool, path.to_str().unwrap()).await,
                Vec::<String>::new()
            );
            let rain_paths =
                BTreeMap::from([(saved.id.clone(), path.to_string_lossy().into_owned())]);
            let reloaded = definitions(&pool, &rain_paths)
                .await
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == saved.id)
                .unwrap();
            assert_eq!(reloaded.local_path, saved.local_path);
            assert_eq!(
                reloaded.availability,
                MusicSoundscapeAvailability::Available
            );
            let invalid_icon = MusicSoundscapeWrite {
                id: saved.id.clone(),
                source_kind: MusicSoundscapeSourceKind::LocalLoop,
                generated_kind: None,
                bundled_identity: None,
                name: saved.name.clone(),
                icon: "unknown".into(),
                local_path: saved.local_path.clone(),
                expected_version: Some(saved.version),
                updated_at_ms: 1_700_000_000_001,
            };
            assert!(validate_write(&invalid_icon).is_err());

            let copied_device = definitions(&pool, &BTreeMap::new())
                .await
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == saved.id)
                .unwrap();
            assert_eq!(copied_device.name, "Rain");
            assert_eq!(copied_device.icon, "lucide:cloud-rain");
            assert_eq!(copied_device.local_path, None);
            assert_eq!(
                copied_device.availability,
                MusicSoundscapeAvailability::Missing
            );

            crate::soundscape_presets::upsert(
                &pool,
                MusicSoundscapePresetWrite {
                    id: "rain-only".into(),
                    name: "Rain".into(),
                    icon: "lucide:cloud-rain".into(),
                    level_milli: 500,
                    sounds: vec![MusicSoundscapePresetSound {
                        soundscape_id: saved.id.clone(),
                        level_milli: 1000,
                    }],
                    expected_version: None,
                    updated_at_ms: 1_700_000_000_001,
                },
            )
            .await
            .unwrap();
            let initial = state(&pool).await.unwrap();
            assert_eq!(initial.active_preset_id, None);
            let playing = update_state(
                &pool,
                MusicSoundscapeStateWrite {
                    active_preset_id: Some("rain-only".into()),
                    desired_playing: true,
                    expected_version: initial.version,
                    updated_at_ms: 1_700_000_000_001,
                },
            )
            .await
            .unwrap();
            assert_eq!(playing.active_preset_id.as_deref(), Some("rain-only"));
            assert!(playing.desired_playing);
            assert!(!playing.automatic_intent);
            for (active_preset_id, expected_version) in [
                (Some("removed-preset"), playing.version),
                (None, playing.version),
                (Some("rain-only"), initial.version),
            ] {
                assert!(
                    update_state(
                        &pool,
                        MusicSoundscapeStateWrite {
                            active_preset_id: active_preset_id.map(str::to_owned),
                            desired_playing: true,
                            expected_version,
                            updated_at_ms: 1_700_000_000_002,
                        }
                    )
                    .await
                    .is_err()
                );
            }

            std::fs::remove_file(&path).unwrap();
            let missing = definitions(&pool, &rain_paths)
                .await
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == saved.id)
                .unwrap();
            assert_eq!(missing.availability, MusicSoundscapeAvailability::Missing);
            let renamed = upsert(
                &pool,
                MusicSoundscapeWrite {
                    id: saved.id.clone(),
                    source_kind: MusicSoundscapeSourceKind::LocalLoop,
                    generated_kind: None,
                    bundled_identity: None,
                    name: "Rain at night".into(),
                    icon: "emoji:🌧️".into(),
                    local_path: Some(path.to_string_lossy().into_owned()),
                    expected_version: Some(saved.version),
                    updated_at_ms: 1_700_000_000_004,
                },
                saved.local_path.as_deref(),
            )
            .await
            .unwrap();
            assert_eq!(renamed.availability, MusicSoundscapeAvailability::Missing);
            assert_eq!(renamed.icon, "emoji:🌧️");
            let remote_rename = upsert(
                &pool,
                MusicSoundscapeWrite {
                    id: saved.id.clone(),
                    source_kind: MusicSoundscapeSourceKind::LocalLoop,
                    generated_kind: None,
                    bundled_identity: None,
                    name: "Rain overnight".into(),
                    icon: renamed.icon.clone(),
                    local_path: None,
                    expected_version: Some(renamed.version),
                    updated_at_ms: 1_700_000_000_005,
                },
                None,
            )
            .await
            .unwrap();
            assert_eq!(remote_rename.local_path, None);
            remove(&pool, &saved.id, remote_rename.version)
                .await
                .unwrap();
            let after_removal = state(&pool).await.unwrap();
            assert_eq!(after_removal.active_preset_id.as_deref(), Some("rain-only"));
            assert!(!after_removal.desired_playing);
            assert!(after_removal.version > playing.version);
            assert!(
                definitions(&pool, &rain_paths)
                    .await
                    .unwrap()
                    .iter()
                    .all(|entry| entry.id != saved.id)
            );
        });
    }
}
