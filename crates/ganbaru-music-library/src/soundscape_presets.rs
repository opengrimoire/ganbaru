use super::soundscapes::MAX_SOUNDSCAPE_LAYERS;
use super::*;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};

const MAX_PRESET_NAME_CHARS: usize = 80;
/// Preset level in thousandths of the Music volume; the top doubles it before output clamping.
const PRESET_LEVEL_MILLI: std::ops::RangeInclusive<i64> = 0..=2_000;
/// Sound level in thousandths of the preset level.
const PRESET_SOUND_LEVEL_MILLI: std::ops::RangeInclusive<i64> = 0..=1_000;

/// Every preset with its sounds in playback order, sorted by name.
pub async fn presets(pool: &SqlitePool) -> MusicLibraryResult<Vec<MusicSoundscapePreset>> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64, i64)>(
        "SELECT id, name, icon, level_milli, created_at_ms, updated_at_ms, version
         FROM music_soundscape_presets ORDER BY lower(name), id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape presets", error))?;
    let mut sounds = HashMap::<String, Vec<MusicSoundscapePresetSound>>::new();
    for (preset_id, soundscape_id, level_milli) in sqlx::query_as::<_, (String, String, i64)>(
        "SELECT preset_id, soundscape_id, level_milli FROM music_soundscape_preset_sounds
         ORDER BY preset_id, order_key, soundscape_id",
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
                level_milli,
            });
    }
    Ok(rows
        .into_iter()
        .map(|row| MusicSoundscapePreset {
            sounds: sounds.remove(&row.0).unwrap_or_default(),
            id: row.0,
            name: row.1,
            icon: row.2,
            level_milli: row.3,
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
    let existing =
        sqlx::query_scalar::<_, i64>("SELECT version FROM music_soundscape_presets WHERE id = ?")
            .bind(&request.id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| {
                MusicLibraryError::database("load soundscape preset version", error)
            })?;
    if let Some(version) = existing {
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
    let name = request.name.trim().to_owned();
    let changed = if existing.is_some() {
        let fields_changed = sqlx::query(
            "UPDATE music_soundscape_presets
             SET name = ?, icon = ?, level_milli = ?
             WHERE id = ? AND (name, icon, level_milli) IS NOT (?, ?, ?)",
        )
        .bind(&name)
        .bind(&request.icon)
        .bind(request.level_milli)
        .bind(&request.id)
        .bind(&name)
        .bind(&request.icon)
        .bind(request.level_milli)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset", error))?
        .rows_affected()
            > 0;
        let sounds_changed =
            replace_preset_sounds(&mut transaction, &request.id, &request.sounds).await?;
        fields_changed || sounds_changed
    } else {
        sqlx::query(
            "INSERT INTO music_soundscape_presets (id, name, icon, level_milli, created_at_ms, updated_at_ms, version)
             VALUES (?, ?, ?, ?, ?, ?, 1)",
        )
        .bind(&request.id)
        .bind(&name)
        .bind(&request.icon)
        .bind(request.level_milli)
        .bind(request.updated_at_ms)
        .bind(request.updated_at_ms)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset", error))?;
        replace_preset_sounds(&mut transaction, &request.id, &request.sounds).await?;
        false
    };
    if changed {
        sqlx::query(
            "UPDATE music_soundscape_presets
             SET updated_at_ms = ?, version = version + 1
             WHERE id = ?",
        )
        .bind(request.updated_at_ms)
        .bind(&request.id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| MusicLibraryError::database("advance soundscape preset version", error))?;
    }
    super::writes::commit(transaction, "commit soundscape preset update").await?;
    preset(pool, &request.id).await
}

/// Diffs a preset's ordered sounds against the stored rows and returns whether any row changed.
///
/// Rows are matched by sound, so a kept sound keeps its row. Added sounds append after the stored keys,
/// then only the sounds whose relative order changed receive new keys.
async fn replace_preset_sounds(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    preset_id: &str,
    sounds: &[MusicSoundscapePresetSound],
) -> MusicLibraryResult<bool> {
    let stored = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT soundscape_id, order_key, level_milli FROM music_soundscape_preset_sounds
         WHERE preset_id = ? ORDER BY order_key, soundscape_id",
    )
    .bind(preset_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load soundscape preset sounds", error))?;
    let wanted = sounds
        .iter()
        .map(|sound| sound.soundscape_id.as_str())
        .collect::<HashSet<_>>();
    let stored_levels = stored
        .iter()
        .map(|(soundscape_id, _, level_milli)| (soundscape_id.as_str(), *level_milli))
        .collect::<HashMap<_, _>>();
    let mut changed = false;
    for (soundscape_id, _, _) in stored
        .iter()
        .filter(|(soundscape_id, _, _)| !wanted.contains(soundscape_id.as_str()))
    {
        sqlx::query(
            "DELETE FROM music_soundscape_preset_sounds WHERE preset_id = ? AND soundscape_id = ?",
        )
        .bind(preset_id)
        .bind(soundscape_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("remove soundscape preset sound", error))?;
        changed = true;
    }
    let mut current = stored
        .iter()
        .filter(|(soundscape_id, _, _)| wanted.contains(soundscape_id.as_str()))
        .map(|(soundscape_id, order_key, _)| (soundscape_id.clone(), order_key.clone()))
        .collect::<Vec<_>>();
    let added = sounds
        .iter()
        .filter(|sound| !stored_levels.contains_key(sound.soundscape_id.as_str()))
        .collect::<Vec<_>>();
    let tail = stored.last().map(|(_, order_key, _)| order_key.as_str());
    for (sound, key) in added
        .iter()
        .zip(super::order::keys_after(tail, added.len())?)
    {
        sqlx::query(
            "INSERT INTO music_soundscape_preset_sounds (preset_id, soundscape_id, order_key, level_milli)
             VALUES (?, ?, ?, ?)",
        )
        .bind(preset_id)
        .bind(&sound.soundscape_id)
        .bind(key.as_str())
        .bind(sound.level_milli)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset sound", error))?;
        current.push((sound.soundscape_id.clone(), key.as_str().to_owned()));
        changed = true;
    }
    let desired = sounds
        .iter()
        .map(|sound| sound.soundscape_id.as_str())
        .collect::<Vec<_>>();
    for (soundscape_id, key) in super::order::reorder(&current, &desired)? {
        sqlx::query(
            "UPDATE music_soundscape_preset_sounds SET order_key = ?
             WHERE preset_id = ? AND soundscape_id = ?",
        )
        .bind(key.as_str())
        .bind(preset_id)
        .bind(&soundscape_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("move soundscape preset sound", error))?;
        changed = true;
    }
    for sound in sounds {
        if stored_levels
            .get(sound.soundscape_id.as_str())
            .is_none_or(|level_milli| *level_milli == sound.level_milli)
        {
            continue;
        }
        sqlx::query(
            "UPDATE music_soundscape_preset_sounds SET level_milli = ?
             WHERE preset_id = ? AND soundscape_id = ?",
        )
        .bind(sound.level_milli)
        .bind(preset_id)
        .bind(&sound.soundscape_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("save soundscape preset sound", error))?;
        changed = true;
    }
    Ok(changed)
}

/// Saves only a preset's level, the setting the soundscape controls adjust while it plays.
///
/// The latest level wins without a version check, so a slider never conflicts with itself; a changed level
/// still advances the version so a concurrent full edit notices it, and an unchanged level writes nothing.
pub async fn set_level(
    pool: &SqlitePool,
    preset_id: &str,
    level_milli: i64,
    updated_at_ms: i64,
) -> MusicLibraryResult<MusicSoundscapePreset> {
    validate_id(preset_id, "presetId")?;
    validate_preset_level(level_milli)?;
    if updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    sqlx::query(
        "UPDATE music_soundscape_presets
         SET level_milli = ?, updated_at_ms = MAX(updated_at_ms + 1, ?), version = version + 1
         WHERE id = ? AND level_milli IS NOT ?",
    )
    .bind(level_milli)
    .bind(updated_at_ms)
    .bind(preset_id)
    .bind(level_milli)
    .execute(pool)
    .await
    .map_err(|error| MusicLibraryError::database("save soundscape preset level", error))?;
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

fn validate_preset_level(level_milli: i64) -> MusicLibraryResult<()> {
    if !PRESET_LEVEL_MILLI.contains(&level_milli) {
        return Err(MusicLibraryError::validation(
            "levelMilli",
            format!(
                "must be between {} and {}",
                PRESET_LEVEL_MILLI.start(),
                PRESET_LEVEL_MILLI.end()
            ),
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
    validate_preset_level(request.level_milli)?;
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
        if !PRESET_SOUND_LEVEL_MILLI.contains(&sound.level_milli) {
            return Err(MusicLibraryError::validation(
                "sounds",
                format!(
                    "levels must be between {} and {}",
                    PRESET_SOUND_LEVEL_MILLI.start(),
                    PRESET_SOUND_LEVEL_MILLI.end()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::pool;

    fn sounds(entries: &[(&str, i64)]) -> Vec<MusicSoundscapePresetSound> {
        entries
            .iter()
            .map(|(id, level)| MusicSoundscapePresetSound {
                soundscape_id: (*id).to_owned(),
                level_milli: *level,
            })
            .collect()
    }

    fn write(
        id: &str,
        entries: &[(&str, i64)],
        expected_version: Option<i64>,
    ) -> MusicSoundscapePresetWrite {
        MusicSoundscapePresetWrite {
            id: id.into(),
            name: " Rainy desk ".into(),
            icon: "lucide:cloud-rain".into(),
            level_milli: 500,
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
            assert!(seeded.iter().all(|preset| preset.sounds.len() == 1
                && preset.sounds[0].level_milli == 1000
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
                        ("generated-brown-noise", 250),
                        ("generated-white-noise", 1000),
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
                upsert(
                    &pool,
                    write("desk", &[("generated-pink-noise", 1000)], None)
                )
                .await
                .is_err()
            );
            let updated = upsert(
                &pool,
                write(
                    "desk",
                    &[("generated-white-noise", 500), ("generated-pink-noise", 0)],
                    Some(1),
                ),
            )
            .await
            .unwrap();
            assert_eq!(updated.version, 2);
            assert_eq!(
                preset(&pool, "desk").await.unwrap().sounds,
                sounds(&[("generated-white-noise", 500), ("generated-pink-noise", 0)])
            );
            assert!(remove(&pool, "desk", 1).await.is_err());
            remove(&pool, "desk", 2).await.unwrap();
            assert!(custom_presets(&pool).await.is_empty());
        });
    }

    #[test]
    fn unchanged_presets_and_levels_write_nothing_and_reorders_keep_one_row_per_sound() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let entries = [
                ("generated-brown-noise", 250),
                ("generated-white-noise", 1000),
                ("generated-pink-noise", 500),
            ];
            let created = upsert(&pool, write("desk", &entries, None)).await.unwrap();
            let mut again = write("desk", &entries, Some(1));
            again.updated_at_ms += 100;
            assert_eq!(upsert(&pool, again).await.unwrap(), created);
            assert_eq!(
                set_level(&pool, "desk", created.level_milli, 1_700_000_000_500)
                    .await
                    .unwrap(),
                created
            );

            let reordered = [
                ("generated-pink-noise", 500),
                ("generated-brown-noise", 250),
            ];
            let updated = upsert(&pool, write("desk", &reordered, Some(1)))
                .await
                .unwrap();
            assert_eq!(updated.version, 2);
            assert_eq!(updated.sounds, sounds(&reordered));
        });
    }

    #[test]
    fn moving_one_sound_rekeys_only_that_sound() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let entries = [
                ("generated-brown-noise", 250),
                ("generated-white-noise", 1000),
                ("generated-pink-noise", 500),
            ];
            upsert(&pool, write("desk", &entries, None)).await.unwrap();
            let keys = |pool: SqlitePool| async move {
                sqlx::query_as::<_, (String, String)>(
                    "SELECT soundscape_id, order_key FROM music_soundscape_preset_sounds
                     WHERE preset_id = 'desk'",
                )
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .collect::<HashMap<_, _>>()
            };
            let before = keys(pool.clone()).await;
            let moved = [entries[2], entries[0], entries[1]];
            let updated = upsert(&pool, write("desk", &moved, Some(1))).await.unwrap();
            assert_eq!(updated.sounds, sounds(&moved));
            let after = keys(pool.clone()).await;
            assert_eq!(
                after["generated-brown-noise"],
                before["generated-brown-noise"]
            );
            assert_eq!(
                after["generated-white-noise"],
                before["generated-white-noise"]
            );
            assert_ne!(
                after["generated-pink-noise"],
                before["generated-pink-noise"]
            );
        });
    }

    #[test]
    fn preset_levels_save_alone_and_stay_in_range() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(&pool, write("desk", &[("generated-pink-noise", 800)], None))
                .await
                .unwrap();
            let louder = set_level(&pool, "desk", 1500, 1_700_000_000_001)
                .await
                .unwrap();
            assert_eq!(louder.level_milli, 1500);
            assert_eq!(louder.version, created.version + 1);
            assert_eq!(louder.sounds, created.sounds);
            for level in [-1, 2_001] {
                assert!(
                    set_level(&pool, "desk", level, 1_700_000_000_002)
                        .await
                        .is_err()
                );
                let mut invalid = write("other", &[("generated-pink-noise", 1000)], None);
                invalid.level_milli = level;
                assert!(upsert(&pool, invalid).await.is_err());
            }
            assert!(
                set_level(&pool, "removed", 1000, 1_700_000_000_002)
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
                            ("generated-brown-noise", 1000),
                            ("generated-brown-noise", 500)
                        ],
                        None
                    )
                )
                .await
                .is_err()
            );
            assert!(
                upsert(&pool, write("unknown", &[("removed-sound", 1000)], None))
                    .await
                    .is_err()
            );
            for level in [-1, 1_001] {
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
            let created = upsert(
                &pool,
                write("desk", &[("generated-pink-noise", 1000)], None),
            )
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
                    &[("local-storm", 1000), ("generated-pink-noise", 400)],
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
                sounds(&[("generated-pink-noise", 400)])
            );
        });
    }
}
