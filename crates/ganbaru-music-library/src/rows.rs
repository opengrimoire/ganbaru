use super::models::*;
use ganbaru_music::error::{MusicLibraryError, MusicLibraryResult};

fn parse_enum<T>(value: &str, field: &str) -> MusicLibraryResult<T>
where
    T: for<'a> TryFrom<&'a str, Error = String>,
{
    T::try_from(value).map_err(|message| MusicLibraryError::validation(field, message))
}

fn parse_bool(value: i64, field: &str) -> MusicLibraryResult<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(MusicLibraryError::validation(
            field,
            format!("expected 0 or 1, received {value}"),
        )),
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicLibraryItemRow {
    pub id: String,
    pub identity_key: String,
    pub source_kind: String,
    pub media_kind: String,
    pub youtube_video_id: Option<String>,
    pub original_title: String,
    pub original_artist: String,
    pub original_album: String,
    pub original_track_number: Option<i64>,
    pub original_artwork_identity: Option<String>,
    pub youtube_resolution_state: Option<String>,
    pub title_override: Option<String>,
    pub artist_override: Option<String>,
    pub album_override: Option<String>,
    pub artwork_override: Option<String>,
    pub duration_ms: Option<i64>,
    pub availability: String,
    pub review_state: String,
    pub review_changed_at_ms: Option<i64>,
    pub review_deferred_until_ms: Option<i64>,
    pub discovered_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

impl TryFrom<MusicLibraryItemRow> for MusicLibraryItem {
    type Error = MusicLibraryError;

    fn try_from(row: MusicLibraryItemRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            id: row.id,
            identity_key: row.identity_key,
            source_kind: parse_enum(&row.source_kind, "sourceKind")?,
            media_kind: parse_enum(&row.media_kind, "mediaKind")?,
            youtube_video_id: row.youtube_video_id,
            original_title: row.original_title,
            original_artist: row.original_artist,
            original_album: row.original_album,
            original_track_number: row.original_track_number,
            original_artwork_identity: row.original_artwork_identity,
            youtube_resolution_state: row
                .youtube_resolution_state
                .as_deref()
                .map(MusicYouTubeResolutionState::try_from)
                .transpose()
                .map_err(|message| {
                    MusicLibraryError::validation("youtubeResolutionState", message)
                })?,
            title_override: row.title_override,
            artist_override: row.artist_override,
            album_override: row.album_override,
            artwork_override: row.artwork_override,
            duration_ms: row.duration_ms,
            availability: parse_enum(&row.availability, "availability")?,
            review_state: parse_enum(&row.review_state, "reviewState")?,
            review_changed_at_ms: row.review_changed_at_ms,
            review_deferred_until_ms: row.review_deferred_until_ms,
            discovered_at_ms: row.discovered_at_ms,
            updated_at_ms: row.updated_at_ms,
            version: row.version,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicPlaylistRow {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: i64,
    pub mix_enabled: i64,
    pub repeat_mode: String,
    pub sort_order: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

impl MusicPlaylistRow {
    pub fn into_model(
        self,
        intended_uses: Vec<MusicIntendedUse>,
    ) -> MusicLibraryResult<MusicPlaylist> {
        Ok(MusicPlaylist {
            id: self.id,
            name: self.name,
            icon: self.icon,
            shuffle_enabled: parse_bool(self.shuffle_enabled, "shuffleEnabled")?,
            mix_enabled: parse_bool(self.mix_enabled, "mixEnabled")?,
            repeat_mode: parse_enum(&self.repeat_mode, "repeatMode")?,
            intended_uses,
            sort_order: self.sort_order,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            version: self.version,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicMembershipRow {
    pub id: String,
    pub playlist_id: String,
    pub item_id: String,
    pub position: i64,
    pub weight: String,
    pub enabled: i64,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

impl TryFrom<MusicMembershipRow> for MusicPlaylistMembership {
    type Error = MusicLibraryError;

    fn try_from(row: MusicMembershipRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            id: row.id,
            playlist_id: row.playlist_id,
            item_id: row.item_id,
            position: row.position,
            weight: parse_enum(&row.weight, "weight")?,
            enabled: parse_bool(row.enabled, "enabled")?,
            start_ms: row.start_ms,
            end_ms: row.end_ms,
            volume: row.volume,
            rate: row.rate,
            created_at_ms: row.created_at_ms,
            updated_at_ms: row.updated_at_ms,
            version: row.version,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicPlaylistPlaybackRow {
    pub membership_id: String,
    pub item_id: String,
    pub identity_key: String,
    pub source_kind: String,
    pub youtube_video_id: Option<String>,
    pub youtube_resolution_state: Option<String>,
    pub title: String,
    pub original_artwork_identity: Option<String>,
    pub artwork_override: Option<String>,
    pub availability: String,
    pub root_id: Option<String>,
    pub relative_path: Option<String>,
    pub position: i64,
    pub weight: String,
    pub enabled: i64,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub snoozed: i64,
    pub snoozed_until: Option<i64>,
    pub snoozed_indefinitely: i64,
}

impl TryFrom<MusicPlaylistPlaybackRow> for MusicPlaylistPlaybackEntry {
    type Error = MusicLibraryError;

    fn try_from(row: MusicPlaylistPlaybackRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            membership_id: row.membership_id,
            item_id: row.item_id,
            identity_key: row.identity_key,
            source_kind: parse_enum(&row.source_kind, "sourceKind")?,
            youtube_video_id: row.youtube_video_id,
            youtube_resolution_state: row
                .youtube_resolution_state
                .as_deref()
                .map(|value| parse_enum(value, "youtubeResolutionState"))
                .transpose()?,
            title: row.title,
            original_artwork_identity: row.original_artwork_identity,
            artwork_override: row.artwork_override,
            availability: parse_enum(&row.availability, "availability")?,
            root_id: row.root_id,
            relative_path: row.relative_path,
            position: row.position,
            weight: parse_enum(&row.weight, "weight")?,
            enabled: parse_bool(row.enabled, "enabled")?,
            start_ms: row.start_ms,
            end_ms: row.end_ms,
            volume: row.volume,
            rate: row.rate,
            snoozed: parse_bool(row.snoozed, "snoozed")?,
            snoozed_until: row.snoozed_until,
            snoozed_indefinitely: parse_bool(row.snoozed_indefinitely, "snoozedIndefinitely")?,
            skip_ranges: Vec::new(),
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicSnoozeRow {
    pub id: String,
    pub item_id: String,
    pub scope: String,
    pub playlist_id: Option<String>,
    pub starts_at_ms: i64,
    pub ends_at_ms: Option<i64>,
    pub reason: String,
    pub created_at_ms: i64,
}

impl TryFrom<MusicSnoozeRow> for MusicSnooze {
    type Error = MusicLibraryError;

    fn try_from(row: MusicSnoozeRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            id: row.id,
            item_id: row.item_id,
            scope: parse_enum(&row.scope, "scope")?,
            playlist_id: row.playlist_id,
            starts_at_ms: row.starts_at_ms,
            ends_at_ms: row.ends_at_ms,
            reason: row.reason,
            created_at_ms: row.created_at_ms,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicItemListRow {
    pub id: String,
    pub identity_key: String,
    pub source_kind: String,
    pub media_kind: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub local_root_id: Option<String>,
    pub relative_path: Option<String>,
    pub source_collection_ids_json: String,
    pub original_artwork_identity: Option<String>,
    pub artwork_override: Option<String>,
    pub duration_ms: Option<i64>,
    pub availability: String,
    pub review_state: String,
    pub discovered_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
    pub playlist_count: i64,
    pub active_snooze_count: i64,
    pub last_played_at_ms: Option<i64>,
    pub play_count: i64,
    pub membership_id: Option<String>,
    pub membership_position: Option<i64>,
    pub membership_weight: Option<String>,
    pub membership_enabled: Option<i64>,
    pub membership_version: Option<i64>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicIssueRow {
    pub id: String,
    pub issue_kind: String,
    pub item_id: Option<String>,
    pub playlist_id: Option<String>,
    pub collection_id: Option<String>,
    pub root_id: Option<String>,
    pub relative_path: Option<String>,
    pub action_required: i64,
    pub message: String,
    pub created_at_ms: i64,
}

impl TryFrom<MusicItemListRow> for MusicItemListEntry {
    type Error = MusicLibraryError;

    fn try_from(row: MusicItemListRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            id: row.id,
            identity_key: row.identity_key,
            source_kind: parse_enum(&row.source_kind, "sourceKind")?,
            media_kind: parse_enum(&row.media_kind, "mediaKind")?,
            title: row.title,
            artist: row.artist,
            album: row.album,
            local_root_id: row.local_root_id,
            relative_path: row.relative_path,
            source_collection_ids: serde_json::from_str(&row.source_collection_ids_json).map_err(
                |error| {
                    MusicLibraryError::runtime(
                        "decode item source collection ids",
                        error.to_string(),
                    )
                },
            )?,
            original_artwork_identity: row.original_artwork_identity,
            artwork_override: row.artwork_override,
            duration_ms: row.duration_ms,
            availability: parse_enum(&row.availability, "availability")?,
            review_state: parse_enum(&row.review_state, "reviewState")?,
            discovered_at_ms: row.discovered_at_ms,
            updated_at_ms: row.updated_at_ms,
            version: row.version,
            playlist_count: row.playlist_count,
            active_snooze_count: row.active_snooze_count,
            last_played_at_ms: row.last_played_at_ms,
            play_count: row.play_count,
            membership_id: row.membership_id,
            membership_position: row.membership_position,
            membership_weight: row
                .membership_weight
                .as_deref()
                .map(|value| parse_enum(value, "membershipWeight"))
                .transpose()?,
            membership_enabled: row
                .membership_enabled
                .map(|value| parse_bool(value, "membershipEnabled"))
                .transpose()?,
            membership_version: row.membership_version,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicLocalLocationRow {
    pub id: String,
    pub item_id: String,
    pub root_id: String,
    pub relative_path: String,
    pub file_size_bytes: Option<i64>,
    pub modified_at_ms: Option<i64>,
    pub lightweight_fingerprint: Option<String>,
    pub strong_fingerprint: Option<String>,
    pub availability: String,
    pub last_seen_generation: Option<i64>,
    pub first_seen_at_ms: i64,
    pub updated_at_ms: i64,
}

impl TryFrom<MusicLocalLocationRow> for MusicLocalLocation {
    type Error = MusicLibraryError;

    fn try_from(row: MusicLocalLocationRow) -> MusicLibraryResult<Self> {
        Ok(Self {
            id: row.id,
            item_id: row.item_id,
            root_id: row.root_id,
            relative_path: row.relative_path,
            file_size_bytes: row.file_size_bytes,
            modified_at_ms: row.modified_at_ms,
            lightweight_fingerprint: row.lightweight_fingerprint,
            strong_fingerprint: row.strong_fingerprint,
            availability: parse_enum(&row.availability, "locationAvailability")?,
            last_seen_generation: row.last_seen_generation,
            first_seen_at_ms: row.first_seen_at_ms,
            updated_at_ms: row.updated_at_ms,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(crate) struct MusicStatisticsRow {
    pub item_id: String,
    pub last_played_at_ms: Option<i64>,
    pub play_count: i64,
    pub completion_count: i64,
    pub skip_count: i64,
    pub updated_at_ms: i64,
}

impl From<MusicStatisticsRow> for MusicListeningStatistics {
    fn from(row: MusicStatisticsRow) -> Self {
        Self {
            item_id: row.item_id,
            last_played_at_ms: row.last_played_at_ms,
            play_count: row.play_count,
            completion_count: row.completion_count,
            skip_count: row.skip_count,
            updated_at_ms: row.updated_at_ms,
        }
    }
}
