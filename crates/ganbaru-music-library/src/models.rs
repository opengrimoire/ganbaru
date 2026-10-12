use serde::{Deserialize, Serialize};

#[cfg(test)]
pub use ganbaru_music::assignments::{
    MusicActivityPhase, MusicAssignmentBehavior, MusicAssignmentProvenanceKind,
    MusicContextAssignmentDraft, MusicSoundscapeBehavior,
};
pub use ganbaru_music::assignments::{
    MusicAssignmentOwnerKind, MusicContextAssignment, MusicContextAssignmentSet,
};

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
        pub enum $name {
            $(#[serde(rename = $value)] $variant),+
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = String;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(format!("unknown {} '{value}'", stringify!($name))),
                }
            }
        }
    };
}

string_enum!(MusicLibrarySourceKind {
    LocalFile => "local-file",
    YouTubeVideo => "youtube-video",
});
string_enum!(MusicMediaKind {
    Audio => "audio",
    Video => "video",
    Unknown => "unknown",
});
string_enum!(MusicReviewState {
    Unreviewed => "unreviewed",
    Reviewed => "reviewed",
    Deferred => "deferred",
    Ignored => "ignored",
});
string_enum!(MusicItemAvailability {
    Available => "available",
    Missing => "missing",
    Unavailable => "unavailable",
    Ambiguous => "ambiguous",
    Unknown => "unknown",
});
string_enum!(MusicLocationAvailability {
    Available => "available",
    Missing => "missing",
    Ambiguous => "ambiguous",
    Unsupported => "unsupported",
    Unknown => "unknown",
});
string_enum!(MusicCollectionKind {
    LocalRoot => "local-root",
    YouTubePlaylist => "youtube-playlist",
});
string_enum!(MusicRefreshState {
    Idle => "idle",
    Queued => "queued",
    Running => "running",
    Partial => "partial",
    Failed => "failed",
});
string_enum!(MusicRefreshJobState {
    Queued => "queued",
    Running => "running",
    Completed => "completed",
    Partial => "partial",
    Failed => "failed",
    Cancelled => "cancelled",
});
string_enum!(MusicSourceHealth {
    Healthy => "healthy",
    Stale => "stale",
    Issues => "issues",
    Disabled => "disabled",
});
#[cfg(desktop)]
string_enum!(MusicRelinkPlanState {
    Planning => "planning",
    Ready => "ready",
    Applied => "applied",
    Cancelled => "cancelled",
});
#[cfg(desktop)]
string_enum!(MusicRelinkMatchKind {
    Exact => "exact",
    Likely => "likely",
    Ambiguous => "ambiguous",
    Missing => "missing",
    New => "new",
});
#[cfg(desktop)]
string_enum!(MusicRepairMatchStrength {
    Exact => "exact",
    Likely => "likely",
    Weak => "weak",
});
string_enum!(MusicYouTubeResolutionState {
    Resolving => "resolving",
    Ready => "ready",
    Unavailable => "unavailable",
    EmbeddingBlocked => "embedding-blocked",
    TimedOut => "timed-out",
});
string_enum!(MusicWeight {
    Rarely => "rarely",
    LessOften => "less-often",
    Normal => "normal",
    MoreOften => "more-often",
    MuchMoreOften => "much-more-often",
});
string_enum!(MusicIntendedUse {
    General => "general",
    Focus => "focus",
    Reading => "reading",
    Relaxation => "relaxation",
    Energizing => "energizing",
});
#[cfg(desktop)]
string_enum!(MusicSoundscapeSourceKind {
    GeneratedNoise => "generated-noise",
    LocalLoop => "local-loop",
    BundledLoop => "bundled-loop",
});
#[cfg(desktop)]
string_enum!(MusicGeneratedNoiseKind {
    White => "white",
    Pink => "pink",
    Brown => "brown",
});
#[cfg(desktop)]
string_enum!(MusicSoundscapeAvailability {
    Available => "available",
    Missing => "missing",
    Unsupported => "unsupported",
});
string_enum!(MusicItemSignal {
    Lyrics => "lyrics",
    SuddenChanges => "sudden-changes",
    HighIntensity => "high-intensity",
    Calm => "calm",
    Repetitive => "repetitive",
    Energizing => "energizing",
});
string_enum!(MusicSnoozeScope {
    Playlist => "playlist",
    AllPlaylists => "all-playlists",
});
string_enum!(MusicRepeatMode {
    Off => "off",
    All => "all",
    One => "one",
});
string_enum!(MusicPlaylistAssignmentKind {
    ProjectFocus => "project-focus",
    ProjectBreak => "project-break",
    CalendarEvent => "calendar-event",
    ContextAssignment => "context-assignment",
});
string_enum!(MusicListDestination {
    Review => "review",
    Library => "library",
    Playlist => "playlist",
});
string_enum!(MusicItemSort {
    Title => "title",
    Artist => "artist",
    Album => "album",
    SourceOrder => "source-order",
    DiscoveredAt => "discovered-at",
    AddedToPlaylist => "added-to-playlist",
    LastPlayedAt => "last-played-at",
    PlayCount => "play-count",
    ManualPosition => "manual-position",
});
string_enum!(MusicSortDirection {
    Ascending => "ascending",
    Descending => "descending",
});
string_enum!(MusicGroupBy {
    None => "none",
    SourceKind => "source-kind",
    ReviewState => "review-state",
    Availability => "availability",
    Album => "album",
    Folder => "folder",
    SourceCollection => "source-collection",
});

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLibraryItem {
    pub id: String,
    pub identity_key: String,
    pub source_kind: MusicLibrarySourceKind,
    pub media_kind: MusicMediaKind,
    pub youtube_video_id: Option<String>,
    pub original_title: String,
    pub original_artist: String,
    pub original_album: String,
    pub original_track_number: Option<i64>,
    pub original_artwork_identity: Option<String>,
    pub youtube_resolution_state: Option<MusicYouTubeResolutionState>,
    pub title_override: Option<String>,
    pub artist_override: Option<String>,
    pub album_override: Option<String>,
    pub artwork_override: Option<String>,
    pub duration_ms: Option<i64>,
    pub availability: MusicItemAvailability,
    pub review_state: MusicReviewState,
    pub review_changed_at_ms: Option<i64>,
    pub review_deferred_until_ms: Option<i64>,
    pub discovered_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLocalRoot {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLocalLocation {
    pub id: String,
    pub item_id: String,
    pub root_id: String,
    pub relative_path: String,
    pub file_size_bytes: Option<i64>,
    pub modified_at_ms: Option<i64>,
    pub lightweight_fingerprint: Option<String>,
    pub strong_fingerprint: Option<String>,
    pub availability: MusicLocationAvailability,
    pub last_seen_generation: Option<i64>,
    pub first_seen_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceCollection {
    pub id: String,
    pub kind: MusicCollectionKind,
    pub identity_key: String,
    pub name: String,
    pub local_root_id: Option<String>,
    pub youtube_playlist_id: Option<String>,
    pub refresh_state: MusicRefreshState,
    pub last_successful_refresh_at_ms: Option<i64>,
    pub previous_successful_refresh_at_ms: Option<i64>,
    pub last_refresh_error_code: Option<String>,
    pub snapshot_generation: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
    pub discovery_enabled: bool,
    pub removed_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylist {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: bool,
    pub mix_enabled: bool,
    pub repeat_mode: MusicRepeatMode,
    pub intended_uses: Vec<MusicIntendedUse>,
    pub sort_order: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistMembership {
    pub id: String,
    pub playlist_id: String,
    pub item_id: String,
    pub position: i64,
    pub weight: MusicWeight,
    pub enabled: bool,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSnooze {
    pub id: String,
    pub item_id: String,
    pub scope: MusicSnoozeScope,
    pub playlist_id: Option<String>,
    pub starts_at_ms: i64,
    pub ends_at_ms: Option<i64>,
    pub reason: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicListeningStatistics {
    pub item_id: String,
    pub last_played_at_ms: Option<i64>,
    pub play_count: i64,
    pub completion_count: i64,
    pub skip_count: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLocalRefreshRequest {
    pub job_id: String,
    pub root_id: String,
    pub collection_id: String,
    pub folder_path: String,
    pub available_roots: Vec<MusicAvailableRootPath>,
    pub requested_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicAvailableRootPath {
    pub root_id: String,
    pub folder_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeVideoWrite {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub duration_ms: Option<i64>,
    pub resolution_state: MusicYouTubeResolutionState,
    pub resolved_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubePlaylistSnapshotWrite {
    pub collection_id: String,
    pub playlist_id: String,
    pub name: String,
    pub video_ids: Vec<String>,
    #[serde(default)]
    pub videos: Vec<MusicYouTubePlaylistVideoWrite>,
    pub resolved_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubePlaylistVideoWrite {
    pub video_id: String,
    pub title: String,
    pub channel: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeSourceFailureWrite {
    pub collection_id: String,
    pub playlist_id: String,
    pub name: String,
    pub resolution_state: MusicYouTubeResolutionState,
    pub error_code: String,
    pub occurred_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicYouTubeSnapshotResult {
    pub collection_id: String,
    pub canonical_item_count: i64,
    pub newly_discovered_count: i64,
    pub repeated_video_count: i64,
    pub generation: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRefreshJobProgress {
    pub job_id: String,
    pub collection_id: String,
    pub root_id: Option<String>,
    pub kind: MusicCollectionKind,
    pub state: MusicRefreshJobState,
    pub generation: i64,
    pub discovered_count: i64,
    pub processed_count: i64,
    pub skipped_count: i64,
    pub issue_count: i64,
    pub truncated_count: i64,
    pub absence_determined: bool,
    pub status_message: String,
    pub requested_at_ms: i64,
    pub started_at_ms: Option<i64>,
    pub finished_at_ms: Option<i64>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLocalRootCreate {
    pub root_id: String,
    pub collection_id: String,
    pub identity_key: String,
    pub name: String,
    pub created_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemRepairPreview {
    pub item_id: String,
    pub folder_path: String,
    pub relative_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<i64>,
    pub file_size_bytes: i64,
    pub lightweight_fingerprint: String,
    pub strong_fingerprint: String,
    pub match_strength: MusicRepairMatchStrength,
    pub reasons: Vec<String>,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemRepairApply {
    pub item_id: String,
    pub root_id: String,
    pub location_id: String,
    pub root_name: String,
    pub folder_path: String,
    pub relative_path: String,
    pub expected_strong_fingerprint: String,
    pub accept_weak_mismatch: bool,
    pub applied_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCollectionWrite {
    pub id: String,
    pub kind: MusicCollectionKind,
    pub identity_key: String,
    pub name: String,
    pub local_root_id: Option<String>,
    pub youtube_playlist_id: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistCreate {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: bool,
    #[serde(default)]
    pub mix_enabled: bool,
    pub repeat_mode: MusicRepeatMode,
    pub intended_uses: Vec<MusicIntendedUse>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistUpdate {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: bool,
    #[serde(default)]
    pub mix_enabled: bool,
    pub repeat_mode: MusicRepeatMode,
    pub intended_uses: Vec<MusicIntendedUse>,
    pub expected_version: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistOrderEntry {
    pub playlist_id: String,
    pub expected_version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistsReorder {
    pub playlists: Vec<MusicPlaylistOrderEntry>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMembershipWrite {
    pub id: String,
    pub playlist_id: String,
    pub item_id: String,
    pub position: i64,
    pub weight: MusicWeight,
    pub enabled: bool,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub expected_version: Option<i64>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicBulkMembershipWrite {
    pub memberships: Vec<MusicMembershipWrite>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicBulkMembershipEdit {
    pub action_id: String,
    pub item_ids: Vec<String>,
    pub add_playlist_ids: Vec<String>,
    pub remove_playlist_ids: Vec<String>,
    pub weight_playlist_ids: Vec<String>,
    pub weight: Option<MusicWeight>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicBulkMembershipResult {
    pub changed_count: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMembershipMatrixEntry {
    pub item_id: String,
    pub playlist_id: String,
    pub weight: MusicWeight,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistReorder {
    pub playlist_id: String,
    pub item_id: String,
    pub target_index: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistReorderResult {
    pub item_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistPlaybackEntry {
    pub membership_id: String,
    pub item_id: String,
    pub identity_key: String,
    pub source_kind: MusicLibrarySourceKind,
    pub youtube_video_id: Option<String>,
    pub youtube_resolution_state: Option<MusicYouTubeResolutionState>,
    pub title: String,
    pub original_artwork_identity: Option<String>,
    pub artwork_override: Option<String>,
    pub availability: MusicItemAvailability,
    pub root_id: Option<String>,
    pub relative_path: Option<String>,
    pub position: i64,
    pub weight: MusicWeight,
    pub enabled: bool,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub snoozed: bool,
    pub snoozed_until: Option<i64>,
    pub snoozed_indefinitely: bool,
    pub skip_ranges: Vec<MusicMembershipSkipRange>,
}

string_enum!(MusicSelectionKind {
    Automatic => "automatic",
    Manual => "manual",
});

string_enum!(MusicListeningOutcome {
    Started => "started",
    Completed => "completed",
    Skipped => "skipped",
});

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicListeningUpdate {
    pub playlist_id: Option<String>,
    pub item_id: String,
    pub selection_kind: MusicSelectionKind,
    pub outcome: MusicListeningOutcome,
    pub occurred_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(test)]
pub struct MusicRecentSelection {
    pub item_id: String,
    pub selected_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeDefinition {
    pub id: String,
    pub source_kind: MusicSoundscapeSourceKind,
    pub generated_kind: Option<MusicGeneratedNoiseKind>,
    pub bundled_identity: Option<String>,
    pub name: String,
    pub icon: String,
    pub group_id: Option<String>,
    pub availability: MusicSoundscapeAvailability,
    pub local_path: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeWrite {
    pub id: String,
    pub source_kind: MusicSoundscapeSourceKind,
    pub generated_kind: Option<MusicGeneratedNoiseKind>,
    pub bundled_identity: Option<String>,
    pub name: String,
    pub icon: String,
    pub group_id: Option<String>,
    pub device_id: String,
    pub local_path: Option<String>,
    pub expected_version: Option<i64>,
    pub updated_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeGroup {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeGroupWrite {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub expected_version: Option<i64>,
    pub updated_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeState {
    /// An automatic selection requires fresh native phase admission after restart.
    pub automatic_intent: bool,
    pub active_soundscape_id: Option<String>,
    pub active_ids: Vec<String>,
    pub multiple_enabled: bool,
    pub generated_level: Option<f64>,
    pub local_level: Option<f64>,
    pub desired_playing: bool,
    pub volume: f64,
    pub updated_at_ms: i64,
    pub version: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSoundscapeStateWrite {
    pub active_soundscape_id: Option<String>,
    pub active_ids: Vec<String>,
    pub multiple_enabled: bool,
    pub generated_level: Option<f64>,
    pub local_level: Option<f64>,
    pub desired_playing: bool,
    pub volume: f64,
    pub expected_version: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicVersionedItem {
    pub item_id: String,
    pub expected_version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicBulkReviewWrite {
    pub items: Vec<MusicVersionedItem>,
    pub review_state: MusicReviewState,
    pub deferred_until: Option<i64>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicReviewSelectionWrite {
    pub action_id: String,
    pub items: Vec<MusicVersionedItem>,
    pub review_state: MusicReviewState,
    pub add_playlist_ids: Vec<String>,
    pub remove_playlist_ids: Vec<String>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicReviewSelectionResult {
    pub membership_changed_count: i64,
    pub review_changed_count: i64,
    pub items: Vec<MusicWriteReceipt>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicBulkSnoozeWrite {
    pub action_id: String,
    pub item_ids: Vec<String>,
    pub scope: MusicSnoozeScope,
    pub playlist_id: Option<String>,
    pub starts_at_ms: i64,
    pub ends_at_ms: Option<i64>,
    pub reason: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicWriteReceipt {
    pub id: String,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistDuplicate {
    pub source_playlist_id: String,
    pub new_playlist_id: String,
    pub name: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistDeleteImpact {
    pub membership_count: i64,
    pub project_focus_assignment_count: i64,
    pub project_break_assignment_count: i64,
    pub calendar_assignment_count: i64,
    pub context_assignment_count: i64,
    pub assignments: Vec<MusicPlaylistAssignmentReference>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistAssignmentReference {
    pub kind: MusicPlaylistAssignmentKind,
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistDelete {
    pub playlist_id: String,
    pub replacement_playlist_id: Option<String>,
    pub expected_version: i64,
    pub expected_impact: MusicPlaylistDeleteImpact,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicReviewWrite {
    pub item_id: String,
    pub review_state: MusicReviewState,
    pub deferred_until: Option<i64>,
    pub expected_version: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMetadataOverrideWrite {
    pub item_id: String,
    pub title_override: Option<String>,
    pub artist_override: Option<String>,
    pub album_override: Option<String>,
    pub artwork_override: Option<String>,
    pub expected_version: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemSignalsWrite {
    pub item_ids: Vec<String>,
    pub signals: Vec<MusicItemSignal>,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMembershipRemove {
    pub membership_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicMembershipSkipRange {
    pub id: String,
    pub membership_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicAdvancedMembershipWrite {
    pub membership: MusicMembershipWrite,
    pub skip_ranges: Vec<MusicMembershipSkipRange>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSnoozeRemove {
    pub snooze_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemWindowRequest {
    pub destination: MusicListDestination,
    pub playlist_id: Option<String>,
    pub search: String,
    pub source_kind: Option<MusicLibrarySourceKind>,
    pub availability: Option<MusicItemAvailability>,
    pub review_state: Option<MusicReviewState>,
    pub source_collection_id: Option<String>,
    pub membership_playlist_id: Option<String>,
    pub snoozed: Option<bool>,
    pub sort: MusicItemSort,
    pub direction: MusicSortDirection,
    pub group_by: MusicGroupBy,
    pub now_ms: i64,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemListEntry {
    pub id: String,
    pub identity_key: String,
    pub source_kind: MusicLibrarySourceKind,
    pub media_kind: MusicMediaKind,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub local_root_id: Option<String>,
    pub relative_path: Option<String>,
    pub source_collection_ids: Vec<String>,
    pub original_artwork_identity: Option<String>,
    pub artwork_override: Option<String>,
    pub duration_ms: Option<i64>,
    pub availability: MusicItemAvailability,
    pub review_state: MusicReviewState,
    pub discovered_at_ms: i64,
    pub updated_at_ms: i64,
    pub version: i64,
    pub playlist_count: i64,
    pub active_snooze_count: i64,
    pub last_played_at_ms: Option<i64>,
    pub play_count: i64,
    pub membership_id: Option<String>,
    pub membership_position: Option<i64>,
    pub membership_weight: Option<MusicWeight>,
    pub membership_enabled: Option<bool>,
    pub membership_version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicGroupCount {
    pub key: String,
    pub count: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItemWindow {
    pub items: Vec<MusicItemListEntry>,
    pub groups: Vec<MusicGroupCount>,
    pub total_count: i64,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicPlaylistSummary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: bool,
    pub mix_enabled: bool,
    pub repeat_mode: MusicRepeatMode,
    pub intended_uses: Vec<MusicIntendedUse>,
    pub sort_order: i64,
    pub total_count: i64,
    pub eligible_count: i64,
    pub unavailable_count: i64,
    pub snoozed_count: i64,
    pub local_count: i64,
    pub online_count: i64,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceSummary {
    pub id: String,
    pub kind: MusicCollectionKind,
    pub name: String,
    pub refresh_state: MusicRefreshState,
    pub last_successful_refresh_at_ms: Option<i64>,
    pub local_root_id: Option<String>,
    pub youtube_playlist_id: Option<String>,
    pub item_count: i64,
    pub missing_count: i64,
    pub new_count: i64,
    pub unreviewed_count: i64,
    pub unavailable_count: i64,
    pub ambiguous_count: i64,
    pub open_issue_count: i64,
    pub health: MusicSourceHealth,
    pub discovery_enabled: bool,
    pub version: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicIssue {
    pub id: String,
    pub issue_kind: String,
    pub item_id: Option<String>,
    pub playlist_id: Option<String>,
    pub collection_id: Option<String>,
    pub root_id: Option<String>,
    pub relative_path: Option<String>,
    pub action_required: bool,
    pub message: String,
    pub created_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkPlanRequest {
    pub plan_id: String,
    pub root_id: String,
    pub replacement_folder_path: String,
    pub created_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkPlanSummary {
    pub id: String,
    pub root_id: String,
    pub state: MusicRelinkPlanState,
    pub exact_count: i64,
    pub likely_count: i64,
    pub ambiguous_count: i64,
    pub missing_count: i64,
    pub new_count: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkPlanEntry {
    pub id: String,
    pub match_kind: MusicRelinkMatchKind,
    pub old_location_id: Option<String>,
    pub suggested_item_id: Option<String>,
    pub candidate_relative_path: Option<String>,
    pub candidate_item_ids: Vec<String>,
    pub file_size_bytes: Option<i64>,
    pub resolved_item_id: Option<String>,
    pub resolved_at_ms: Option<i64>,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkPlanWindow {
    pub entries: Vec<MusicRelinkPlanEntry>,
    pub total_count: i64,
    pub offset: i64,
    pub limit: i64,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkDecision {
    pub entry_id: String,
    pub item_id: String,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicRelinkApplyRequest {
    pub plan_id: String,
    pub decisions: Vec<MusicRelinkDecision>,
    pub applied_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceRemovalImpact {
    pub collection_id: String,
    pub item_count: i64,
    pub membership_count: i64,
    pub shared_item_count: i64,
    pub orphaned_item_count: i64,
    pub active_refresh_count: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceRemovalRequest {
    pub collection_id: String,
    pub expected_version: i64,
    pub expected_impact: MusicSourceRemovalImpact,
    pub remove_orphaned_items: bool,
    pub removed_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInspectorDetail {
    pub item: MusicLibraryItem,
    pub locations: Vec<MusicLocalLocation>,
    pub memberships: Vec<MusicPlaylistMembership>,
    pub membership_skip_ranges: Vec<MusicMembershipSkipRange>,
    pub snoozes: Vec<MusicSnooze>,
    pub signals: Vec<MusicItemSignal>,
    pub statistics: Option<MusicListeningStatistics>,
    pub source_collection_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSearchRebuildResult {
    pub indexed_item_count: i64,
    pub schema_version: i64,
    pub fingerprint: String,
    pub rebuilt_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeLocation {
    pub root_id: String,
    pub relative_path: String,
    pub availability: MusicLocationAvailability,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeItem {
    pub identity_key: String,
    pub source_kind: MusicLibrarySourceKind,
    pub youtube_video_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<i64>,
    pub signals: Vec<MusicItemSignal>,
    pub locations: Vec<MusicInterchangeLocation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeRange {
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeSnooze {
    pub scope: MusicSnoozeScope,
    pub starts_at_ms: i64,
    pub ends_at_ms: Option<i64>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeMembership {
    pub item: MusicInterchangeItem,
    pub position: i64,
    pub weight: MusicWeight,
    pub enabled: bool,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
    pub skip_ranges: Vec<MusicInterchangeRange>,
    pub snoozes: Vec<MusicInterchangeSnooze>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangePlaylist {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub shuffle_enabled: bool,
    pub mix_enabled: bool,
    pub repeat_mode: MusicRepeatMode,
    pub intended_uses: Vec<MusicIntendedUse>,
    pub memberships: Vec<MusicInterchangeMembership>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeRoot {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeDocument {
    pub format: String,
    pub version: i64,
    pub exported_at: i64,
    pub roots: Vec<MusicInterchangeRoot>,
    pub playlists: Vec<MusicInterchangePlaylist>,
    pub context_assignments: Vec<MusicContextAssignment>,
    pub warnings: Vec<String>,
}

string_enum!(MusicImportPlaylistConflict {
    KeepExisting => "keep-existing",
    ImportCopy => "import-copy",
    ReplaceExisting => "replace-existing",
});

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeImportRequest {
    pub document: MusicInterchangeDocument,
    pub playlist_conflict: MusicImportPlaylistConflict,
    pub replace_item_descriptions: bool,
    pub import_context_assignments: bool,
    pub imported_at: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicInterchangeImportResult {
    pub playlist_count: i64,
    pub item_count: i64,
    pub membership_count: i64,
    pub assignment_count: i64,
}
