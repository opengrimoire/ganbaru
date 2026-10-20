use super::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use super::*;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

const MAX_PRESET_NAME_CHARS: usize = 80;

/// Every preset with its sounds in playback order, sorted by name.
pub async fn presets(pool: &SqlitePool) -> MusicLibraryResult<Vec<MusicSoundscapePreset>> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64)>(
        "SELECT id, name, icon, created_at_ms, updated_at_ms, version
         FROM music_soundscape_presets ORDER BY lower(name), id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape presets", error))?;
    let mut sounds = HashMap::<String, Vec<String>>::new();
    for (preset_id, soundscape_id) in sqlx::query_as::<_, (String, String)>(
        "SELECT preset_id, soundscape_id FROM music_soundscape_preset_sounds
         ORDER BY preset_id, position",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape preset sounds", error))?
    {
        sounds.entry(preset_id).or_default().push(soundscape_id);
    }
    Ok(rows
        .into_iter()
        .map(|row| MusicSoundscapePreset {
            soundscape_ids: sounds.remove(&row.0).unwrap_or_default(),
            id: row.0,
            name: row.1,
            icon: row.2,
            created_at_ms: row.3,
            updated_at_ms: row.4,
            version: row.5,
        })
        .collect())
}

/// Creates or replaces a preset, including its ordered sounds, under optimistic versioning.
pub async fn upsert(
    pool: &SqlitePool,
    request: MusicSoundscapePresetWrite,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    validate_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin soundscape preset update", error))?;
    for id in &request.soundscape_ids {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_soundscapes WHERE id = ?)")
                .bind(id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("check soundscape preset sound", error)
                })?;
        if !exists {
            return Err(MusicLibraryError::not_found("soundscape sound", id));
        }
    }
    let existing = sqlx::query_as::<_, (i64, i64)>(
        "SELECT created_at_ms, version FROM music_soundscape_presets WHERE id = ?",
    )
    .bind(&request.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape preset version", error))?;
    if let Some((_, version)) = existing {
        if request.expected_version != Some(version) {
            return Err(MusicLibraryError::conflict(
                "the soundscape preset changed before this update",
            ));
        }
    } else if request.expected_version.is_some() {
        return Err(MusicLibraryError::not_found(
            "soundscape preset",
            &request.id,
        ));
    }
    let created_at_ms = existing.map(|row| row.0).unwrap_or(request.updated_at_ms);
    let version = existing.map(|row| row.1 + 1).unwrap_or(1);
    let name = request.name.trim().to_owned();
    sqlx::query(
        "INSERT INTO music_soundscape_presets (id, name, icon, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon,
           updated_at_ms = excluded.updated_at_ms, version = excluded.version",
    )
    .bind(&request.id)
    .bind(&name)
    .bind(&request.icon)
    .bind(created_at_ms)
    .bind(request.updated_at_ms)
    .bind(version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save soundscape preset", error))?;
    sqlx::query("DELETE FROM music_soundscape_preset_sounds WHERE preset_id = ?")
        .bind(&request.id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("replace soundscape preset sounds", error))?;
    for (position, soundscape_id) in request.soundscape_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id)
             VALUES (?, ?, ?)",
        )
        .bind(&request.id)
        .bind(position as i64)
        .bind(soundscape_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset sound", error))?;
    }
    super::writes::commit(transaction, "commit soundscape preset update").await?;
    Ok(MusicSoundscapePreset {
        id: request.id,
        name,
        icon: request.icon,
        soundscape_ids: request.soundscape_ids,
        created_at_ms,
        updated_at_ms: request.updated_at_ms,
        version,
    })
}

/// Removes a preset; assignments that still name it report it as unavailable when they activate.
pub async fn remove(
    pool: &SqlitePool,
    preset_id: &str,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    validate_id(preset_id, "presetId")?;
    let result = sqlx::query("DELETE FROM music_soundscape_presets WHERE id = ? AND version = ?")
        .bind(preset_id)
        .bind(expected_version)
        .execute(pool)
        .await
        .map_err(|error| MusicLibraryError::database("remove soundscape preset", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the soundscape preset changed before removal",
        ));
    }
    Ok(())
}

fn validate_write(request: &MusicSoundscapePresetWrite) -> MusicLibraryResult<()> {
    validate_id(&request.id, "id")?;
    let name_chars = request.name.trim().chars().count();
    if name_chars == 0 || name_chars > MAX_PRESET_NAME_CHARS {
        return Err(MusicLibraryError::validation(
            "name",
            format!("must contain between one and {MAX_PRESET_NAME_CHARS} characters"),
        ));
    }
    validate_icon(&request.icon)?;
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    if request.soundscape_ids.is_empty() || request.soundscape_ids.len() > MAX_SOUNDSCAPE_LAYERS {
        return Err(MusicLibraryError::validation(
            "soundscapeIds",
            format!("must contain between one and {MAX_SOUNDSCAPE_LAYERS} sounds"),
        ));
    }
    let mut seen = HashSet::new();
    for id in &request.soundscape_ids {
        validate_id(id, "soundscapeIds")?;
        if !seen.insert(id.as_str()) {
            return Err(MusicLibraryError::validation(
                "soundscapeIds",
                "must not contain duplicates",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::pool;

    fn write(
        id: &str,
        sounds: &[&str],
        expected_version: Option<i64>,
    ) -> MusicSoundscapePresetWrite {
        MusicSoundscapePresetWrite {
            id: id.into(),
            name: " Rainy desk ".into(),
            icon: "lucide:cloud-rain".into(),
            soundscape_ids: sounds.iter().map(|id| (*id).to_owned()).collect(),
            expected_version,
            updated_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn presets_keep_sound_order_and_require_the_current_version() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(
                &pool,
                write(
                    "desk",
                    &["generated-brown-noise", "generated-white-noise"],
                    None,
                ),
            )
            .await
            .unwrap();
            assert_eq!(created.name, "Rainy desk");
            assert_eq!(created.version, 1);
            assert_eq!(presets(&pool).await.unwrap(), vec![created.clone()]);
            assert!(
                upsert(&pool, write("desk", &["generated-pink-noise"], None))
                    .await
                    .is_err()
            );
            let updated = upsert(
                &pool,
                write(
                    "desk",
                    &["generated-white-noise", "generated-pink-noise"],
                    Some(1),
                ),
            )
            .await
            .unwrap();
            assert_eq!(updated.version, 2);
            assert_eq!(
                presets(&pool).await.unwrap()[0].soundscape_ids,
                vec!["generated-white-noise", "generated-pink-noise"]
            );
            assert!(remove(&pool, "desk", 1).await.is_err());
            remove(&pool, "desk", 2).await.unwrap();
            assert!(presets(&pool).await.unwrap().is_empty());
        });
    }

    #[test]
    fn presets_reject_empty_duplicate_and_unknown_sounds() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            assert!(upsert(&pool, write("empty", &[], None)).await.is_err());
            assert!(
                upsert(
                    &pool,
                    write(
                        "twice",
                        &["generated-brown-noise", "generated-brown-noise"],
                        None
                    )
                )
                .await
                .is_err()
            );
            assert!(
                upsert(&pool, write("unknown", &["removed-sound"], None))
                    .await
                    .is_err()
            );
            assert!(presets(&pool).await.unwrap().is_empty());
        });
    }

    #[test]
    fn removing_a_sound_drops_it_from_presets() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            sqlx::query("INSERT INTO music_soundscapes (id, source_kind, name, availability, created_at_ms, updated_at_ms, version) VALUES ('local-storm', 'local-loop', 'Storm', 'missing', 1, 1, 1)")
                .execute(&pool).await.unwrap();
            upsert(
                &pool,
                write("storm", &["local-storm", "generated-pink-noise"], None),
            )
            .await
            .unwrap();
            crate::soundscapes::remove(&pool, "local-storm", 1)
                .await
                .unwrap();
            assert_eq!(
                presets(&pool).await.unwrap()[0].soundscape_ids,
                vec!["generated-pink-noise"]
            );
        });
    }
}
