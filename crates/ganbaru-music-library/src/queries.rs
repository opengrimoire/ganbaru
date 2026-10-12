use super::*;
use sqlx::{QueryBuilder, Sqlite, SqlitePool};
use std::collections::HashMap;

pub async fn membership_matrix(
    pool: &SqlitePool,
    item_ids: Vec<String>,
) -> MusicLibraryResult<Vec<MusicMembershipMatrixEntry>> {
    validate_review_selection_ids(&item_ids, "itemIds")?;
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT item_id, playlist_id, weight FROM music_playlist_memberships WHERE item_id IN (",
    );
    let mut separated = query.separated(", ");
    for item_id in &item_ids {
        separated.push_bind(item_id);
    }
    separated.push_unseparated(") ORDER BY playlist_id, item_id");
    let rows = query
        .build_query_as::<(String, String, String)>()
        .fetch_all(pool)
        .await
        .map_err(|error| MusicLibraryError::database("load membership matrix", error))?;
    rows.into_iter()
        .map(|(item_id, playlist_id, weight)| {
            Ok(MusicMembershipMatrixEntry {
                item_id,
                playlist_id,
                weight: MusicWeight::try_from(weight.as_str()).map_err(|message| {
                    MusicLibraryError::runtime("decode membership weight", message)
                })?,
            })
        })
        .collect()
}

pub async fn playlist_playback_entries(
    pool: &SqlitePool,
    playlist_id: &str,
    now_ms: i64,
) -> MusicLibraryResult<Vec<MusicPlaylistPlaybackEntry>> {
    validate_id(playlist_id, "playlistId")?;
    if now_ms <= 0 {
        return Err(MusicLibraryError::validation("nowMs", "must be positive"));
    }
    let rows = sqlx::query_as::<_, MusicPlaylistPlaybackRow>(
        "SELECT
            membership.id AS membership_id,
            item.id AS item_id,
            item.identity_key,
            item.source_kind,
            item.youtube_video_id,
            item.youtube_resolution_state,
            COALESCE(NULLIF(item.title_override, ''), item.original_title) AS title,
            item.original_artwork_identity,
            item.artwork_override,
            item.availability,
            (SELECT location.root_id FROM music_local_locations AS location
             WHERE location.item_id = item.id AND location.availability = 'available'
             ORDER BY location.updated_at_ms DESC, location.id LIMIT 1) AS root_id,
            (SELECT location.relative_path FROM music_local_locations AS location
             WHERE location.item_id = item.id AND location.availability = 'available'
             ORDER BY location.updated_at_ms DESC, location.id LIMIT 1) AS relative_path,
            membership.position,
            membership.weight,
            membership.enabled,
            membership.start_ms,
            membership.end_ms,
            membership.volume,
            membership.rate,
            EXISTS(
                SELECT 1 FROM music_snoozes AS snooze
                WHERE snooze.item_id = item.id
                  AND snooze.starts_at_ms <= ?
                  AND (snooze.ends_at_ms IS NULL OR snooze.ends_at_ms > ?)
                  AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = membership.playlist_id)
            ) AS snoozed,
            (SELECT MAX(snooze.ends_at_ms) FROM music_snoozes AS snooze
             WHERE snooze.item_id = item.id
               AND snooze.starts_at_ms <= ?
               AND snooze.ends_at_ms > ?
               AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = membership.playlist_id)
            ) AS snoozed_until,
            EXISTS(
                SELECT 1 FROM music_snoozes AS snooze
                WHERE snooze.item_id = item.id
                  AND snooze.starts_at_ms <= ?
                  AND snooze.ends_at_ms IS NULL
                  AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = membership.playlist_id)
            ) AS snoozed_indefinitely
         FROM music_playlist_memberships AS membership
         JOIN music_library_items AS item ON item.id = membership.item_id
         WHERE membership.playlist_id = ?
         ORDER BY membership.position, membership.id",
    )
    .bind(now_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist playback entries", error))?;
    let mut entries = rows
        .into_iter()
        .map(TryInto::try_into)
        .collect::<MusicLibraryResult<Vec<MusicPlaylistPlaybackEntry>>>()?;
    let skip_ranges = sqlx::query_as::<_, (String, String, i64, i64, i64)>(
        "SELECT skip.id, skip.membership_id, skip.start_ms, skip.end_ms, skip.sort_order
         FROM music_membership_skip_ranges AS skip
         JOIN music_playlist_memberships AS membership ON membership.id = skip.membership_id
         WHERE membership.playlist_id = ?
         ORDER BY skip.membership_id, skip.sort_order",
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist playback skip ranges", error))?;
    let mut by_membership: HashMap<String, Vec<MusicMembershipSkipRange>> = HashMap::new();
    for (id, membership_id, start_ms, end_ms, sort_order) in skip_ranges {
        by_membership
            .entry(membership_id.clone())
            .or_default()
            .push(MusicMembershipSkipRange {
                id,
                membership_id,
                start_ms,
                end_ms,
                sort_order,
            });
    }
    for entry in &mut entries {
        entry.skip_ranges = by_membership
            .remove(&entry.membership_id)
            .unwrap_or_default();
    }
    Ok(entries)
}

fn push_item_from(builder: &mut QueryBuilder<'_, Sqlite>, request: &MusicItemWindowRequest) {
    builder.push(
        " FROM music_library_items AS item
          LEFT JOIN music_listening_statistics AS stats ON stats.item_id = item.id ",
    );
    if request.destination == MusicListDestination::Playlist {
        builder.push(
            "JOIN music_playlist_memberships AS membership
             ON membership.item_id = item.id AND membership.playlist_id = ",
        );
        builder.push_bind(request.playlist_id.clone().unwrap_or_default());
        builder.push(" ");
    } else {
        builder.push(
            "LEFT JOIN music_playlist_memberships AS membership
             ON membership.item_id = item.id AND 0 = 1 ",
        );
    }
}

fn push_item_filters(builder: &mut QueryBuilder<'_, Sqlite>, request: &MusicItemWindowRequest) {
    builder.push(" WHERE 1 = 1 ");
    if let Some(source_kind) = request.source_kind {
        builder.push("AND item.source_kind = ");
        builder.push_bind(source_kind.as_ref().to_string());
        builder.push(" ");
    }
    if let Some(playlist_id) = &request.membership_playlist_id {
        builder.push(
            "AND EXISTS (SELECT 1 FROM music_playlist_memberships AS filtered_membership
             WHERE filtered_membership.item_id = item.id AND filtered_membership.playlist_id = ",
        );
        builder.push_bind(playlist_id.clone());
        builder.push(") ");
    }
    if let Some(availability) = request.availability {
        builder.push("AND item.availability = ");
        builder.push_bind(availability.as_ref().to_string());
        builder.push(" ");
    }
    if let Some(review_state) = request.review_state {
        builder.push("AND item.review_state = ");
        builder.push_bind(review_state.as_ref().to_string());
        builder.push(" ");
    }
    if let Some(collection_id) = &request.source_collection_id {
        builder.push(
            "AND EXISTS (
                SELECT 1 FROM music_source_collection_items AS source_item
                WHERE source_item.item_id = item.id AND source_item.collection_id = ",
        );
        builder.push_bind(collection_id.clone());
        builder.push(") ");
    }
    if let Some(snoozed) = request.snoozed {
        builder.push(if snoozed {
            "AND EXISTS ("
        } else {
            "AND NOT EXISTS ("
        });
        builder.push(
            "SELECT 1 FROM music_snoozes AS snooze
             WHERE snooze.item_id = item.id
               AND snooze.starts_at_ms <= ",
        );
        builder.push_bind(request.now_ms);
        builder.push(" AND (snooze.ends_at_ms IS NULL OR snooze.ends_at_ms > ");
        builder.push_bind(request.now_ms);
        builder.push(") ");
        if request.destination == MusicListDestination::Playlist {
            builder.push("AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = ");
            builder.push_bind(request.playlist_id.clone().unwrap_or_default());
            builder.push(") ");
        }
        builder.push(") ");
    }
    if let Some(search_query) = super::search::match_expression(&request.search) {
        builder.push(
            "AND item.id IN (
                SELECT item_id FROM music_search_fts WHERE music_search_fts MATCH ",
        );
        builder.push_bind(search_query);
        builder.push(") ");
    }
}

fn push_item_order(builder: &mut QueryBuilder<'_, Sqlite>, request: &MusicItemWindowRequest) {
    builder.push(" ORDER BY ");
    if let Some(group) = group_expression(request.group_by) {
        builder.push(group);
        builder.push(" COLLATE NOCASE ASC, ");
    }
    let expression = match request.sort {
        MusicItemSort::Title => "COALESCE(item.title_override, item.original_title) COLLATE NOCASE",
        MusicItemSort::Artist => {
            "COALESCE(item.artist_override, item.original_artist) COLLATE NOCASE"
        }
        MusicItemSort::Album => "COALESCE(item.album_override, item.original_album) COLLATE NOCASE",
        MusicItemSort::SourceOrder => {
            "COALESCE((SELECT MIN(source_item.source_position)
            FROM music_source_collection_items AS source_item
            WHERE source_item.item_id = item.id), 9223372036854775807)"
        }
        MusicItemSort::DiscoveredAt => "item.discovered_at_ms",
        MusicItemSort::AddedToPlaylist => "membership.created_at_ms",
        MusicItemSort::LastPlayedAt => "COALESCE(stats.last_played_at_ms, 0)",
        MusicItemSort::PlayCount => "COALESCE(stats.play_count, 0)",
        MusicItemSort::ManualPosition => "membership.position",
    };
    builder.push(expression);
    builder.push(match request.direction {
        MusicSortDirection::Ascending => " ASC, ",
        MusicSortDirection::Descending => " DESC, ",
    });
    builder.push("item.id ASC");
}

fn group_expression(group_by: MusicGroupBy) -> Option<&'static str> {
    match group_by {
        MusicGroupBy::None => None,
        MusicGroupBy::SourceKind => Some("item.source_kind"),
        MusicGroupBy::ReviewState => Some("item.review_state"),
        MusicGroupBy::Availability => Some("item.availability"),
        MusicGroupBy::Album => Some(
            "CASE WHEN trim(COALESCE(item.album_override, item.original_album)) = ''
             THEN 'unknown' ELSE COALESCE(item.album_override, item.original_album) END",
        ),
        MusicGroupBy::Folder => Some(
            "CASE WHEN item.source_kind = 'youtube-video' THEN 'Online'
             ELSE COALESCE((
                 SELECT CASE
                     WHEN instr(replace(location.relative_path, '\\', '/'), '/') > 0
                     THEN substr(replace(location.relative_path, '\\', '/'), 1,
                         instr(replace(location.relative_path, '\\', '/'), '/') - 1)
                     ELSE 'Root folder'
                 END
                 FROM music_local_locations AS location
                 WHERE location.item_id = item.id
                 ORDER BY location.availability = 'available' DESC, location.relative_path
                 LIMIT 1
             ), 'Unknown folder') END",
        ),
        MusicGroupBy::SourceCollection => Some(
            "COALESCE((
                SELECT MIN(source.name COLLATE NOCASE)
                FROM music_source_collection_items AS source_item
                JOIN music_source_collections AS source ON source.id = source_item.collection_id
                WHERE source_item.item_id = item.id
            ), 'Unlinked source')",
        ),
    }
}

pub async fn item_window(
    pool: &SqlitePool,
    request: MusicItemWindowRequest,
) -> MusicLibraryResult<MusicItemWindow> {
    validate_item_window(&request)?;
    super::search::ensure(pool, request.now_ms).await?;
    let mut count_query = QueryBuilder::<Sqlite>::new("SELECT COUNT(*)");
    push_item_from(&mut count_query, &request);
    push_item_filters(&mut count_query, &request);
    let total_count: i64 = count_query
        .build_query_scalar()
        .fetch_one(pool)
        .await
        .map_err(|error| MusicLibraryError::database("count music item window", error))?;

    let mut item_query = QueryBuilder::<Sqlite>::new(
        "SELECT
            item.id, item.identity_key, item.source_kind, item.media_kind,
            COALESCE(item.title_override, item.original_title) AS title,
            COALESCE(item.artist_override, item.original_artist) AS artist,
            COALESCE(item.album_override, item.original_album) AS album,
            (SELECT location.root_id FROM music_local_locations AS location
             WHERE location.item_id = item.id
             ORDER BY location.availability = 'available' DESC, location.updated_at_ms DESC, location.id
             LIMIT 1) AS local_root_id,
            (SELECT location.relative_path FROM music_local_locations AS location
             WHERE location.item_id = item.id
             ORDER BY location.availability = 'available' DESC, location.updated_at_ms DESC, location.id
             LIMIT 1) AS relative_path,
            COALESCE((
                SELECT json_group_array(collection_id) FROM (
                    SELECT source_item.collection_id AS collection_id
                    FROM music_source_collection_items AS source_item
                    WHERE source_item.item_id = item.id
                    ORDER BY source_item.collection_id
                )
            ), '[]') AS source_collection_ids_json,
            item.original_artwork_identity,
            item.artwork_override,
            item.duration_ms, item.availability, item.review_state,
            item.discovered_at_ms, item.updated_at_ms, item.version,
            (SELECT COUNT(*) FROM music_playlist_memberships AS all_memberships
             WHERE all_memberships.item_id = item.id) AS playlist_count,
            (SELECT COUNT(*) FROM music_snoozes AS active_snooze
             WHERE active_snooze.item_id = item.id
               AND active_snooze.starts_at_ms <= ",
    );
    item_query.push_bind(request.now_ms);
    item_query.push(" AND (active_snooze.ends_at_ms IS NULL OR active_snooze.ends_at_ms > ");
    item_query.push_bind(request.now_ms);
    if request.destination == MusicListDestination::Playlist {
        item_query
            .push(" AND (active_snooze.scope = 'all-playlists' OR active_snooze.playlist_id = ");
        item_query.push_bind(request.playlist_id.clone().unwrap_or_default());
        item_query.push(")");
    }
    item_query.push(
        ")) AS active_snooze_count,
            stats.last_played_at_ms, COALESCE(stats.play_count, 0) AS play_count,
            membership.id AS membership_id, membership.position AS membership_position,
            membership.weight AS membership_weight, membership.enabled AS membership_enabled,
            membership.version AS membership_version",
    );
    push_item_from(&mut item_query, &request);
    push_item_filters(&mut item_query, &request);
    push_item_order(&mut item_query, &request);
    item_query.push(" LIMIT ");
    item_query.push_bind(request.limit);
    item_query.push(" OFFSET ");
    item_query.push_bind(request.offset);
    let rows = item_query
        .build_query_as::<MusicItemListRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| MusicLibraryError::database("load music item window", error))?;
    let items = rows
        .into_iter()
        .map(MusicItemListEntry::try_from)
        .collect::<MusicLibraryResult<Vec<_>>>()?;

    let groups = if let Some(expression) = group_expression(request.group_by) {
        let mut group_query = QueryBuilder::<Sqlite>::new("SELECT ");
        group_query.push(expression);
        group_query.push(" AS group_key, COUNT(*) AS group_count");
        push_item_from(&mut group_query, &request);
        push_item_filters(&mut group_query, &request);
        group_query.push(" GROUP BY ");
        group_query.push(expression);
        group_query.push(" ORDER BY group_count DESC, group_key COLLATE NOCASE ASC");
        group_query
            .build_query_as::<(String, i64)>()
            .fetch_all(pool)
            .await
            .map_err(|error| MusicLibraryError::database("group music item window", error))?
            .into_iter()
            .map(|(key, count)| MusicGroupCount { key, count })
            .collect()
    } else {
        Vec::new()
    };

    Ok(MusicItemWindow {
        items,
        groups,
        total_count,
        offset: request.offset,
        limit: request.limit,
    })
}

#[derive(sqlx::FromRow)]
struct PlaylistSummaryRow {
    id: String,
    name: String,
    icon: String,
    shuffle_enabled: i64,
    mix_enabled: i64,
    repeat_mode: String,
    intended_uses: String,
    sort_order: i64,
    total_count: i64,
    eligible_count: i64,
    unavailable_count: i64,
    snoozed_count: i64,
    local_count: i64,
    online_count: i64,
    version: i64,
}

pub async fn playlist_summaries(
    pool: &SqlitePool,
    now_ms: i64,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicPlaylistSummary>> {
    validate_summary_window(offset, limit)?;
    if now_ms <= 0 {
        return Err(MusicLibraryError::validation("nowMs", "must be positive"));
    }
    super::defaults::ensure_built_in_music_playlists(pool).await?;
    let rows = sqlx::query_as::<_, PlaylistSummaryRow>(
        "SELECT playlist.id, playlist.name, playlist.icon, playlist.shuffle_enabled, playlist.mix_enabled,
                playlist.repeat_mode, playlist.sort_order,
                COALESCE((
                    SELECT group_concat(intended.intended_use, ',')
                    FROM music_playlist_intended_uses AS intended
                    WHERE intended.playlist_id = playlist.id
                    ORDER BY intended.intended_use
                ), '') AS intended_uses,
                COUNT(membership.id) AS total_count,
                SUM(CASE WHEN membership.enabled = 1 AND item.availability = 'available'
                    AND NOT EXISTS (
                        SELECT 1 FROM music_snoozes AS snooze
                        WHERE snooze.item_id = item.id AND snooze.starts_at_ms <= ?
                          AND (snooze.ends_at_ms IS NULL OR snooze.ends_at_ms > ?)
                          AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = playlist.id)
                    ) THEN 1 ELSE 0 END) AS eligible_count,
                SUM(CASE WHEN item.id IS NOT NULL AND item.availability <> 'available'
                    THEN 1 ELSE 0 END) AS unavailable_count,
                SUM(CASE WHEN item.id IS NOT NULL AND EXISTS (
                        SELECT 1 FROM music_snoozes AS snooze
                        WHERE snooze.item_id = item.id AND snooze.starts_at_ms <= ?
                          AND (snooze.ends_at_ms IS NULL OR snooze.ends_at_ms > ?)
                          AND (snooze.scope = 'all-playlists' OR snooze.playlist_id = playlist.id)
                    ) THEN 1 ELSE 0 END) AS snoozed_count,
                SUM(CASE WHEN item.source_kind = 'local-file' THEN 1 ELSE 0 END) AS local_count,
                SUM(CASE WHEN item.source_kind = 'youtube-video' THEN 1 ELSE 0 END) AS online_count,
                playlist.version
         FROM music_playlists AS playlist
         LEFT JOIN music_playlist_memberships AS membership ON membership.playlist_id = playlist.id
         LEFT JOIN music_library_items AS item ON item.id = membership.item_id
         GROUP BY playlist.id
         ORDER BY playlist.sort_order, playlist.name COLLATE NOCASE, playlist.id
         LIMIT ? OFFSET ?",
    )
    .bind(now_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(now_ms)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist summaries", error))?;
    rows.into_iter()
        .map(|row| {
            Ok(MusicPlaylistSummary {
                id: row.id,
                name: row.name,
                icon: row.icon,
                shuffle_enabled: match row.shuffle_enabled {
                    0 => false,
                    1 => true,
                    value => {
                        return Err(MusicLibraryError::validation(
                            "shuffleEnabled",
                            format!("expected 0 or 1, received {value}"),
                        ));
                    }
                },
                mix_enabled: match row.mix_enabled {
                    0 => false,
                    1 => true,
                    value => {
                        return Err(MusicLibraryError::validation(
                            "mixEnabled",
                            format!("expected 0 or 1, received {value}"),
                        ));
                    }
                },
                repeat_mode: MusicRepeatMode::try_from(row.repeat_mode.as_str())
                    .map_err(|message| MusicLibraryError::validation("repeatMode", message))?,
                intended_uses: row
                    .intended_uses
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        MusicIntendedUse::try_from(value).map_err(|message| {
                            MusicLibraryError::validation("intendedUses", message)
                        })
                    })
                    .collect::<MusicLibraryResult<Vec<_>>>()?,
                sort_order: row.sort_order,
                total_count: row.total_count,
                eligible_count: row.eligible_count,
                unavailable_count: row.unavailable_count,
                snoozed_count: row.snoozed_count,
                local_count: row.local_count,
                online_count: row.online_count,
                version: row.version,
            })
        })
        .collect()
}

#[derive(sqlx::FromRow)]
struct SourceSummaryRow {
    id: String,
    kind: String,
    name: String,
    refresh_state: String,
    last_successful_refresh_at_ms: Option<i64>,
    local_root_id: Option<String>,
    youtube_playlist_id: Option<String>,
    item_count: i64,
    missing_count: i64,
    new_count: i64,
    unreviewed_count: i64,
    unavailable_count: i64,
    ambiguous_count: i64,
    open_issue_count: i64,
    discovery_enabled: i64,
    version: i64,
}

const SOURCE_STALE_AFTER_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

pub async fn source_summaries(
    pool: &SqlitePool,
    now_ms: i64,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicSourceSummary>> {
    validate_summary_window(offset, limit)?;
    if now_ms <= 0 {
        return Err(MusicLibraryError::validation("nowMs", "must be positive"));
    }
    let rows = sqlx::query_as::<_, SourceSummaryRow>(
        "SELECT source.id, source.kind, source.name, source.refresh_state,
                source.last_successful_refresh_at_ms, source.local_root_id,
                source.youtube_playlist_id,
                COUNT(source_item.item_id) AS item_count,
                SUM(CASE WHEN item.availability = 'missing' THEN 1 ELSE 0 END) AS missing_count,
                SUM(CASE WHEN item.review_state = 'unreviewed'
                              AND source_item.first_discovered_at_ms >= COALESCE(source.previous_successful_refresh_at_ms, 0)
                    THEN 1 ELSE 0 END) AS new_count,
                SUM(CASE WHEN item.review_state = 'unreviewed' THEN 1 ELSE 0 END) AS unreviewed_count,
                SUM(CASE WHEN item.availability = 'unavailable' THEN 1 ELSE 0 END) AS unavailable_count,
                SUM(CASE WHEN item.availability = 'ambiguous' THEN 1 ELSE 0 END) AS ambiguous_count,
                ((SELECT COUNT(*) FROM music_refresh_job_issues AS refresh_issue
                  JOIN music_refresh_jobs AS refresh_job ON refresh_job.id = refresh_issue.job_id
                  WHERE refresh_job.source_collection_id = source.id
                    AND refresh_issue.issue_code <> 'metadata-fallback'
                    AND NOT EXISTS (
                        SELECT 1 FROM music_refresh_jobs AS newer
                        WHERE newer.source_collection_id = source.id
                          AND newer.generation > refresh_job.generation
                    )) +
                 (SELECT COUNT(*) FROM music_relink_plan_entries AS relink_entry
                  JOIN music_relink_plans AS relink_plan ON relink_plan.id = relink_entry.plan_id
                  WHERE relink_plan.root_id = source.local_root_id
                    AND relink_plan.state IN ('ready', 'applied')
                    AND relink_entry.match_kind IN ('ambiguous', 'missing')
                    AND relink_entry.resolved_at_ms IS NULL) +
                 SUM(CASE WHEN item.availability IN ('missing', 'unavailable', 'ambiguous')
                     THEN 1 ELSE 0 END)) AS open_issue_count,
                source.discovery_enabled,
                source.version
         FROM music_source_collections AS source
         LEFT JOIN music_source_collection_items AS source_item
           ON source_item.collection_id = source.id
         LEFT JOIN music_library_items AS item ON item.id = source_item.item_id
         GROUP BY source.id
         ORDER BY source.name COLLATE NOCASE, source.id
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music source summaries", error))?;
    rows.into_iter()
        .map(|row| {
            let health = source_health(&row, now_ms)?;
            let discovery_enabled = parse_query_bool(row.discovery_enabled, "discoveryEnabled")?;
            Ok(MusicSourceSummary {
                id: row.id,
                kind: MusicCollectionKind::try_from(row.kind.as_str())
                    .map_err(|message| MusicLibraryError::validation("kind", message))?,
                name: row.name,
                refresh_state: MusicRefreshState::try_from(row.refresh_state.as_str())
                    .map_err(|message| MusicLibraryError::validation("refreshState", message))?,
                last_successful_refresh_at_ms: row.last_successful_refresh_at_ms,
                local_root_id: row.local_root_id,
                youtube_playlist_id: row.youtube_playlist_id,
                item_count: row.item_count,
                missing_count: row.missing_count,
                new_count: row.new_count,
                unreviewed_count: row.unreviewed_count,
                unavailable_count: row.unavailable_count,
                ambiguous_count: row.ambiguous_count,
                open_issue_count: row.open_issue_count,
                health,
                discovery_enabled,
                version: row.version,
            })
        })
        .collect()
}

fn source_health(row: &SourceSummaryRow, now_ms: i64) -> MusicLibraryResult<MusicSourceHealth> {
    if !parse_query_bool(row.discovery_enabled, "discoveryEnabled")? {
        return Ok(MusicSourceHealth::Disabled);
    }
    if row.open_issue_count > 0
        || row.missing_count > 0
        || row.unavailable_count > 0
        || row.ambiguous_count > 0
        || matches!(row.refresh_state.as_str(), "partial" | "failed")
    {
        return Ok(MusicSourceHealth::Issues);
    }
    if row
        .last_successful_refresh_at_ms
        .is_none_or(|refreshed_at| now_ms.saturating_sub(refreshed_at) > SOURCE_STALE_AFTER_MS)
    {
        return Ok(MusicSourceHealth::Stale);
    }
    Ok(MusicSourceHealth::Healthy)
}

fn parse_query_bool(value: i64, field: &str) -> MusicLibraryResult<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(MusicLibraryError::validation(
            field,
            format!("expected 0 or 1, received {value}"),
        )),
    }
}

pub async fn issues(
    pool: &SqlitePool,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicIssue>> {
    validate_summary_window(offset, limit)?;
    let rows = sqlx::query_as::<_, MusicIssueRow>(
        "SELECT id, issue_kind, item_id, playlist_id, collection_id, root_id,
                relative_path, action_required, message, created_at_ms
             FROM (
                SELECT 'availability:' || item.id AS id,
                    CASE
                        WHEN item.youtube_resolution_state = 'embedding-blocked' THEN 'youtube-embedding-blocked'
                        WHEN item.youtube_resolution_state = 'timed-out' THEN 'youtube-timed-out'
                        WHEN item.source_kind = 'youtube-video' THEN 'youtube-unavailable'
                        ELSE 'item-' || item.availability
                    END AS issue_kind,
                    item.id AS item_id, NULL AS playlist_id,
                    (SELECT source_item.collection_id FROM music_source_collection_items AS source_item
                     WHERE source_item.item_id = item.id ORDER BY source_item.collection_id LIMIT 1)
                        AS collection_id,
                    (SELECT location.root_id FROM music_local_locations AS location
                     WHERE location.item_id = item.id ORDER BY location.root_id LIMIT 1) AS root_id,
                    (SELECT location.relative_path FROM music_local_locations AS location
                     WHERE location.item_id = item.id ORDER BY location.root_id, location.relative_path LIMIT 1)
                        AS relative_path,
                    1 AS action_required,
                    CASE item.availability
                        WHEN 'missing' THEN 'The local media location is missing.'
                        WHEN 'ambiguous' THEN 'The media identity needs confirmation.'
                        ELSE 'The online media is unavailable.'
                    END AS message,
                    item.updated_at_ms AS created_at_ms
                FROM music_library_items AS item
                WHERE item.availability IN ('missing', 'ambiguous', 'unavailable')
                UNION ALL
                SELECT refresh_issue.id, refresh_issue.issue_code, refresh_issue.item_id, NULL,
                    refresh_job.source_collection_id, refresh_job.local_root_id,
                    refresh_issue.relative_path,
                    CASE WHEN refresh_issue.issue_code = 'metadata-fallback' THEN 0 ELSE 1 END,
                    refresh_issue.message, refresh_issue.created_at_ms
                FROM music_refresh_job_issues AS refresh_issue
                JOIN music_refresh_jobs AS refresh_job ON refresh_job.id = refresh_issue.job_id
                WHERE NOT EXISTS (
                    SELECT 1 FROM music_refresh_jobs AS newer
                    WHERE newer.source_collection_id = refresh_job.source_collection_id
                      AND newer.generation > refresh_job.generation
                )
                UNION ALL
                SELECT relink_entry.id, 'relink-' || relink_entry.match_kind,
                    relink_entry.suggested_item_id, NULL,
                    (SELECT source.id FROM music_source_collections AS source
                     WHERE source.local_root_id = relink_plan.root_id LIMIT 1),
                    relink_plan.root_id, relink_entry.candidate_relative_path, 1,
                    CASE relink_entry.match_kind
                        WHEN 'ambiguous' THEN 'Several existing tracks could match this replacement file.'
                        ELSE 'An existing track was not found in the replacement folder.'
                    END,
                    relink_entry.created_at_ms
                FROM music_relink_plan_entries AS relink_entry
                JOIN music_relink_plans AS relink_plan ON relink_plan.id = relink_entry.plan_id
                WHERE relink_plan.state IN ('ready', 'applied')
                  AND relink_entry.match_kind IN ('ambiguous', 'missing')
                  AND relink_entry.resolved_at_ms IS NULL
             )
             ORDER BY created_at_ms DESC, id
             LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music issues", error))?;
    rows.into_iter()
        .map(|row| {
            Ok(MusicIssue {
                id: row.id,
                issue_kind: row.issue_kind,
                item_id: row.item_id,
                playlist_id: row.playlist_id,
                collection_id: row.collection_id,
                root_id: row.root_id,
                relative_path: row.relative_path,
                action_required: parse_query_bool(row.action_required, "actionRequired")?,
                message: row.message,
                created_at_ms: row.created_at_ms,
            })
        })
        .collect()
}

pub async fn inspector_detail(
    pool: &SqlitePool,
    item_id: &str,
) -> MusicLibraryResult<MusicInspectorDetail> {
    let item_row = sqlx::query_as::<_, MusicLibraryItemRow>(
        "SELECT id, identity_key, source_kind, media_kind, youtube_video_id,
                original_title, original_artist, original_album, original_track_number,
                original_artwork_identity, youtube_resolution_state, title_override,
                artist_override, album_override, artwork_override, duration_ms,
                availability, review_state, review_changed_at_ms, review_deferred_until_ms, discovered_at_ms,
                updated_at_ms, version
         FROM music_library_items WHERE id = ?",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music inspector item", error))?
    .ok_or_else(|| MusicLibraryError::not_found("music library item", item_id))?;
    let item = MusicLibraryItem::try_from(item_row)?;
    let locations = sqlx::query_as::<_, MusicLocalLocationRow>(
        "SELECT id, item_id, root_id, relative_path, file_size_bytes, modified_at_ms,
                lightweight_fingerprint, strong_fingerprint, availability,
                last_seen_generation, first_seen_at_ms, updated_at_ms
         FROM music_local_locations WHERE item_id = ? ORDER BY root_id, relative_path",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item locations", error))?
    .into_iter()
    .map(MusicLocalLocation::try_from)
    .collect::<MusicLibraryResult<Vec<_>>>()?;
    let memberships = sqlx::query_as::<_, MusicMembershipRow>(
        "SELECT id, playlist_id, item_id, position, weight, enabled,
                start_ms, end_ms, volume, rate, created_at_ms, updated_at_ms, version
         FROM music_playlist_memberships WHERE item_id = ? ORDER BY playlist_id",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item memberships", error))?
    .into_iter()
    .map(MusicPlaylistMembership::try_from)
    .collect::<MusicLibraryResult<Vec<_>>>()?;
    let membership_skip_ranges = sqlx::query_as::<_, (String, String, i64, i64, i64)>(
        "SELECT skip.id, skip.membership_id, skip.start_ms, skip.end_ms, skip.sort_order
         FROM music_membership_skip_ranges AS skip
         JOIN music_playlist_memberships AS membership ON membership.id = skip.membership_id
         WHERE membership.item_id = ?
         ORDER BY skip.membership_id, skip.sort_order",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load membership skip ranges", error))?
    .into_iter()
    .map(
        |(id, membership_id, start_ms, end_ms, sort_order)| MusicMembershipSkipRange {
            id,
            membership_id,
            start_ms,
            end_ms,
            sort_order,
        },
    )
    .collect();
    let snoozes = sqlx::query_as::<_, MusicSnoozeRow>(
        "SELECT id, item_id, scope, playlist_id, starts_at_ms, ends_at_ms, reason, created_at_ms
         FROM music_snoozes WHERE item_id = ? ORDER BY created_at_ms DESC, id",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item snoozes", error))?
    .into_iter()
    .map(MusicSnooze::try_from)
    .collect::<MusicLibraryResult<Vec<_>>>()?;
    let signal_rows: Vec<String> = sqlx::query_scalar(
        "SELECT signal FROM music_item_signals WHERE item_id = ? ORDER BY signal",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item signals", error))?;
    let signals = signal_rows
        .iter()
        .map(|value| {
            MusicItemSignal::try_from(value.as_str())
                .map_err(|message| MusicLibraryError::validation("signal", message))
        })
        .collect::<MusicLibraryResult<Vec<_>>>()?;
    let statistics = sqlx::query_as::<_, MusicStatisticsRow>(
        "SELECT item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms
         FROM music_listening_statistics WHERE item_id = ?",
    )
    .bind(item_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item statistics", error))?
    .map(MusicListeningStatistics::from);
    let source_collection_ids = sqlx::query_scalar(
        "SELECT collection_id FROM music_source_collection_items
         WHERE item_id = ? ORDER BY collection_id",
    )
    .bind(item_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music item sources", error))?;
    Ok(MusicInspectorDetail {
        item,
        locations,
        memberships,
        membership_skip_ranges,
        snoozes,
        signals,
        statistics,
        source_collection_ids,
    })
}

pub async fn local_roots(
    pool: &SqlitePool,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicLocalRoot>> {
    validate_summary_window(offset, limit)?;
    let rows: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT id, name, created_at_ms, updated_at_ms, version
         FROM music_local_roots
         ORDER BY name COLLATE NOCASE, id
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load logical music roots", error))?;
    Ok(rows
        .into_iter()
        .map(
            |(id, name, created_at_ms, updated_at_ms, version)| MusicLocalRoot {
                id,
                name,
                created_at_ms,
                updated_at_ms,
                version,
            },
        )
        .collect())
}

#[derive(sqlx::FromRow)]
struct SourceCollectionRow {
    id: String,
    kind: String,
    identity_key: String,
    name: String,
    local_root_id: Option<String>,
    youtube_playlist_id: Option<String>,
    refresh_state: String,
    last_successful_refresh_at_ms: Option<i64>,
    previous_successful_refresh_at_ms: Option<i64>,
    last_refresh_error_code: Option<String>,
    snapshot_generation: i64,
    created_at_ms: i64,
    updated_at_ms: i64,
    version: i64,
    discovery_enabled: i64,
    removed_at_ms: Option<i64>,
}

pub async fn source_collections(
    pool: &SqlitePool,
    offset: i64,
    limit: i64,
) -> MusicLibraryResult<Vec<MusicSourceCollection>> {
    validate_summary_window(offset, limit)?;
    sqlx::query_as::<_, SourceCollectionRow>(
        "SELECT id, kind, identity_key, name, local_root_id, youtube_playlist_id,
                refresh_state, last_successful_refresh_at_ms, last_refresh_error_code,
                previous_successful_refresh_at_ms, snapshot_generation, created_at_ms,
                updated_at_ms, version, discovery_enabled, removed_at_ms
         FROM music_source_collections
         ORDER BY name COLLATE NOCASE, id
         LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music source collections", error))?
    .into_iter()
    .map(|row| {
        Ok(MusicSourceCollection {
            id: row.id,
            kind: MusicCollectionKind::try_from(row.kind.as_str())
                .map_err(|message| MusicLibraryError::validation("kind", message))?,
            identity_key: row.identity_key,
            name: row.name,
            local_root_id: row.local_root_id,
            youtube_playlist_id: row.youtube_playlist_id,
            refresh_state: MusicRefreshState::try_from(row.refresh_state.as_str())
                .map_err(|message| MusicLibraryError::validation("refreshState", message))?,
            last_successful_refresh_at_ms: row.last_successful_refresh_at_ms,
            previous_successful_refresh_at_ms: row.previous_successful_refresh_at_ms,
            last_refresh_error_code: row.last_refresh_error_code,
            snapshot_generation: row.snapshot_generation,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
            version: row.version,
            discovery_enabled: parse_query_bool(row.discovery_enabled, "discoveryEnabled")?,
            removed_at_ms: row.removed_at_ms,
        })
    })
    .collect()
}

pub async fn playlist_detail(
    pool: &SqlitePool,
    playlist_id: &str,
) -> MusicLibraryResult<MusicPlaylist> {
    let row = sqlx::query_as::<_, MusicPlaylistRow>(
        "SELECT id, name, icon, shuffle_enabled, mix_enabled, repeat_mode, sort_order,
                created_at_ms, updated_at_ms, version
         FROM music_playlists WHERE id = ?",
    )
    .bind(playlist_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load music playlist", error))?
    .ok_or_else(|| MusicLibraryError::not_found("music playlist", playlist_id))?;
    let use_rows: Vec<String> = sqlx::query_scalar(
        "SELECT intended_use FROM music_playlist_intended_uses
         WHERE playlist_id = ? ORDER BY intended_use",
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .map_err(|error| MusicLibraryError::database("load playlist intended uses", error))?;
    let intended_uses = use_rows
        .iter()
        .map(|value| {
            MusicIntendedUse::try_from(value.as_str())
                .map_err(|message| MusicLibraryError::validation("intendedUse", message))
        })
        .collect::<MusicLibraryResult<Vec<_>>>()?;
    row.into_model(intended_uses)
}
