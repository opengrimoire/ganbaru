use serde::Serialize;
use sqlx::{QueryBuilder, Sqlite, SqlitePool, Transaction};

use super::{MusicLibraryError, MusicLibraryResult, search};

pub(crate) const DENSE_MUSIC_FIXTURE_PROFILE: &str = "dense-music-v1";
const FIXTURE_PREFIX: &str = "benchmark-music-";
const SEEDED_AT: i64 = 1_720_000_000_000;
const LOCAL_ROOT_COUNT: usize = 4;
const YOUTUBE_COLLECTION_COUNT: usize = 3;
const LOCAL_ITEM_COUNT: usize = 5_400;
const YOUTUBE_ITEM_COUNT: usize = 600;
const PLAYLIST_COUNT: usize = 10;
const INSERT_BATCH_SIZE: usize = 250;

fn fixture_database_error(error: sqlx::Error) -> MusicLibraryError {
    MusicLibraryError::database("seed dense music fixture", error)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DenseMusicFixtureSummary {
    pub profile: &'static str,
    pub local_root_count: usize,
    pub source_collection_count: usize,
    pub item_count: usize,
    pub local_item_count: usize,
    pub youtube_item_count: usize,
    pub missing_item_count: usize,
    pub duplicate_provenance_count: usize,
    pub playlist_count: usize,
    pub membership_count: usize,
    pub snooze_count: usize,
    pub assignment_reference_count: usize,
}

/// Seeds a deterministic, network-free music library for benchmark and UI work.
pub async fn seed_dense_music_fixture(
    pool: &SqlitePool,
) -> MusicLibraryResult<DenseMusicFixtureSummary> {
    let mut transaction = pool.begin().await.map_err(fixture_database_error)?;
    clear_previous_fixture(&mut transaction).await?;
    seed_roots_and_collections(&mut transaction).await?;
    seed_playlists(&mut transaction).await?;
    seed_items(&mut transaction).await?;
    seed_locations(&mut transaction).await?;
    seed_source_provenance(&mut transaction).await?;
    let membership_count = seed_memberships(&mut transaction).await?;
    let snooze_count = seed_snoozes(&mut transaction).await?;
    seed_signals_statistics_and_history(&mut transaction).await?;
    seed_assignment_references(&mut transaction).await?;
    transaction.commit().await.map_err(fixture_database_error)?;
    search::rebuild(pool, SEEDED_AT).await?;

    Ok(DenseMusicFixtureSummary {
        profile: DENSE_MUSIC_FIXTURE_PROFILE,
        local_root_count: LOCAL_ROOT_COUNT,
        source_collection_count: LOCAL_ROOT_COUNT + YOUTUBE_COLLECTION_COUNT,
        item_count: item_count(),
        local_item_count: LOCAL_ITEM_COUNT,
        youtube_item_count: YOUTUBE_ITEM_COUNT,
        missing_item_count: missing_local_indices().count() + unavailable_youtube_indices().count(),
        duplicate_provenance_count: duplicate_local_indices().count()
            + duplicate_youtube_indices().count(),
        playlist_count: PLAYLIST_COUNT,
        membership_count,
        snooze_count,
        assignment_reference_count: 4,
    })
}

async fn clear_previous_fixture(
    transaction: &mut Transaction<'_, Sqlite>,
) -> MusicLibraryResult<()> {
    for statement in [
        "DELETE FROM calendar_events WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM projects WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM project_groups WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM music_playlists WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM music_library_items WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM music_source_collections WHERE id LIKE 'benchmark-music-%'",
        "DELETE FROM music_local_roots WHERE id LIKE 'benchmark-music-%'",
    ] {
        sqlx::query(statement)
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_roots_and_collections(
    transaction: &mut Transaction<'_, Sqlite>,
) -> MusicLibraryResult<()> {
    for root_index in 0..LOCAL_ROOT_COUNT {
        let root_id = root_id(root_index);
        sqlx::query(
            "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&root_id)
        .bind(format!("Soundtrack archive {}", root_index + 1))
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;

        sqlx::query(
            "INSERT INTO music_source_collections
                (id, kind, identity_key, name, local_root_id, refresh_state,
                 last_successful_refresh_at_ms, snapshot_generation, created_at_ms, updated_at_ms)
             VALUES (?, 'local-root', ?, ?, ?, 'idle', ?, 3, ?, ?)",
        )
        .bind(local_collection_id(root_index))
        .bind(format!("local-root:{root_id}"))
        .bind(format!("Local archive {}", root_index + 1))
        .bind(root_id)
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;
    }

    for collection_index in 0..YOUTUBE_COLLECTION_COUNT {
        let playlist_key = format!("denseYT{:02}", collection_index + 1);
        sqlx::query(
            "INSERT INTO music_source_collections
                (id, kind, identity_key, name, youtube_playlist_id, refresh_state,
                 last_successful_refresh_at_ms, snapshot_generation, created_at_ms, updated_at_ms)
             VALUES (?, 'youtube-playlist', ?, ?, ?, 'idle', ?, 2, ?, ?)",
        )
        .bind(youtube_collection_id(collection_index))
        .bind(format!("youtube-playlist:{playlist_key}"))
        .bind(format!("Online soundtrack set {}", collection_index + 1))
        .bind(playlist_key)
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_playlists(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<()> {
    const NAMES: [&str; PLAYLIST_COUNT] = [
        "Deep work",
        "Quiet reading",
        "Morning start",
        "Exercise energy",
        "Creative flow",
        "Evening reset",
        "Rainy focus",
        "Bath and unwind",
        "Short break option",
        "Favorite themes",
    ];
    const USES: [&str; PLAYLIST_COUNT] = [
        "focus",
        "reading",
        "general",
        "energizing",
        "focus",
        "relaxation",
        "focus",
        "relaxation",
        "general",
        "general",
    ];

    for (index, name) in NAMES.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO music_playlists
                (id, name, icon, shuffle_enabled, repeat_mode, sort_order, created_at_ms, updated_at_ms)
             VALUES (?, ?, ?, ?, 'all', ?, ?, ?)",
        )
        .bind(playlist_id(index))
        .bind(name)
        .bind("lucide:list-music")
        .bind(i64::from(index.is_multiple_of(2)))
        .bind(index as i64)
        .bind(SEEDED_AT)
        .bind(SEEDED_AT)
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;
        sqlx::query(
            "INSERT INTO music_playlist_intended_uses (playlist_id, intended_use) VALUES (?, ?)",
        )
        .bind(playlist_id(index))
        .bind(USES[index])
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_items(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<()> {
    for start in (0..item_count()).step_by(INSERT_BATCH_SIZE) {
        let end = (start + INSERT_BATCH_SIZE).min(item_count());
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, youtube_video_id,
                 original_title, original_artist, original_album, duration_ms,
                 availability, review_state, review_changed_at_ms, discovered_at_ms, updated_at_ms) ",
        );
        query.push_values(start..end, |mut row, index| {
            let local = index < LOCAL_ITEM_COUNT;
            let source_index = if local {
                index
            } else {
                index - LOCAL_ITEM_COUNT
            };
            let availability = if local && source_index.is_multiple_of(29) {
                "missing"
            } else if !local && source_index.is_multiple_of(31) {
                "unavailable"
            } else {
                "available"
            };
            let review_state = ["unreviewed", "reviewed", "deferred", "ignored"][index % 4];
            let youtube_id = (!local).then(|| format!("denseVideo{source_index:06}"));
            let identity = if local {
                format!("local:dense-fingerprint-{source_index:06}")
            } else {
                format!("youtube:denseVideo{source_index:06}")
            };
            let title_prefix = if local {
                "Archive track"
            } else {
                "Online theme"
            };
            let artist_prefix = if local {
                "Game composer"
            } else {
                "Soundtrack channel"
            };
            let album_count = if local { 48 } else { 12 };
            row.push_bind(item_id(index))
                .push_bind(identity)
                .push_bind(if local { "local-file" } else { "youtube-video" })
                .push_bind(if local && source_index.is_multiple_of(23) {
                    "video"
                } else {
                    "audio"
                })
                .push_bind(youtube_id)
                .push_bind(format!("{title_prefix} {:04}", source_index + 1))
                .push_bind(format!("{artist_prefix} {:02}", source_index % 24 + 1))
                .push_bind(format!("Album {:02}", source_index % album_count + 1))
                .push_bind(90_000_i64 + (source_index % 240) as i64 * 1_000)
                .push_bind(availability)
                .push_bind(review_state)
                .push_bind(SEEDED_AT + index as i64)
                .push_bind(SEEDED_AT + index as i64)
                .push_bind(SEEDED_AT + index as i64);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_locations(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<()> {
    for start in (0..LOCAL_ITEM_COUNT).step_by(INSERT_BATCH_SIZE) {
        let end = (start + INSERT_BATCH_SIZE).min(LOCAL_ITEM_COUNT);
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_local_locations
                (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
                 lightweight_fingerprint, availability, last_seen_generation,
                 first_seen_at_ms, updated_at_ms) ",
        );
        query.push_values(start..end, |mut row, index| {
            let root_index = index % LOCAL_ROOT_COUNT;
            let missing = index.is_multiple_of(29);
            row.push_bind(format!("{FIXTURE_PREFIX}location-{index:06}-primary"))
                .push_bind(item_id(index))
                .push_bind(root_id(root_index))
                .push_bind(relative_path(index, "primary"))
                .push_bind(2_000_000_i64 + index as i64 * 137)
                .push_bind(SEEDED_AT - index as i64 * 1_000)
                .push_bind(format!("dense-light-{index:06}"))
                .push_bind(if missing { "missing" } else { "available" })
                .push_bind(if missing { 2_i64 } else { 3_i64 })
                .push_bind(SEEDED_AT)
                .push_bind(SEEDED_AT + index as i64);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }

    let duplicate_indices = duplicate_local_indices().collect::<Vec<_>>();
    for chunk in duplicate_indices.chunks(INSERT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_local_locations
                (id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
                 lightweight_fingerprint, availability, last_seen_generation,
                 first_seen_at_ms, updated_at_ms) ",
        );
        query.push_values(chunk.iter().copied(), |mut row, index| {
            let second_root = (index % LOCAL_ROOT_COUNT + 1) % LOCAL_ROOT_COUNT;
            row.push_bind(format!("{FIXTURE_PREFIX}location-{index:06}-duplicate"))
                .push_bind(item_id(index))
                .push_bind(root_id(second_root))
                .push_bind(relative_path(index, "duplicate"))
                .push_bind(2_000_000_i64 + index as i64 * 137)
                .push_bind(SEEDED_AT - index as i64 * 1_000)
                .push_bind(format!("dense-light-{index:06}"))
                .push_bind("available")
                .push_bind(3_i64)
                .push_bind(SEEDED_AT)
                .push_bind(SEEDED_AT + index as i64);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_source_provenance(
    transaction: &mut Transaction<'_, Sqlite>,
) -> MusicLibraryResult<()> {
    for start in (0..item_count()).step_by(INSERT_BATCH_SIZE) {
        let end = (start + INSERT_BATCH_SIZE).min(item_count());
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, source_position, first_discovered_at_ms,
                 last_seen_generation, missing_from_latest_snapshot) ",
        );
        query.push_values(start..end, |mut row, index| {
            let local = index < LOCAL_ITEM_COUNT;
            let source_index = if local {
                index
            } else {
                index - LOCAL_ITEM_COUNT
            };
            let collection = if local {
                local_collection_id(source_index % LOCAL_ROOT_COUNT)
            } else {
                youtube_collection_id(source_index % YOUTUBE_COLLECTION_COUNT)
            };
            row.push_bind(collection)
                .push_bind(item_id(index))
                .push_bind(source_index as i64)
                .push_bind(SEEDED_AT + index as i64)
                .push_bind(if local { 3_i64 } else { 2_i64 })
                .push_bind(i64::from(
                    (local && source_index.is_multiple_of(29))
                        || (!local && source_index.is_multiple_of(31)),
                ));
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }

    let duplicate_items = duplicate_provenance_pairs();
    for chunk in duplicate_items.chunks(INSERT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, source_position, first_discovered_at_ms,
                 last_seen_generation, missing_from_latest_snapshot) ",
        );
        query.push_values(
            chunk.iter(),
            |mut row, (collection_id, index, source_index)| {
                row.push_bind(collection_id.clone())
                    .push_bind(item_id(*index))
                    .push_bind(*source_index as i64)
                    .push_bind(SEEDED_AT + *index as i64)
                    .push_bind(2_i64)
                    .push_bind(0_i64);
            },
        );
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_memberships(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<usize> {
    let weights = [
        "rarely",
        "less-often",
        "normal",
        "more-often",
        "much-more-often",
    ];
    let mut total = 0;

    for playlist_index in 0..PLAYLIST_COUNT {
        let included = (0..item_count())
            .filter(|item_index| (item_index * 3 + playlist_index * 5) % 11 < 4)
            .collect::<Vec<_>>();
        total += included.len();
        for (chunk_index, chunk) in included.chunks(INSERT_BATCH_SIZE).enumerate() {
            let mut query = QueryBuilder::<Sqlite>::new(
                "INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, position, weight, enabled,
                     start_ms, end_ms, volume, rate, created_at_ms, updated_at_ms) ",
            );
            query.push_values(chunk.iter().enumerate(), |mut row, (offset, item_index)| {
                let position = chunk_index * INSERT_BATCH_SIZE + offset;
                row.push_bind(format!(
                    "{FIXTURE_PREFIX}membership-{playlist_index:02}-{item_index:06}"
                ))
                .push_bind(playlist_id(playlist_index))
                .push_bind(item_id(*item_index))
                .push_bind(position as i64)
                .push_bind(weights[(item_index + playlist_index) % weights.len()])
                .push_bind(i64::from(!(item_index + playlist_index).is_multiple_of(19)))
                .push_bind(item_index.is_multiple_of(97).then_some(5_000_i64))
                .push_bind(item_index.is_multiple_of(97).then_some(85_000_i64))
                .push_bind(item_index.is_multiple_of(43).then_some(0.82_f64))
                .push_bind(item_index.is_multiple_of(71).then_some(1.05_f64))
                .push_bind(SEEDED_AT + position as i64)
                .push_bind(SEEDED_AT + position as i64);
            });
            query
                .build()
                .execute(&mut **transaction)
                .await
                .map_err(fixture_database_error)?;
        }
    }
    Ok(total)
}

async fn seed_snoozes(transaction: &mut Transaction<'_, Sqlite>) -> MusicLibraryResult<usize> {
    let indices = (0..item_count()).step_by(83).collect::<Vec<_>>();
    for chunk in indices.chunks(INSERT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_snoozes
                (id, item_id, scope, playlist_id, starts_at_ms, ends_at_ms, reason, created_at_ms) ",
        );
        query.push_values(chunk.iter().copied(), |mut row, index| {
            let global = index.is_multiple_of(2);
            row.push_bind(format!("{FIXTURE_PREFIX}snooze-{index:06}"))
                .push_bind(item_id(index))
                .push_bind(if global { "all-playlists" } else { "playlist" })
                .push_bind((!global).then(|| playlist_id(index % PLAYLIST_COUNT)))
                .push_bind(SEEDED_AT)
                .push_bind(SEEDED_AT + 86_400_000 * (index % 14 + 1) as i64)
                .push_bind("Deterministic review-later fixture")
                .push_bind(SEEDED_AT);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(indices.len())
}

async fn seed_signals_statistics_and_history(
    transaction: &mut Transaction<'_, Sqlite>,
) -> MusicLibraryResult<()> {
    let signal_indices = (0..item_count()).step_by(17).collect::<Vec<_>>();
    for chunk in signal_indices.chunks(INSERT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_item_signals (item_id, signal, created_at_ms) ",
        );
        query.push_values(chunk.iter().copied(), |mut row, index| {
            let signals = [
                "lyrics",
                "sudden-changes",
                "high-intensity",
                "calm",
                "repetitive",
                "energizing",
            ];
            row.push_bind(item_id(index))
                .push_bind(signals[index % signals.len()])
                .push_bind(SEEDED_AT);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }

    let statistic_indices = (0..item_count()).step_by(7).collect::<Vec<_>>();
    for chunk in statistic_indices.chunks(INSERT_BATCH_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_listening_statistics
                (item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms) ",
        );
        query.push_values(chunk.iter().copied(), |mut row, index| {
            let plays = (index % 31 + 1) as i64;
            row.push_bind(item_id(index))
                .push_bind(SEEDED_AT - index as i64 * 10_000)
                .push_bind(plays)
                .push_bind((plays - (index % 5) as i64).max(0))
                .push_bind((index % 6) as i64)
                .push_bind(SEEDED_AT);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }

    for playlist_index in 0..PLAYLIST_COUNT {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO music_recent_selections
                (playlist_id, item_id, selection_kind, selected_at_ms) ",
        );
        query.push_values(0..64, |mut row, history_index| {
            let index = (playlist_index * 173 + history_index * 19) % item_count();
            row.push_bind(playlist_id(playlist_index))
                .push_bind(item_id(index))
                .push_bind(if history_index.is_multiple_of(7) {
                    "manual"
                } else {
                    "automatic"
                })
                .push_bind(SEEDED_AT + history_index as i64);
        });
        query
            .build()
            .execute(&mut **transaction)
            .await
            .map_err(fixture_database_error)?;
    }
    Ok(())
}

async fn seed_assignment_references(
    transaction: &mut Transaction<'_, Sqlite>,
) -> MusicLibraryResult<()> {
    sqlx::query(
        "INSERT INTO project_groups (id, name, icon, sort_order)
         VALUES ('benchmark-music-group', 'Music benchmark projects', 'music', 9000)",
    )
    .execute(&mut **transaction)
    .await
    .map_err(fixture_database_error)?;
    sqlx::query(
        "INSERT INTO projects
            (id, group_id, name, icon, sort_order, focus_playlist_id, break_playlist_id)
         VALUES ('benchmark-music-project', 'benchmark-music-group',
                 'Dense music workflow', 'music', 9000, ?, ?)",
    )
    .bind(playlist_id(0))
    .bind(playlist_id(8))
    .execute(&mut **transaction)
    .await
    .map_err(fixture_database_error)?;
    for (event_index, selected_playlist) in [0_usize, 1].into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO calendar_events
                (id, title, start_time, end_time, project_id, playlist_id)
             VALUES (?, ?, ?, ?, 'benchmark-music-project', ?)",
        )
        .bind(format!("benchmark-music-event-{event_index}"))
        .bind(format!("Music benchmark session {}", event_index + 1))
        .bind(format!("2026-07-{:02}T09:00:00Z", event_index + 20))
        .bind(format!("2026-07-{:02}T10:00:00Z", event_index + 20))
        .bind(playlist_id(selected_playlist))
        .execute(&mut **transaction)
        .await
        .map_err(fixture_database_error)?;
    }
    Ok(())
}

fn item_count() -> usize {
    LOCAL_ITEM_COUNT + YOUTUBE_ITEM_COUNT
}

fn item_id(index: usize) -> String {
    format!("{FIXTURE_PREFIX}item-{index:06}")
}

fn root_id(index: usize) -> String {
    format!("{FIXTURE_PREFIX}root-{index:02}")
}

fn local_collection_id(index: usize) -> String {
    format!("{FIXTURE_PREFIX}collection-local-{index:02}")
}

fn youtube_collection_id(index: usize) -> String {
    format!("{FIXTURE_PREFIX}collection-youtube-{index:02}")
}

fn playlist_id(index: usize) -> String {
    format!("{FIXTURE_PREFIX}playlist-{index:02}")
}

fn relative_path(index: usize, copy: &str) -> String {
    format!(
        "Album {:02}/Disc {:02}/Track {:04} {copy}.{}",
        index % 48 + 1,
        index % 3 + 1,
        index + 1,
        if index.is_multiple_of(23) {
            "mp4"
        } else {
            "flac"
        }
    )
}

fn missing_local_indices() -> impl Iterator<Item = usize> {
    (0..LOCAL_ITEM_COUNT).filter(|index| index.is_multiple_of(29))
}

fn unavailable_youtube_indices() -> impl Iterator<Item = usize> {
    (0..YOUTUBE_ITEM_COUNT).filter(|index| index.is_multiple_of(31))
}

fn duplicate_local_indices() -> impl Iterator<Item = usize> {
    (0..LOCAL_ITEM_COUNT).filter(|index| index.is_multiple_of(41))
}

fn duplicate_youtube_indices() -> impl Iterator<Item = usize> {
    (0..YOUTUBE_ITEM_COUNT).filter(|index| index.is_multiple_of(37))
}

fn duplicate_provenance_pairs() -> Vec<(String, usize, usize)> {
    let mut pairs = duplicate_local_indices()
        .map(|index| {
            let collection_index = (index % LOCAL_ROOT_COUNT + 1) % LOCAL_ROOT_COUNT;
            (local_collection_id(collection_index), index, index)
        })
        .collect::<Vec<_>>();
    pairs.extend(duplicate_youtube_indices().map(|source_index| {
        let collection_index =
            (source_index % YOUTUBE_COLLECTION_COUNT + 1) % YOUTUBE_COLLECTION_COUNT;
        (
            youtube_collection_id(collection_index),
            LOCAL_ITEM_COUNT + source_index,
            source_index,
        )
    }));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

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
        ganbaru_db::run_migrations(&pool).await.unwrap();
        pool
    }

    #[test]
    #[ignore = "benchmark fixture contract"]
    fn dense_fixture_is_complete_deterministic_and_repeatable() {
        crate::test_support::block_on(async {
            let pool = migrated_pool().await;
            let first = seed_dense_music_fixture(&pool).await.unwrap();
            let second = seed_dense_music_fixture(&pool).await.unwrap();
            assert_eq!(first, second);
            assert_eq!(second.item_count, 6_000);
            assert!(second.membership_count > 20_000);
            assert!(second.duplicate_provenance_count > 100);
            assert!(second.missing_item_count > 150);

            let counts = sqlx::query(
            "SELECT
                (SELECT COUNT(*) FROM music_library_items WHERE id LIKE 'benchmark-music-%') AS items,
                (SELECT COUNT(*) FROM music_playlist_memberships WHERE id LIKE 'benchmark-music-%') AS memberships,
                (SELECT COUNT(DISTINCT review_state) FROM music_library_items
                    WHERE id LIKE 'benchmark-music-%') AS review_states,
                (SELECT COUNT(DISTINCT item_id) FROM music_source_collection_items
                    WHERE item_id LIKE 'benchmark-music-%'
                    GROUP BY item_id HAVING COUNT(*) > 1 LIMIT 1) AS has_duplicate,
                (SELECT COUNT(*) FROM music_search_fts
                    WHERE item_id LIKE 'benchmark-music-%') AS search_rows",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
            assert_eq!(counts.get::<i64, _>("items"), second.item_count as i64);
            assert_eq!(
                counts.get::<i64, _>("memberships"),
                second.membership_count as i64
            );
            assert_eq!(counts.get::<i64, _>("review_states"), 4);
            assert!(counts.get::<i64, _>("has_duplicate") > 0);
            assert_eq!(
                counts.get::<i64, _>("search_rows"),
                second.item_count as i64
            );

            let assignments: i64 = sqlx::query_scalar(
                "SELECT
                (SELECT COUNT(focus_playlist_id) + COUNT(break_playlist_id)
                 FROM projects WHERE id = 'benchmark-music-project')
                + (SELECT COUNT(playlist_id) FROM calendar_events
                   WHERE id LIKE 'benchmark-music-event-%')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(assignments, second.assignment_reference_count as i64);

            let stable_sample: String = sqlx::query_scalar(
                "SELECT group_concat(id || ':' || review_state || ':' || availability, '|')
             FROM (
                SELECT id, review_state, availability
                FROM music_library_items
                WHERE id LIKE 'benchmark-music-%'
                ORDER BY id
                LIMIT 5
             )",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                stable_sample,
                "benchmark-music-item-000000:unreviewed:missing|benchmark-music-item-000001:reviewed:available|benchmark-music-item-000002:deferred:available|benchmark-music-item-000003:ignored:available|benchmark-music-item-000004:unreviewed:available"
            );
        });
    }
}
