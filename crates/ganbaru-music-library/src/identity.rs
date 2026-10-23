//! Content-derived keys for Music rows whose identity follows from their content.
//!
//! Two devices that discover the same file, follow the same YouTube playlist, or add the same item
//! to the same playlist compute the same key, so concurrent creation converges on one row. Roots,
//! playlists, presets, user soundscapes, and snoozes are user decisions and keep random keys.
//! Rows created before these rules keep their stored keys; writers match by identity key first.
//! Tables with composite primary keys carry a derived `sync_key` from
//! [`ganbaru_music::sync_keys`], and [`rekey_pending`] converts their migration placeholders.

use crate::{MusicLibraryError, MusicLibraryResult};
use ganbaru_music::sync_keys::{self, CompositeKeyTable};
use ganbaru_sync_contracts::PENDING_KEY_PREFIX;
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, Connection, Row, Sqlite, SqliteConnection, Transaction};

/// Returns `music-{kind}-{hex}`, where `hex` is the SHA-256 of `kind` followed by a zero byte and
/// each value in order.
pub fn stable_id(kind: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    format!("music-{kind}-{:x}", hasher.finalize())
}

/// Returns a root-relative media path with forward slashes, the stored and keyed form.
pub fn normalize_relative_path(relative_path: &str) -> String {
    relative_path.replace('\\', "/")
}

/// Key of a local media item first discovered at `relative_path` under `root_id`.
pub fn local_item_id(root_id: &str, relative_path: &str) -> String {
    stable_id("item", &[root_id, &normalize_relative_path(relative_path)])
}

/// Key of an imported item that is not a YouTube video, derived from its portable identity key.
pub fn import_item_id(identity_key: &str) -> String {
    stable_id("import-item", &[identity_key])
}

/// Key of the library item for one YouTube video.
pub fn youtube_item_id(video_id: &str) -> String {
    format!("music-youtube-{video_id}")
}

/// Key of the location of a media file at `relative_path` under `root_id`.
pub fn location_id(root_id: &str, relative_path: &str) -> String {
    stable_id(
        "location",
        &[root_id, &normalize_relative_path(relative_path)],
    )
}

/// Identity key of the source collection that mirrors a local root.
pub fn local_root_identity_key(root_id: &str) -> String {
    format!("local-root:{root_id}")
}

/// Identity key of the source collection that mirrors a YouTube playlist.
pub fn youtube_playlist_identity_key(playlist_id: &str) -> String {
    format!("youtube-playlist:{playlist_id}")
}

/// Key of a source collection with the given identity key.
pub fn collection_id(identity_key: &str) -> String {
    stable_id("collection", &[identity_key])
}

/// Key of the membership of `item_id` in `playlist_id`, unique per pair.
pub fn membership_id(playlist_id: &str, item_id: &str) -> String {
    stable_id("membership", &[playlist_id, item_id])
}

/// Replaces every `pending:` placeholder `sync_key` of the Music tables with composite primary
/// keys by its derived key, the value writers compute for the same row. Returns how many rows
/// it rekeyed.
///
/// The capture migration fills existing rows with placeholders because SQL cannot compute the
/// key. This pass runs when a read-write vault opens, after migrations and before any writer or
/// sync start, so no capture or operation names a placeholder. Each rekey captures the row as
/// created under its final key, and a second run changes nothing.
pub async fn rekey_pending(connection: &mut SqliteConnection) -> MusicLibraryResult<u64> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music key conversion", error))?;
    let mut rekeyed = 0;
    for table in sync_keys::COMPOSITE_KEY_TABLES {
        rekeyed += rekey_table(&mut transaction, &table).await?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| MusicLibraryError::database("commit music key conversion", error))?;
    Ok(rekeyed)
}

async fn rekey_table(
    transaction: &mut Transaction<'_, Sqlite>,
    table: &CompositeKeyTable,
) -> MusicLibraryResult<u64> {
    let select = format!(
        "SELECT rowid, {columns} FROM {name} WHERE {key} GLOB ?",
        columns = table.columns.join(", "),
        name = table.table,
        key = sync_keys::SYNC_KEY_COLUMN,
    );
    let rows = sqlx::query(AssertSqlSafe(select))
        .bind(format!("{PENDING_KEY_PREFIX}*"))
        .fetch_all(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("read pending music row keys", error))?;
    let update = format!(
        "UPDATE {name} SET {key} = ? WHERE rowid = ?",
        name = table.table,
        key = sync_keys::SYNC_KEY_COLUMN,
    );
    for row in &rows {
        let rowid: i64 = row
            .try_get(0)
            .map_err(|error| MusicLibraryError::database("read pending music row id", error))?;
        let parts = (1..=table.columns.len())
            .map(|index| row.try_get::<String, _>(index))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| MusicLibraryError::database("read pending music row key", error))?;
        let parts = parts.iter().map(String::as_str).collect::<Vec<_>>();
        sqlx::query(AssertSqlSafe(update.as_str()))
            .bind(table.key(&parts)?)
            .bind(rowid)
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("rekey music row", error))?;
    }
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_stable_id_hashes_kind_then_zero_separated_values() {
        assert_eq!(
            stable_id("item", &["root-1", "a/b.mp3"]),
            "music-item-0bc7749f6ec41f483604e58a2f6ed2a6973325d6ffc88e6abd80f492f4413af5"
        );
        assert_ne!(
            stable_id("item", &["root-1a", "/b.mp3"]),
            stable_id("item", &["root-1", "a/b.mp3"]),
            "the separator keeps value boundaries distinct"
        );
    }

    #[test]
    fn identity_local_keys_reproduce_scanned_ids_and_normalize_separators() {
        assert_eq!(
            local_item_id("root-1", "a/b.mp3"),
            "music-item-0bc7749f6ec41f483604e58a2f6ed2a6973325d6ffc88e6abd80f492f4413af5"
        );
        assert_eq!(
            local_item_id("root-1", "a\\b.mp3"),
            local_item_id("root-1", "a/b.mp3")
        );
        assert_eq!(
            location_id("root-1", "a\\b.mp3"),
            "music-location-b607cb434e270354620db1cb7a2da220d0f9debeeb2025977343157f07a22330"
        );
        assert_ne!(
            local_item_id("root-1", "a/b.mp3"),
            local_item_id("root-2", "a/b.mp3")
        );
    }

    #[test]
    fn identity_collection_membership_and_import_keys_match_the_formula() {
        assert_eq!(local_root_identity_key("root-1"), "local-root:root-1");
        assert_eq!(
            youtube_playlist_identity_key("PL123"),
            "youtube-playlist:PL123"
        );
        assert_eq!(
            collection_id(&local_root_identity_key("root-1")),
            "music-collection-b340de7f2cec6f37f126f7933ef29463ff3bb4d1de69ae49f84adc8e044cbb2c"
        );
        assert_eq!(
            membership_id("playlist-1", "item-1"),
            "music-membership-68a9cb5e741a46357c216fbd5f6781003c432cb851c3712fd66265868b6939bf"
        );
        assert_ne!(
            membership_id("playlist-1", "item-1"),
            membership_id("item-1", "playlist-1")
        );
        assert_eq!(
            import_item_id("local-file:sha256:abc"),
            "music-import-item-4ca95b8c1fb675c8ba7cc15d3accf03f303d8127c419590992ca97aad4b01ff3"
        );
        assert_eq!(youtube_item_id("dQw4w9WgXcQ"), "music-youtube-dQw4w9WgXcQ");
    }

    #[test]
    fn identity_derived_keys_fit_the_id_limit() {
        let longest = [
            local_item_id("r", "p"),
            location_id("r", "p"),
            collection_id("c"),
            membership_id("p", "i"),
            import_item_id("k"),
        ]
        .into_iter()
        .map(|id| id.len())
        .max()
        .unwrap_or_default();
        assert!(longest <= crate::validation::MAX_ID_BYTES);
    }

    /// Seeds every composite key table: the dense fixture covers signals, statistics, source
    /// provenance, and intended uses, the migration seeds preset sounds, and one project default
    /// adds context assignments.
    async fn seeded_pool() -> sqlx::SqlitePool {
        use ganbaru_music::assignments::{
            MusicActivityPhase, MusicAssignmentBehavior, MusicAssignmentOwnerKind,
            MusicAssignmentProvenanceKind, MusicContextAssignmentDraft, MusicSoundscapeBehavior,
            replace_assignments_in_transaction,
        };
        let pool = crate::tests::pool().await;
        crate::fixtures::seed_dense_music_fixture(&pool)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        replace_assignments_in_transaction(
            &mut transaction,
            MusicAssignmentOwnerKind::ProjectDefault,
            "benchmark-music-project",
            vec![MusicContextAssignmentDraft {
                phase: MusicActivityPhase::Focus,
                behavior: MusicAssignmentBehavior::PauseMusic,
                playlist_id: None,
                soundscape_preset_id: None,
                soundscape_behavior: MusicSoundscapeBehavior::Inherit,
                provenance_kind: MusicAssignmentProvenanceKind::Explicit,
                provenance_id: None,
            }],
            1_700_000_000_000,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        pool
    }

    /// Returns each row's stored key and the key derived from its composite columns.
    async fn stored_and_derived(
        pool: &sqlx::SqlitePool,
        table: &CompositeKeyTable,
    ) -> Vec<(String, String)> {
        let select = format!(
            "SELECT {key}, {columns} FROM {name} ORDER BY rowid",
            key = sync_keys::SYNC_KEY_COLUMN,
            columns = table.columns.join(", "),
            name = table.table,
        );
        let rows = sqlx::query(AssertSqlSafe(select))
            .fetch_all(pool)
            .await
            .unwrap();
        rows.iter()
            .map(|row| {
                let parts = (1..=table.columns.len())
                    .map(|index| row.get::<String, _>(index))
                    .collect::<Vec<_>>();
                let parts = parts.iter().map(String::as_str).collect::<Vec<_>>();
                (row.get(0), table.key(&parts).unwrap())
            })
            .collect()
    }

    async fn capture_count(pool: &sqlx::SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_capture")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[test]
    fn identity_writers_store_the_derived_key_of_every_composite_row() {
        crate::test_support::block_on(async {
            let pool = seeded_pool().await;
            for table in sync_keys::COMPOSITE_KEY_TABLES {
                let rows = stored_and_derived(&pool, &table).await;
                assert!(!rows.is_empty(), "{} has seeded rows", table.table);
                for (stored, derived) in rows {
                    assert_eq!(stored, derived, "{} key", table.table);
                }
            }
        });
    }

    #[test]
    fn identity_rekey_pending_restores_writer_keys_and_is_idempotent() {
        crate::test_support::block_on(async {
            let pool = seeded_pool().await;
            let mut expected = Vec::new();
            let mut pending_rows = 0_u64;
            for table in sync_keys::COMPOSITE_KEY_TABLES {
                let rows = stored_and_derived(&pool, &table).await;
                pending_rows += rows.len() as u64;
                expected.push(rows);
                // Stored keys never change back to placeholders, so the rows are recreated with
                // the placeholders the capture migration writes.
                let name = table.table;
                let key = sync_keys::SYNC_KEY_COLUMN;
                let statements = [
                    format!("CREATE TEMP TABLE staged AS SELECT * FROM {name}"),
                    format!("UPDATE staged SET {key} = '{PENDING_KEY_PREFIX}' || rowid"),
                    format!("DELETE FROM {name}"),
                    format!("INSERT INTO {name} SELECT * FROM staged"),
                    "DROP TABLE staged".to_string(),
                ];
                let statements = statements.iter().map(String::as_str).collect::<Vec<_>>();
                crate::test_support::execute_as_sync_apply(&pool, &statements).await;
            }

            let mut connection = pool.acquire().await.unwrap();
            assert_eq!(rekey_pending(&mut connection).await.unwrap(), pending_rows);
            drop(connection);
            for (table, rows) in sync_keys::COMPOSITE_KEY_TABLES.iter().zip(expected) {
                assert_eq!(stored_and_derived(&pool, table).await, rows);
            }

            let captures = capture_count(&pool).await;
            let mut connection = pool.acquire().await.unwrap();
            assert_eq!(rekey_pending(&mut connection).await.unwrap(), 0);
            drop(connection);
            assert_eq!(capture_count(&pool).await, captures);
        });
    }
}
