use super::*;
use sqlx::SqlitePool;

pub async fn groups(pool: &SqlitePool) -> MusicLibraryResult<Vec<MusicSoundscapeGroup>> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64)>(
        "SELECT id, name, icon, created_at_ms, updated_at_ms, version
         FROM music_soundscape_groups ORDER BY lower(name), id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load sound groups", error))?;
    Ok(rows
        .into_iter()
        .map(|row| MusicSoundscapeGroup {
            id: row.0,
            name: row.1,
            icon: row.2,
            created_at_ms: row.3,
            updated_at_ms: row.4,
            version: row.5,
        })
        .collect())
}

pub async fn upsert(
    pool: &SqlitePool,
    request: MusicSoundscapeGroupWrite,
) -> MusicLibraryResult<MusicSoundscapeGroup> {
    validate_id(&request.id, "id")?;
    if request.name.trim().is_empty() || request.name.chars().count() > 80 {
        return Err(MusicLibraryError::validation(
            "name",
            "must contain between one and 80 characters",
        ));
    }
    super::validate_icon(&request.icon)?;
    if request.updated_at_ms <= 0 {
        return Err(MusicLibraryError::validation(
            "updatedAt",
            "must be positive",
        ));
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin sound group update", error))?;
    let existing = sqlx::query_as::<_, (i64, i64)>(
        "SELECT created_at_ms, version FROM music_soundscape_groups WHERE id = ?",
    )
    .bind(&request.id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("load sound group version", error))?;
    if let Some((_, version)) = existing {
        if request.expected_version != Some(version) {
            return Err(MusicLibraryError::conflict(
                "the sound group changed before this update",
            ));
        }
    } else if request.expected_version.is_some() {
        return Err(MusicLibraryError::not_found("sound group", &request.id));
    }
    let created_at_ms = existing.map(|row| row.0).unwrap_or(request.updated_at_ms);
    let version = existing.map(|row| row.1 + 1).unwrap_or(1);
    sqlx::query(
        "INSERT INTO music_soundscape_groups (id, name, icon, created_at_ms, updated_at_ms, version)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon,
           updated_at_ms = excluded.updated_at_ms, version = excluded.version",
    )
    .bind(&request.id)
    .bind(request.name.trim())
    .bind(&request.icon)
    .bind(created_at_ms)
    .bind(request.updated_at_ms)
    .bind(version)
    .execute(&mut *transaction)
    .await
    .map_err(|error| MusicLibraryError::database("save sound group", error))?;
    super::writes::commit(transaction, "commit sound group update").await?;
    Ok(MusicSoundscapeGroup {
        id: request.id,
        name: request.name.trim().to_owned(),
        icon: request.icon,
        created_at_ms,
        updated_at_ms: request.updated_at_ms,
        version,
    })
}

pub async fn remove(
    pool: &SqlitePool,
    group_id: &str,
    expected_version: i64,
) -> MusicLibraryResult<()> {
    validate_id(group_id, "groupId")?;
    let result = sqlx::query("DELETE FROM music_soundscape_groups WHERE id = ? AND version = ?")
        .bind(group_id)
        .bind(expected_version)
        .execute(pool)
        .await
        .map_err(|error| MusicLibraryError::database("remove sound group", error))?;
    if result.rows_affected() == 0 {
        return Err(MusicLibraryError::conflict(
            "the sound group changed before removal",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::pool;

    #[test]
    fn groups_are_versioned_and_removal_ungroups_sounds() {
        crate::test_support::block_on(async {
            let pool = pool().await;
            let created = upsert(
                &pool,
                MusicSoundscapeGroupWrite {
                    id: "rain-group".into(),
                    name: "Rain".into(),
                    icon: "lucide:cloud-rain".into(),
                    expected_version: None,
                    updated_at_ms: 1_700_000_000_000,
                },
            )
            .await
            .unwrap();
            assert_eq!(created.version, 1);
            assert_eq!(groups(&pool).await.unwrap(), vec![created.clone()]);
            assert!(
                upsert(
                    &pool,
                    MusicSoundscapeGroupWrite {
                        id: created.id.clone(),
                        name: "Storm".into(),
                        icon: "lucide:wind".into(),
                        expected_version: None,
                        updated_at_ms: 1_700_000_000_001,
                    }
                )
                .await
                .is_err()
            );
            sqlx::query("INSERT INTO music_soundscapes (id, source_kind, name, availability, created_at_ms, updated_at_ms, version, group_id) VALUES ('local-storm', 'local-loop', 'Storm', 'missing', 1, 1, 1, ?)")
                .bind(&created.id).execute(&pool).await.unwrap();
            remove(&pool, &created.id, created.version).await.unwrap();
            let group_id: Option<String> = sqlx::query_scalar(
                "SELECT group_id FROM music_soundscapes WHERE id = 'local-storm'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(group_id, None);
        });
    }
}
