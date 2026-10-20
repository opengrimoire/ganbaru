use super::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use super::*;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

const MAX_PRESET_NAME_CHARS: usize = 80;
/// Highest preset level, which doubles the Music volume before output clamping.
const MAX_PRESET_LEVEL: f64 = 2.0;

/// Every preset with its sounds in playback order, sorted by name.
pub async fn presets(pool: &SqlitePool) -> MusicLibraryResult<Vec<MusicSoundscapePreset>> {
    let rows = sqlx::query_as::<_, (String, String, String, f64, i64, i64, i64)>(
        "SELECT id, name, icon, level, created_at_ms, updated_at_ms, version
         FROM music_soundscape_presets ORDER BY lower(name), id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape presets", error))?;
    let mut sounds = HashMap::<String, Vec<MusicSoundscapePresetSound>>::new();
    for (preset_id, soundscape_id, level) in sqlx::query_as::<_, (String, String, f64)>(
        "SELECT preset_id, soundscape_id, level FROM music_soundscape_preset_sounds
         ORDER BY preset_id, position",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape preset sounds", error))?
    {
        sounds
            .entry(preset_id)
            .or_default()
            .push(MusicSoundscapePresetSound {
                soundscape_id,
                level,
            });
    }
    Ok(rows
        .into_iter()
        .map(|row| MusicSoundscapePreset {
            sounds: sounds.remove(&row.0).unwrap_or_default(),
            id: row.0,
            name: row.1,
            icon: row.2,
            level: row.3,
            created_at_ms: row.4,
            updated_at_ms: row.5,
            version: row.6,
        })
        .collect())
}

/// One preset with its sounds.
pub async fn preset(
    pool: &SqlitePool,
    preset_id: &str,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    presets(pool)
        .await?
        .into_iter()
        .find(|preset| preset.id == preset_id)
        .ok_or_else(|| MusicLibraryError::not_found("soundscape preset", preset_id))
}

/// Creates or replaces a preset, including its ordered sounds and levels, under optimistic versioning.
pub async fn upsert(
    pool: &SqlitePool,
    request: MusicSoundscapePresetWrite,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    validate_write(&request)?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin soundscape preset update", error))?;
    for sound in &request.sounds {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM music_soundscapes WHERE id = ?)")
                .bind(&sound.soundscape_id)
                .fetch_one(&mut *transaction)
                .await
                .map_err(|error| {
                    MusicLibraryError::database("check soundscape preset sound", error)
                })?;
        if !exists {
            return Err(MusicLibraryError::not_found(
                "soundscape sound",
                &sound.soundscape_id,
            ));
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
        "INSERT INTO music_soundscape_presets (id, name, icon, level, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon, level = excluded.level,
           updated_at_ms = excluded.updated_at_ms, version = excluded.version",
    )
    .bind(&request.id)
    .bind(&name)
    .bind(&request.icon)
    .bind(request.level)
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
    for (position, sound) in request.sounds.iter().enumerate() {
        sqlx::query(
            "INSERT INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id, level)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&request.id)
        .bind(position as i64)
        .bind(&sound.soundscape_id)
        .bind(sound.level)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset sound", error))?;
    }
    super::writes::commit(transaction, "commit soundscape preset update").await?;
    Ok(MusicSoundscapePreset {
        id: request.id,
        name,
        icon: request.icon,
        level: request.level,
        sounds: request.sounds,
        created_at_ms,
        updated_at_ms: request.updated_at_ms,
        version,
    })
}

/// Saves only a preset's level, the setting the soundscape controls adjust while it plays.
///
/// The latest level wins without a version check, so a slider never conflicts with itself; the version still
/// advances so a concurrent full edit notices the change.
pub async fn set_level(
    pool: &SqlitePool,
    preset_id: &str,
    level: f64,
    updated_at_ms: i64,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    validate_id(preset_id, "presetId")?;
    validate_preset_level(level)?;
    if updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    let result = sqlx::query(
        "UPDATE music_soundscape_presets
         SET level = ?, updated_at_ms = MAX(updated_at_ms + 1, ?), version = version + 1
         WHERE id = ?",
    )
    .bind(level)
    .bind(updated_at_ms)
    .bind(preset_id)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("save soundscape preset level", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::not_found("soundscape preset", preset_id));
    }
    preset(pool, preset_id).await
}

/// Removes a preset; assignments that still name it report it as unavailable when they activate.
pub async fn remove(
    pool: &SqlitePool,
    preset_id: &str,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    validate_id(preset_id, "presetId")?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin soundscape preset removal", error))?;
    let result = sqlx::query("DELETE FROM music_soundscape_presets WHERE id = ? AND version = ?")
        .bind(preset_id)
        .bind(expected_version)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove soundscape preset", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the soundscape preset changed before removal",
        ));
    }
    // The foreign key clears the active preset; nothing is left to play, so playback stops too.
    sqlx::query(
        "UPDATE music_soundscape_state
         SET active_preset_id = NULL, desired_playing = 0, updated_at_ms = updated_at_ms + 1,
             version = version + 1
         WHERE singleton = 1 AND desired_playing = 1 AND active_preset_id IS NULL",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("stop the removed soundscape preset", error))?;
    super::writes::commit(transaction, "commit soundscape preset removal").await
}

fn validate_preset_level(level: f64) -> MusicLibraryResult<()> {
    if !level.is_finite() || !(0.0..=MAX_PRESET_LEVEL).contains(&level) {
        return Err(MusicLibraryError::validation(
            "level",
            format!("must be a finite value between zero and {MAX_PRESET_LEVEL}"),
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
    validate_preset_level(request.level)?;
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    if request.sounds.is_empty() || request.sounds.len() > MAX_SOUNDSCAPE_LAYERS {
        return Err(MusicLibraryError::validation(
            "sounds",
            format!("must contain between one and {MAX_SOUNDSCAPE_LAYERS} sounds"),
        ));
    }
    let mut seen = HashSet::new();
    for sound in &request.sounds {
        validate_id(&sound.soundscape_id, "sounds")?;
        if !seen.insert(sound.soundscape_id.as_str()) {
            return Err(MusicLibraryError::validation(
                "sounds",
                "must not contain duplicates",
            ));
        }
        if !sound.level.is_finite() || !(0.0..=1.0).contains(&sound.level) {
            return Err(MusicLibraryError::validation(
                "sounds",
                "levels must be finite values between zero and one",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::pool;

    fn sounds(entries: &[(&str, f64)]) -> Vec<MusicSoundscapePresetSound> {
        entries
            .iter()
            .map(|(id, level)| MusicSoundscapePresetSound {
                soundscape_id: (*id).to_owned(),
                level: *level,
            })
            .collect()
    }

    fn write(
        id: &str,
        entries: &[(&str, f64)],
        expected_version: Option<i64>,
    ) -> MusicSoundscapePresetWrite {
        MusicSoundscapePresetWrite {
            id: id.into(),
            name: " Rainy desk ".into(),
            icon: "lucide:cloud-rain".into(),
            level: 0.5,
            sounds: sounds(entries),
            expected_version,
            updated_at_ms: 1_700_000_000_000,
        }
    }

    /// Fresh vaults start with one seeded rain preset per generated sound.
    async fn custom_presets(pool: &SqlitePool) -> Vec<MusicSoundscapePreset> {
        presets(pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|preset| !preset.id.starts_with("preset-generated-"))
            .collect()
    }

    #[test]
    fn fresh_vaults_offer_each_generated_sound_as_a_preset() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let seeded = presets(&pool).await.unwrap();
            assert_eq!(seeded.len(), 3);
            assert!(seeded.iter().all(|preset| preset.level == 0.5
                && preset.sounds.len() == 1
                && preset.sounds[0].level == 1.0
                && preset.id == format!("preset-{}", preset.sounds[0].soundscape_id)));
        });
    }

    #[test]
    fn presets_keep_sound_order_and_levels_and_require_the_current_version() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(
                &pool,
                write(
                    "desk",
                    &[
                        ("generated-brown-noise", 0.25),
                        ("generated-white-noise", 1.0),
                    ],
                    None,
                ),
            )
            .await
            .unwrap();
            assert_eq!(created.name, "Rainy desk");
            assert_eq!(created.version, 1);
            assert_eq!(custom_presets(&pool).await, vec![created.clone()]);
            assert!(
                upsert(&pool, write("desk", &[("generated-pink-noise", 1.0)], None))
                    .await
                    .is_err()
            );
            let updated = upsert(
                &pool,
                write(
                    "desk",
                    &[
                        ("generated-white-noise", 0.5),
                        ("generated-pink-noise", 0.0),
                    ],
                    Some(1),
                ),
            )
            .await
            .unwrap();
            assert_eq!(updated.version, 2);
            assert_eq!(
                preset(&pool, "desk").await.unwrap().sounds,
                sounds(&[
                    ("generated-white-noise", 0.5),
                    ("generated-pink-noise", 0.0)
                ])
            );
            assert!(remove(&pool, "desk", 1).await.is_err());
            remove(&pool, "desk", 2).await.unwrap();
            assert!(custom_presets(&pool).await.is_empty());
        });
    }

    #[test]
    fn preset_levels_save_alone_and_stay_in_range() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(&pool, write("desk", &[("generated-pink-noise", 0.8)], None))
                .await
                .unwrap();
            let louder = set_level(&pool, "desk", 1.5, 1_700_000_000_001)
                .await
                .unwrap();
            assert_eq!(louder.level, 1.5);
            assert_eq!(louder.version, created.version + 1);
            assert_eq!(louder.sounds, created.sounds);
            for level in [-0.1, 2.1, f64::NAN] {
                assert!(
                    set_level(&pool, "desk", level, 1_700_000_000_002)
                        .await
                        .is_err()
                );
                let mut invalid = write("other", &[("generated-pink-noise", 1.0)], None);
                invalid.level = level;
                assert!(upsert(&pool, invalid).await.is_err());
            }
            assert!(
                set_level(&pool, "removed", 1.0, 1_700_000_000_002)
                    .await
                    .is_err()
            );
        });
    }

    #[test]
    fn presets_reject_empty_duplicate_unknown_and_out_of_range_sounds() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            assert!(upsert(&pool, write("empty", &[], None)).await.is_err());
            assert!(
                upsert(
                    &pool,
                    write(
                        "twice",
                        &[
                            ("generated-brown-noise", 1.0),
                            ("generated-brown-noise", 0.5)
                        ],
                        None
                    )
                )
                .await
                .is_err()
            );
            assert!(
                upsert(&pool, write("unknown", &[("removed-sound", 1.0)], None))
                    .await
                    .is_err()
            );
            for level in [-0.1, 1.1, f64::INFINITY] {
                assert!(
                    upsert(
                        &pool,
                        write("loud", &[("generated-brown-noise", level)], None)
                    )
                    .await
                    .is_err()
                );
            }
            assert!(custom_presets(&pool).await.is_empty());
        });
    }

    #[test]
    fn removing_the_active_preset_stops_the_soundscape() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(&pool, write("desk", &[("generated-pink-noise", 1.0)], None))
                .await
                .unwrap();
            let initial = crate::soundscapes::state(&pool).await.unwrap();
            crate::soundscapes::update_state(
                &pool,
                MusicSoundscapeStateWrite {
                    active_preset_id: Some("desk".into()),
                    desired_playing: true,
                    expected_version: initial.version,
                    updated_at_ms: 1_700_000_000_001,
                },
            )
            .await
            .unwrap();
            remove(&pool, "desk", created.version).await.unwrap();
            let state = crate::soundscapes::state(&pool).await.unwrap();
            assert_eq!(state.active_preset_id, None);
            assert!(!state.desired_playing);
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
                write(
                    "storm",
                    &[("local-storm", 1.0), ("generated-pink-noise", 0.4)],
                    None,
                ),
            )
            .await
            .unwrap();
            crate::soundscapes::remove(&pool, "local-storm", 1)
                .await
                .unwrap();
            assert_eq!(
                preset(&pool, "storm").await.unwrap().sounds,
                sounds(&[("generated-pink-noise", 0.4)])
            );
        });
    }
}
