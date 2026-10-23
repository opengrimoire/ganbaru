//! Content-derived keys for Music rows whose identity follows from their content.
//!
//! Two devices that discover the same file, follow the same YouTube playlist, or add the same item
//! to the same playlist compute the same key, so concurrent creation converges on one row. Roots,
//! playlists, presets, user soundscapes, and snoozes are user decisions and keep random keys.
//! Rows created before these rules keep their stored keys and writers match by identity key
//! first, except playlist memberships, whose key the sync manifest checks. Tables with composite
//! primary keys carry a derived `sync_key` from [`ganbaru_music::sync_keys`], and
//! [`rekey_pending`] converts their migration placeholders and older membership keys.

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

/// A Music table whose rows take content-derived keys without a sync existence register, so a
/// published deletion retires the key: sync refuses to create a row under it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetirableTable {
    /// `music_library_items`.
    Items,
    /// `music_local_locations`.
    Locations,
    /// `music_source_collections`.
    Collections,
}

impl RetirableTable {
    fn spec(self) -> &'static ganbaru_sync::manifest::TableSpec {
        use ganbaru_sync::manifest::vault::music;
        match self {
            Self::Items => &music::ITEMS,
            Self::Locations => &music::LOCATIONS,
            Self::Collections => &music::COLLECTIONS,
        }
    }
}

/// Successor generations tried before a create is refused. Each one needs a published create
/// and deletion of the previous key, so reaching the bound means a writer is looping.
const MAX_SUCCESSOR_GENERATION: u32 = 1_000;

/// Returns the key a writer creates a row of `table` under when its content derives `base`:
/// `base` itself unless a published deletion retired it, and otherwise the first successor
/// `{base}-{generation}` from generation 2 that is not retired.
///
/// A key is retired when the sync row registry knows it and the domain table holds no row under
/// it, the condition of the recreate guard trigger, and no held deletion of it awaits Undo.
/// Successors are deterministic, so devices that saw the same deletions re-add the same content
/// under the same key and concurrent re-adds converge on one row; the table's unique identity
/// columns settle devices that disagree.
pub async fn creatable_key(
    connection: &mut SqliteConnection,
    table: RetirableTable,
    base: String,
) -> MusicLibraryResult<String> {
    let spec = table.spec();
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM sync_rows WHERE table_id = ?1 AND row_key = ?2)
            AND NOT EXISTS (SELECT 1 FROM {name} WHERE {key} = ?2)
            AND NOT EXISTS (
                SELECT 1 FROM sync_capture
                WHERE table_id = ?1 AND row_key = ?2 AND held_until_ms IS NOT NULL
            )",
        name = spec.name,
        key = spec.key_column,
    );
    let mut candidate = base.clone();
    for generation in 2..=MAX_SUCCESSOR_GENERATION + 1 {
        let retired: bool = sqlx::query_scalar(AssertSqlSafe(sql.as_str()))
            .bind(i64::from(spec.id.0))
            .bind(&candidate)
            .fetch_one(&mut *connection)
            .await
            .map_err(|error| MusicLibraryError::database("check retired music key", error))?;
        if !retired {
            return Ok(candidate);
        }
        candidate = format!("{base}-{generation}");
        if candidate.len() > crate::validation::MAX_ID_BYTES {
            return Err(MusicLibraryError::conflict(format!(
                "{} key {base} is retired and has no successor within the id limit",
                spec.name
            )));
        }
    }
    Err(MusicLibraryError::conflict(format!(
        "{} key {base} was deleted and recreated too many times",
        spec.name
    )))
}

/// Key of the membership of `item_id` in `playlist_id`, unique per pair. It is the derived row
/// key the sync manifest checks, so it fails only for a part holding a NUL byte.
pub fn membership_id(playlist_id: &str, item_id: &str) -> MusicLibraryResult<String> {
    sync_keys::membership_key(playlist_id, item_id)
}

/// Replaces every `pending:` placeholder `sync_key` of the Music tables with composite primary
/// keys by its derived key, the value writers compute for the same row, and moves every playlist
/// membership whose `id` predates derived keys to its derived key. Returns how many rows it
/// rekeyed.
///
/// The capture migration fills existing rows with placeholders because SQL cannot compute the
/// key. This pass runs when a read-write vault opens, after migrations and before any writer or
/// sync start, so no capture or operation names a placeholder or an older membership key. Each
/// rekey captures the row as created under its final key, and a second run changes nothing.
pub async fn rekey_pending(connection: &mut SqliteConnection) -> MusicLibraryResult<u64> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(|error| MusicLibraryError::database("begin music key conversion", error))?;
    let mut rekeyed = 0;
    for table in sync_keys::COMPOSITE_KEY_TABLES {
        rekeyed += rekey_table(&mut transaction, &table).await?;
    }
    rekeyed += rekey_memberships(&mut transaction).await?;
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

/// Moves each playlist membership whose `id` is not the derived key of its playlist and item to
/// that key, with its skip ranges. The immutable key trigger refuses a changed `id`, so the rows
/// are copied aside, deleted, and inserted again under their derived keys.
///
/// Older keys exist only in vaults written before Music replicated, whose memberships have never
/// been captured or published, so their removal runs with the apply flag set and leaves no
/// capture, while the insert captures each row as created. Session checkpoints name memberships
/// inside their payload, so a pass that moves any membership drops them, losing at most one resume
/// position.
///
/// Invariant: every membership writer stores [`membership_id`] of its playlist and item. Local
/// writers derive it, the immutable key trigger refuses a changed `id`, `playlist_id`, or
/// `item_id` outside sync apply, and the manifest derives applied rows' keys from the same two
/// columns. So after the first pass of an older vault this finds nothing, and an older key the
/// sync row registry already knows is a writer bug, which debug builds assert, because its
/// deletion here would never reach other devices.
async fn rekey_memberships(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<u64> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT id, playlist_id, item_id FROM music_playlist_memberships")
            .fetch_all(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("read music membership keys", error))?;
    let mut moved = Vec::new();
    for (id, playlist_id, item_id) in rows {
        let derived = membership_id(&playlist_id, &item_id)?;
        if id != derived {
            moved.push((id, derived));
        }
    }
    if moved.is_empty() {
        return Ok(0);
    }
    #[cfg(debug_assertions)]
    for (old, _) in &moved {
        let registered: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sync_rows WHERE table_id = ? AND row_key = ?)",
        )
        .bind(i64::from(
            ganbaru_sync::manifest::vault::music::MEMBERSHIPS.id.0,
        ))
        .bind(old)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| MusicLibraryError::database("check music membership key", error))?;
        debug_assert!(
            !registered,
            "membership {old} is replicated under a key other than its derived key"
        );
    }
    run_rekey_statements(
        transaction,
        &[
            "CREATE TEMP TABLE music_membership_rekey_rows AS
                SELECT * FROM music_playlist_memberships WHERE 0",
            "CREATE TEMP TABLE music_membership_rekey_skips AS
                SELECT * FROM music_membership_skip_ranges WHERE 0",
        ],
        &[],
    )
    .await?;
    for (old, new) in &moved {
        run_rekey_statements(
            transaction,
            &[
                "INSERT INTO temp.music_membership_rekey_rows
                    SELECT * FROM music_playlist_memberships WHERE id = ?1",
                "UPDATE temp.music_membership_rekey_rows SET id = ?2 WHERE id = ?1",
                "INSERT INTO temp.music_membership_rekey_skips
                    SELECT * FROM music_membership_skip_ranges WHERE membership_id = ?1",
                "UPDATE temp.music_membership_rekey_skips SET membership_id = ?2
                    WHERE membership_id = ?1",
            ],
            &[old, new],
        )
        .await?;
    }
    run_rekey_statements(
        transaction,
        &["UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1"],
        &[],
    )
    .await?;
    for (old, _) in &moved {
        run_rekey_statements(
            transaction,
            &["DELETE FROM music_playlist_memberships WHERE id = ?1"],
            &[old],
        )
        .await?;
    }
    run_rekey_statements(
        transaction,
        &[
            "UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1",
            "INSERT INTO music_playlist_memberships SELECT * FROM temp.music_membership_rekey_rows",
            "INSERT INTO music_membership_skip_ranges
                SELECT * FROM temp.music_membership_rekey_skips",
            "DROP TABLE temp.music_membership_rekey_rows",
            "DROP TABLE temp.music_membership_rekey_skips",
            "DELETE FROM music_session_checkpoints",
        ],
        &[],
    )
    .await?;
    Ok(moved.len() as u64)
}

/// Runs fixed membership rekey statements in order, binding `parameters` to each.
async fn run_rekey_statements(
    transaction: &mut Transaction<'_, Sqlite>,
    statements: &[&'static str],
    parameters: &[&String],
) -> MusicLibraryResult<()> {
    for statement in statements {
        let mut query = sqlx::query(*statement);
        for parameter in parameters {
            query = query.bind(parameter.as_str());
        }
        query
            .execute(&mut **transaction)
            .await
            .map_err(|error| MusicLibraryError::database("rekey music memberships", error))?;
    }
    Ok(())
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
            membership_id("playlist-1", "item-1").unwrap(),
            ganbaru_sync_contracts::derived_key("music-membership", &["playlist-1", "item-1"])
                .unwrap()
                .into_string()
        );
        assert_ne!(
            membership_id("playlist-1", "item-1").unwrap(),
            membership_id("item-1", "playlist-1").unwrap()
        );
        assert!(membership_id("playlist\0", "item-1").is_err());
        assert_eq!(
            import_item_id("local-file:sha256:abc"),
            "music-import-item-4ca95b8c1fb675c8ba7cc15d3accf03f303d8127c419590992ca97aad4b01ff3"
        );
        assert_eq!(youtube_item_id("dQw4w9WgXcQ"), "music-youtube-dQw4w9WgXcQ");
    }

    #[test]
    fn identity_writer_keys_use_the_prefixes_and_inputs_the_manifest_checks() {
        use ganbaru_sync::manifest::vault::music;
        let composite = [
            (&music::SIGNALS, sync_keys::SIGNALS),
            (&music::STATISTICS, sync_keys::STATISTICS),
            (&music::COLLECTION_ITEMS, sync_keys::COLLECTION_ITEMS),
            (&music::INTENDED_USES, sync_keys::INTENDED_USES),
            (&music::PRESET_SOUNDS, sync_keys::PRESET_SOUNDS),
            (&music::ASSIGNMENTS, sync_keys::ASSIGNMENTS),
        ];
        assert_eq!(composite.len(), sync_keys::COMPOSITE_KEY_TABLES.len());
        for (spec, table) in composite {
            let derived = spec
                .derived_key
                .expect("composite tables derive their keys");
            assert_eq!(spec.name, table.table);
            assert_eq!(derived.prefix, Some(table.prefix), "{}", table.table);
            assert_eq!(derived.key_inputs, table.columns, "{}", table.table);
        }
        let memberships = music::MEMBERSHIPS
            .derived_key
            .expect("memberships derive their keys");
        assert_eq!(memberships.prefix, Some(sync_keys::MEMBERSHIP_PREFIX));
        assert_eq!(memberships.key_inputs, ["playlist_id", "item_id"]);
    }

    #[test]
    fn identity_derived_keys_fit_the_id_limit() {
        let longest = [
            local_item_id("r", "p"),
            location_id("r", "p"),
            collection_id("c"),
            membership_id("p", "i").unwrap(),
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

    #[test]
    fn identity_creatable_key_skips_only_retired_keys() {
        crate::test_support::block_on(async {
            let pool = crate::tests::pool().await;
            let base = collection_id("youtube-playlist:PLretired");
            let mut connection = pool.acquire().await.unwrap();
            assert_eq!(
                creatable_key(&mut connection, RetirableTable::Collections, base.clone())
                    .await
                    .unwrap(),
                base
            );
            for key in [base.clone(), format!("{base}-2")] {
                sqlx::query(
                    "INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, ?, 'tombstoned')",
                )
                .bind(i64::from(
                    ganbaru_sync::manifest::vault::music::COLLECTIONS_TABLE.0,
                ))
                .bind(&key)
                .execute(&mut *connection)
                .await
                .unwrap();
            }
            assert_eq!(
                creatable_key(&mut connection, RetirableTable::Collections, base.clone())
                    .await
                    .unwrap(),
                format!("{base}-3")
            );
            sqlx::query(
                "INSERT INTO music_source_collections
                    (id, kind, identity_key, name, youtube_playlist_id, created_at_ms, updated_at_ms)
                 VALUES (?, 'youtube-playlist', 'youtube-playlist:PLretired', 'Live', 'PLretired', 1, 1)",
            )
            .bind(format!("{base}-2"))
            .execute(&mut *connection)
            .await
            .expect_err("sync refuses a retired successor");
            let other = creatable_key(&mut connection, RetirableTable::Items, base.clone())
                .await
                .unwrap();
            assert_eq!(other, base, "retirement is per table");
        });
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

    /// Every membership with its skip ranges, keyed by playlist and item and holding each
    /// stored key.
    async fn memberships(pool: &sqlx::SqlitePool) -> Vec<(String, String, String, String, i64)> {
        sqlx::query_as(
            "SELECT m.playlist_id, m.item_id, m.id, m.order_key,
                (SELECT COUNT(*) FROM music_membership_skip_ranges WHERE membership_id = m.id)
             FROM music_playlist_memberships AS m ORDER BY m.playlist_id, m.item_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[test]
    fn identity_rekey_moves_older_membership_keys_with_skip_ranges_and_captures_only_new_keys() {
        crate::test_support::block_on(async {
            let pool = seeded_pool().await;
            let first: String = sqlx::query_scalar(
                "SELECT id FROM music_playlist_memberships ORDER BY playlist_id, item_id LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES (?1, 0, 1000, 2000), (?1, 1, 3000, 4000)",
            )
            .bind(&first)
            .execute(&pool)
            .await
            .unwrap();
            let expected = memberships(&pool).await;
            // Rows keep their `id` under the immutable trigger, so they are recreated under the
            // older `music-membership-{hash}` form with the apply flag set.
            crate::test_support::execute_as_sync_apply(
                &pool,
                &[
                    "CREATE TEMP TABLE staged AS SELECT * FROM music_playlist_memberships",
                    "CREATE TEMP TABLE staged_skips AS SELECT * FROM music_membership_skip_ranges",
                    "UPDATE staged_skips SET membership_id = 'music-membership-older-' || membership_id",
                    "UPDATE staged SET id = 'music-membership-older-' || id",
                    "DELETE FROM music_playlist_memberships",
                    "INSERT INTO music_playlist_memberships SELECT * FROM staged",
                    "INSERT INTO music_membership_skip_ranges SELECT * FROM staged_skips",
                    "DROP TABLE staged",
                    "DROP TABLE staged_skips",
                    "DELETE FROM sync_capture",
                ],
            )
            .await;

            let mut connection = pool.acquire().await.unwrap();
            assert_eq!(
                rekey_pending(&mut connection).await.unwrap(),
                expected.len() as u64
            );
            drop(connection);
            assert_eq!(memberships(&pool).await, expected);
            let (created, older): (i64, i64) = sqlx::query_as(
                "SELECT
                    COUNT(*) FILTER (WHERE created = 1 AND deleted = 0),
                    COUNT(*) FILTER (WHERE row_key GLOB 'music-membership-older-*')
                 FROM sync_capture WHERE table_id = ?",
            )
            .bind(i64::from(
                ganbaru_sync::manifest::vault::music::MEMBERSHIPS.id.0,
            ))
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(created, expected.len() as i64);
            assert_eq!(older, 0);

            let captures = capture_count(&pool).await;
            let mut connection = pool.acquire().await.unwrap();
            assert_eq!(rekey_pending(&mut connection).await.unwrap(), 0);
            drop(connection);
            assert_eq!(capture_count(&pool).await, captures);
        });
    }
}
