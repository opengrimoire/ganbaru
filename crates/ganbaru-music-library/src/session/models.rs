//! Typed application session commands and projections shared by native media adapters.

use crate::{MusicItemAvailability, MusicRepeatMode, MusicWeight};
use serde::{Deserialize, Serialize};

pub const MAX_QUEUE_ENTRIES: usize = 10_000;
pub const MAX_HISTORY: usize = 512;
pub const MAX_RECENT: usize = 5;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    LocalFile,
    YoutubeVideo,
    YoutubePlaylist,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSource {
    pub kind: SourceKind,
    pub identity: String,
    pub original_input: String,
    pub title: String,
    /// Display artist for media metadata; `None` leaves the decoder's file tags in place.
    pub artist: Option<String>,
    /// Display album for media metadata; `None` leaves the decoder's file tags in place.
    pub album: Option<String>,
    pub path: Option<String>,
    pub artwork_path: Option<String>,
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionBackend {
    NativeAudio,
    Browser,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Idle,
    Loading,
    Ready,
    Playing,
    Paused,
    Ended,
    Error,
}

impl SessionStatus {
    /// Returns the stable transport label shared with operating-system adapters.
    #[cfg(desktop)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ready => "ready",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Ended => "ended",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlaybackOrder {
    InOrder,
    Shuffle,
    Mix,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionOwner {
    Manual,
    Review,
    CalendarEvent,
    Pomodoro,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    Disabled,
    Snoozed,
    Offline,
    Unavailable,
    EmbeddingBlocked,
    PhaseConstraint,
    UnboundRoot,
    InvalidSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionIssue {
    BrowserHostUnavailable,
    NoEligibleItems,
    SourceFailure,
    PersistenceFailure,
    Interrupted,
    /// The checkpoint was restored paused at startup. This is expected and not shown to the
    /// user, but the first explicit Play reloads the selection like an interruption.
    Restored,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSkipRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionQueueEntry {
    pub item_id: Option<String>,
    pub membership_id: Option<String>,
    pub source: SessionSource,
    pub backend: SessionBackend,
    pub availability: MusicItemAvailability,
    pub enabled: bool,
    pub weight: MusicWeight,
    pub snoozed_until: Option<i64>,
    pub snoozed_indefinitely: bool,
    pub embedding_blocked: bool,
    pub bound: bool,
    pub phase_allowed: bool,
    pub skip_ranges: Vec<SessionSkipRange>,
    pub volume: Option<f64>,
    pub rate: Option<f64>,
}

/// Browser adapters report observed media state; they never decide queue progression.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionObservation {
    pub session_id: String,
    pub generation: u64,
    pub sequence: u64,
    pub source_identity: String,
    pub status: SessionStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SessionIntent {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    Select { index: usize },
    Seek { position_ms: u64 },
    SeekBy { delta_ms: i64 },
    Volume { volume: f64 },
    Muted { muted: bool },
    Rate { rate: f64 },
    Order { order: PlaybackOrder },
    Online { online: bool },
    BrowserHost { available: bool },
    Observe { observation: SessionObservation },
    Refresh,
    RetryContext,
    SuspendReview,
    RestoreReview { checkpoint_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCommand {
    pub action_id: String,
    pub session_id: Option<String>,
    pub intent: SessionIntent,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SessionQueueIntent {
    SavedPlaylist {
        playlist_id: String,
        explicit_item_id: Option<String>,
        avoid_item_id: Option<String>,
    },
    LibraryItems {
        item_ids: Vec<String>,
        selected_item_id: Option<String>,
        name: String,
    },
    Sources {
        sources: Vec<SessionSource>,
        selected_index: Option<usize>,
        name: String,
    },
    ReviewItem {
        item_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionStart {
    pub action_id: String,
    pub queue: SessionQueueIntent,
    pub autoplay: bool,
    pub resume: bool,
    pub order: PlaybackOrder,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
}

/// A generation-tagged adapter effect is emitted only after its transition commits.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SessionEffect {
    Load {
        session_id: String,
        generation: u64,
        source: Box<SessionSource>,
        backend: SessionBackend,
        position_ms: u64,
        autoplay: bool,
        volume: f64,
        muted: bool,
        rate: f64,
    },
    Play {
        generation: u64,
    },
    Pause {
        generation: u64,
    },
    Stop {
        generation: u64,
    },
    Seek {
        generation: u64,
        position_ms: u64,
    },
    Settings {
        generation: u64,
        volume: f64,
        muted: bool,
        rate: f64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionProjection {
    #[serde(default)]
    pub soundscape_version: Option<i64>,
    pub session_id: String,
    pub revision: u64,
    pub generation: u64,
    pub queue_revision: u64,
    pub current_index: Option<usize>,
    pub current_source: Option<SessionSource>,
    pub backend: Option<SessionBackend>,
    pub status: SessionStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub order: PlaybackOrder,
    pub repeat_mode: MusicRepeatMode,
    pub owner: SessionOwner,
    pub context: Option<SessionContext>,
    pub playlist_id: Option<String>,
    pub queue_name: String,
    pub can_previous: bool,
    pub can_next: bool,
    pub issue: Option<SessionIssue>,
    pub error: Option<String>,
    pub review_checkpoint_id: Option<String>,
    pub queue: Option<Vec<SessionQueueEntry>>,
}

/// Typed failure context from the private Android service channel.
#[cfg(any(target_os = "android", test))]
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AndroidInterruption {
    ServiceStopped,
    SourceAuthorityChanged,
    SourceResolutionTimeout,
}

#[cfg(any(target_os = "android", test))]
impl AndroidInterruption {
    /// Backend error context distinguishes source delivery from service termination.
    pub fn message(self) -> &'static str {
        match self {
            Self::ServiceStopped => "Android media service stopped",
            Self::SourceAuthorityChanged => "Android Music source authority changed before loading",
            Self::SourceResolutionTimeout => {
                "Android Music document resolution exceeded its deadline"
            }
        }
    }
}

/// Canonical assignment provenance; localized display strings belong to the frontend.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionContext {
    pub activation_key: String,
    pub event_id: String,
    pub event_title: String,
    pub phase: ganbaru_music::assignments::MusicActivityPhase,
    pub behavior: ganbaru_music::assignments::MusicAssignmentBehavior,
    pub assignment_source: String,
    pub playlist_id: Option<String>,
    pub state: String,
    pub issue: Option<String>,
}

/// Device-scoped state stores portable queue references; resolved paths remain runtime data.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionCheckpoint {
    pub session_id: String,
    pub revision: u64,
    pub generation: u64,
    pub queue: Option<SessionQueueIntent>,
    pub selected_entry_id: Option<String>,
    pub history: Vec<String>,
    pub recent_entry_ids: Vec<String>,
    pub remaining_shuffle: Vec<String>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub order: PlaybackOrder,
    pub repeat_mode: MusicRepeatMode,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    pub random_state: u64,
}
