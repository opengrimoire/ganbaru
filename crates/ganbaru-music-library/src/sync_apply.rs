//! Device state that follows Music rows applied by sync.
//!
//! The search index is derived from items and the rows that describe them, so after sync
//! materializes such rows the affected search rows are rebuilt by item id. A deleted row no longer
//! names its item, so a deletion marks the index stale instead and the next search rebuilds it.
//! Locations that arrive from another device name the roots this device should refresh, so the
//! availability it observes catches up with the new rows.

use sqlx::{QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::{MusicLibraryError, MusicLibraryResult, search};

/// Items table, keyed by item id.
pub const ITEMS_TABLE: &str = "music_library_items";
/// Item signals, keyed by `sync_key`.
pub const SIGNALS_TABLE: &str = "music_item_signals";
/// Local locations, keyed by location id.
pub const LOCATIONS_TABLE: &str = "music_local_locations";
/// Source collections, keyed by collection id.
pub const COLLECTIONS_TABLE: &str = "music_source_collections";
/// Source collection items, keyed by `sync_key`.
pub const COLLECTION_ITEMS_TABLE: &str = "music_source_collection_items";
/// Playlists, keyed by playlist id.
pub const PLAYLISTS_TABLE: &str = "music_playlists";
/// Playlist intended uses, keyed by `sync_key`.
pub const INTENDED_USES_TABLE: &str = "music_playlist_intended_uses";
/// Playlist memberships, keyed by membership id.
pub const MEMBERSHIPS_TABLE: &str = "music_playlist_memberships";

/// Tables whose rows feed the search index, so applied changes to them refresh it.
pub const SEARCH_SOURCE_TABLES: [&str; 8] = [
    ITEMS_TABLE,
    SIGNALS_TABLE,
    LOCATIONS_TABLE,
    COLLECTIONS_TABLE,
    COLLECTION_ITEMS_TABLE,
    PLAYLISTS_TABLE,
    INTENDED_USES_TABLE,
    MEMBERSHIPS_TABLE,
];

/// Most keys bound in one statement, below SQLite's variable limit.
const KEY_CHUNK: usize = 500;

/// How one search source table maps its row keys to the items whose search rows it feeds.
struct SearchSource {
    /// Statement prefix that selects distinct item ids; the key list follows it.
    items_by_key: &'static str,
    /// Statement prefix that counts the keys still present; the key list follows it.
    present_keys: &'static str,
}

fn search_source(table: &str) -> MusicLibraryResult<SearchSource> {
    let source = match table {
        SIGNALS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_item_signals WHERE sync_key IN (",
            present_keys: "SELECT COUNT(*) FROM music_item_signals WHERE sync_key IN (",
        },
        LOCATIONS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_local_locations WHERE id IN (",
            present_keys: "SELECT COUNT(*) FROM music_local_locations WHERE id IN (",
        },
        COLLECTIONS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_source_collection_items
                WHERE collection_id IN (",
            present_keys: "SELECT COUNT(*) FROM music_source_collections WHERE id IN (",
        },
        COLLECTION_ITEMS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_source_collection_items
                WHERE sync_key IN (",
            present_keys: "SELECT COUNT(*) FROM music_source_collection_items WHERE sync_key IN (",
        },
        PLAYLISTS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_playlist_memberships
                WHERE playlist_id IN (",
            present_keys: "SELECT COUNT(*) FROM music_playlists WHERE id IN (",
        },
        INTENDED_USES_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT membership.item_id
                FROM music_playlist_intended_uses AS intended
                JOIN music_playlist_memberships AS membership
                    ON membership.playlist_id = intended.playlist_id
                WHERE intended.sync_key IN (",
            present_keys: "SELECT COUNT(*) FROM music_playlist_intended_uses WHERE sync_key IN (",
        },
        MEMBERSHIPS_TABLE => SearchSource {
            items_by_key: "SELECT DISTINCT item_id FROM music_playlist_memberships WHERE id IN (",
            present_keys: "SELECT COUNT(*) FROM music_playlist_memberships WHERE id IN (",
        },
        other => {
            return Err(MusicLibraryError::validation(
                "table",
                format!("{other} does not feed the music search index"),
            ));
        }
    };
    Ok(source)
}

/// Builds `prefix` followed by `keys` as bound values and a closing parenthesis.
fn keyed_query(prefix: &'static str, keys: &[String]) -> QueryBuilder<Sqlite> {
    let mut query = QueryBuilder::<Sqlite>::new(prefix);
    let mut separated = query.separated(", ");
    for key in keys {
        separated.push_bind(key);
    }
    separated.push_unseparated(")");
    query
}

/// Rebuilds the search rows that rows of `table` with the changed `keys` feed, on the
/// connection of the sync transaction that applied them. Deleted keys mark the index stale, so
/// the next search rebuilds it.
pub async fn refresh_search_after_apply(
    conn: &mut SqliteConnection,
    table: &str,
    keys: &[String],
) -> MusicLibraryResult<()> {
    if keys.is_empty() {
        return Ok(());
    }
    if table == ITEMS_TABLE {
        for chunk in keys.chunks(KEY_CHUNK) {
            search::refresh_items_on(conn, chunk).await?;
        }
        return Ok(());
    }
    let source = search_source(table)?;
    let mut item_ids: Vec<String> = Vec::new();
    let mut deleted = false;
    for chunk in keys.chunks(KEY_CHUNK) {
        let present: i64 = keyed_query(source.present_keys, chunk)
            .build_query_scalar()
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| MusicLibraryError::database("count applied music rows", error))?;
        deleted |= usize::try_from(present).map_or(true, |present| present < chunk.len());
        let found: Vec<String> = keyed_query(source.items_by_key, chunk)
            .build_query_scalar()
            .fetch_all(&mut *conn)
            .await
            .map_err(|error| MusicLibraryError::database("load applied music items", error))?;
        item_ids.extend(found);
    }
    item_ids.sort_unstable();
    item_ids.dedup();
    for chunk in item_ids.chunks(KEY_CHUNK) {
        search::refresh_items_on(conn, chunk).await?;
    }
    if deleted {
        search::mark_stale(conn).await?;
    }
    Ok(())
}

/// The distinct roots of the locations among `location_ids` that still exist, the roots that
/// received new or changed locations.
pub async fn roots_of_locations(
    conn: &mut SqliteConnection,
    location_ids: &[String],
) -> MusicLibraryResult<Vec<String>> {
    let mut roots: Vec<String> = Vec::new();
    for chunk in location_ids.chunks(KEY_CHUNK) {
        let found: Vec<String> = keyed_query(
            "SELECT DISTINCT root_id FROM music_local_locations WHERE id IN (",
            chunk,
        )
        .build_query_scalar()
        .fetch_all(&mut *conn)
        .await
        .map_err(|error| MusicLibraryError::database("load applied music roots", error))?;
        roots.extend(found);
    }
    roots.sort_unstable();
    roots.dedup();
    Ok(roots)
}

/// The local source collections of `root_id` whose discovery is enabled, the targets a refresh
/// of that root runs for.
pub async fn refreshable_collections_of_root(
    pool: &SqlitePool,
    root_id: &str,
) -> MusicLibraryResult<Vec<String>> {
    sqlx::query_scalar(
        "SELECT id FROM music_source_collections
         WHERE kind = 'local-root' AND local_root_id = ? AND discovery_enabled = 1
         ORDER BY id",
    )
    .bind(root_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load refreshable music sources", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{block_on, execute_as_sync_apply};

    async fn migrated_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    async fn search_text(pool: &SqlitePool, item_id: &str) -> Option<String> {
        sqlx::query_scalar(
            "SELECT title || '|' || signals || '|' || relative_paths
             FROM music_search_fts WHERE item_id = ?",
        )
        .bind(item_id)
        .fetch_optional(pool)
        .await
        .unwrap()
    }

    async fn index_state_rows(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM music_search_index_state")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    async fn refresh(pool: &SqlitePool, table: &str, keys: &[&str]) {
        let keys: Vec<String> = keys.iter().map(|key| (*key).to_string()).collect();
        let mut conn = pool.acquire().await.unwrap();
        refresh_search_after_apply(&mut conn, table, &keys)
            .await
            .unwrap();
    }

    /// Stages one item with a signal and a location as sync would apply them, without
    /// touching the search index.
    async fn stage_item(pool: &SqlitePool) {
        execute_as_sync_apply(
            pool,
            &[
                "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
                 VALUES ('root-1', 'Root', 1, 1)",
                "INSERT INTO music_library_items
                    (id, identity_key, source_kind, media_kind, original_title,
                     discovered_at_ms, updated_at_ms)
                 VALUES ('item-1', 'local:item-1', 'local-file', 'audio', 'Remote title', 1, 1)",
                "INSERT INTO music_item_signals (item_id, signal, created_at_ms, sync_key)
                 VALUES ('item-1', 'calm', 1, 'signal-key-1')",
                "INSERT INTO music_local_locations
                    (id, item_id, root_id, relative_path, first_seen_at_ms, updated_at_ms)
                 VALUES ('location-1', 'item-1', 'root-1', 'album/track.mp3', 1, 1)",
            ],
        )
        .await;
    }

    #[test]
    fn applied_items_and_children_rebuild_their_search_rows() {
        block_on(async {
            let pool = migrated_pool().await;
            stage_item(&pool).await;
            assert_eq!(search_text(&pool, "item-1").await, None);

            refresh(&pool, ITEMS_TABLE, &["item-1"]).await;
            assert_eq!(
                search_text(&pool, "item-1").await.as_deref(),
                Some("Remote title|calm|album/track.mp3")
            );

            execute_as_sync_apply(
                &pool,
                &[
                    "UPDATE music_local_locations SET relative_path = 'moved/track.mp3'
                   WHERE id = 'location-1'",
                ],
            )
            .await;
            refresh(&pool, LOCATIONS_TABLE, &["location-1"]).await;
            assert_eq!(
                search_text(&pool, "item-1").await.as_deref(),
                Some("Remote title|calm|moved/track.mp3")
            );
        });
    }

    #[test]
    fn applied_deletions_mark_the_index_stale_and_deleted_items_leave_it() {
        block_on(async {
            let pool = migrated_pool().await;
            stage_item(&pool).await;
            crate::search::rebuild(&pool, 1).await.unwrap();
            assert_eq!(index_state_rows(&pool).await, 1);

            refresh(&pool, SIGNALS_TABLE, &["signal-key-1"]).await;
            assert_eq!(
                index_state_rows(&pool).await,
                1,
                "a present key is no deletion"
            );

            execute_as_sync_apply(
                &pool,
                &["DELETE FROM music_item_signals WHERE sync_key = 'signal-key-1'"],
            )
            .await;
            refresh(&pool, SIGNALS_TABLE, &["signal-key-1"]).await;
            assert_eq!(index_state_rows(&pool).await, 0);

            execute_as_sync_apply(
                &pool,
                &["DELETE FROM music_library_items WHERE id = 'item-1'"],
            )
            .await;
            refresh(&pool, ITEMS_TABLE, &["item-1"]).await;
            assert_eq!(search_text(&pool, "item-1").await, None);
        });
    }

    #[test]
    fn every_search_source_table_is_accepted_and_others_are_refused() {
        block_on(async {
            let pool = migrated_pool().await;
            let keys = vec!["missing".to_string()];
            let mut conn = pool.acquire().await.unwrap();
            for table in SEARCH_SOURCE_TABLES {
                refresh_search_after_apply(&mut conn, table, &keys)
                    .await
                    .unwrap_or_else(|error| panic!("{table}: {error:?}"));
            }
            assert!(
                refresh_search_after_apply(&mut conn, "music_snoozes", &keys)
                    .await
                    .is_err()
            );
        });
    }

    /// Changed items one post-apply refresh covers in the desktop budget.
    const BUDGET_ITEMS: i64 = 1_000;
    const BUDGET: std::time::Duration = std::time::Duration::from_millis(200);

    /// Desktop budget of the post-apply search refresh. Ignored by default because timings only
    /// mean something in the release profile: `cargo test -p ganbaru-music-library --lib
    /// --release -j 1 search_refresh_budget -- --ignored --nocapture`. The debug profile prints
    /// the measurement without asserting it.
    #[test]
    #[ignore = "timing budget, run in the release profile"]
    fn search_refresh_budget_of_one_thousand_changed_items() {
        block_on(async {
            let pool = migrated_pool().await;
            let insert = format!(
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {BUDGET_ITEMS})
                 INSERT INTO music_library_items
                    (id, identity_key, source_kind, media_kind, original_title, original_artist,
                     discovered_at_ms, updated_at_ms)
                 SELECT 'item-' || i, 'local:item-' || i, 'local-file', 'audio',
                        'Title ' || i, 'Artist ' || (i % 50), 1, 1
                 FROM n"
            );
            execute_as_sync_apply(&pool, &[insert.as_str()]).await;
            let keys: Vec<String> = (1..=BUDGET_ITEMS).map(|i| format!("item-{i}")).collect();
            let mut conn = pool.acquire().await.unwrap();
            let started = std::time::Instant::now();
            refresh_search_after_apply(&mut conn, ITEMS_TABLE, &keys)
                .await
                .unwrap();
            let measured = started.elapsed();
            println!(
                "post-apply search refresh of {BUDGET_ITEMS} items: {measured:?} (budget {BUDGET:?})"
            );
            let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_search_fts")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
            assert_eq!(indexed, BUDGET_ITEMS);
            if !cfg!(debug_assertions) {
                assert!(measured < BUDGET, "took {measured:?}, budget {BUDGET:?}");
            }
        });
    }

    #[test]
    fn applied_locations_name_their_roots_once() {
        block_on(async {
            let pool = migrated_pool().await;
            stage_item(&pool).await;
            let mut conn = pool.acquire().await.unwrap();
            let roots = roots_of_locations(
                &mut conn,
                &[
                    "location-1".to_string(),
                    "location-1".to_string(),
                    "deleted".to_string(),
                ],
            )
            .await
            .unwrap();
            assert_eq!(roots, vec!["root-1".to_string()]);
        });
    }
}
