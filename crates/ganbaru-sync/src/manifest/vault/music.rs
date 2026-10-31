//! Music: table ids 48 to 95 and the artwork asset class.
//!
//! Roots, items, locations, source collections, playlists, memberships, snoozes, soundscapes,
//! presets, and Focus phase assignments replicate; the six tables with a composite primary key
//! are keyed by `sync_key`. Device observations (availability, file size and mtime, refresh
//! state, generations) are `Local` columns, and sessions, receipts, refresh jobs, and relink
//! plans stay on the device. Ids 63 to 95 are reserved.

use ganbaru_sync_contracts::{GroupId, TableId};

use crate::assets::AssetClass;
use crate::domains::music::{
    MAX_ARTWORK_BYTES, MAX_ARTWORK_REFERENCE_CHARS, MAX_COLLECTION_NAME_CHARS,
    MAX_DESCRIPTION_CHARS, MAX_ENUM_CHARS, MAX_FINGERPRINT_CHARS, MAX_ICON_CHARS, MAX_ID_CHARS,
    MAX_IDENTITY_KEY_CHARS, MAX_NAME_CHARS, MAX_OVERRIDE_CHARS, MAX_PRESET_LEVEL_MILLI,
    MAX_PRESET_NAME_CHARS, MAX_RATE_MILLI, MAX_REASON_CHARS, MAX_RELATIVE_PATH_CHARS,
    MAX_SKIP_RANGES, MAX_SOUND_LEVEL_MILLI, MAX_VOLUME_MILLI, MIN_RATE_MILLI,
};
use crate::manifest::{
    Collation, ColumnSpec, ColumnType, DerivedKey, DeviceTable, DomainManifest, DomainTrigger,
    GroupSpec, GroupStorage, LocalColumn, MergeKind, OnDelete, OwnedBound, OwnedCodec, OwnedOrder,
    OwnedTable, ReferenceColumn, Resolution, SYNC_KEY_COLUMN, TableIdRange, TableSpec,
    TriggerPolicy, group_id,
};

/// Domain name for Music.
pub const DOMAIN_NAME: &str = "music";

/// Domain adapter name for Music.
pub const ADAPTER: &str = "music";

/// `music_local_roots`.
pub const ROOTS_TABLE: TableId = TableId(48);
/// `music_library_items`.
pub const ITEMS_TABLE: TableId = TableId(49);
/// `music_item_signals`.
pub const SIGNALS_TABLE: TableId = TableId(50);
/// `music_listening_statistics`.
pub const STATISTICS_TABLE: TableId = TableId(51);
/// `music_local_locations`.
pub const LOCATIONS_TABLE: TableId = TableId(52);
/// `music_source_collections`.
pub const COLLECTIONS_TABLE: TableId = TableId(53);
/// `music_source_collection_items`.
pub const COLLECTION_ITEMS_TABLE: TableId = TableId(54);
/// `music_playlists`.
pub const PLAYLISTS_TABLE: TableId = TableId(55);
/// `music_playlist_intended_uses`.
pub const INTENDED_USES_TABLE: TableId = TableId(56);
/// `music_playlist_memberships`.
pub const MEMBERSHIPS_TABLE: TableId = TableId(57);
/// `music_snoozes`.
pub const SNOOZES_TABLE: TableId = TableId(58);
/// `music_soundscapes`.
pub const SOUNDSCAPES_TABLE: TableId = TableId(59);
/// `music_soundscape_presets`.
pub const PRESETS_TABLE: TableId = TableId(60);
/// `music_soundscape_preset_sounds`.
pub const PRESET_SOUNDS_TABLE: TableId = TableId(61);
/// `music_context_assignments`.
pub const ASSIGNMENTS_TABLE: TableId = TableId(62);

/// Owned skip ranges of a membership.
pub const SKIP_RANGES_OWNED: &str = "music_membership_skip_ranges";

/// Artwork overrides of Music items, at most 10 MiB each.
pub const ARTWORK_CLASS: AssetClass = AssetClass {
    name: "music/artwork",
    max_bytes: MAX_ARTWORK_BYTES,
};

/// Root field groups.
pub mod root_group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(2);
}

/// Item field groups.
pub mod item_group {
    use super::{GroupId, group_id};

    /// `source_kind`, `identity_key`, `youtube_video_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `discovered_at_ms`.
    pub const DISCOVERED: GroupId = group_id(1);
    /// Media kind, original tags, duration, and whether that metadata came from the source.
    /// Coupled: resolved metadata wins over a concurrent placeholder.
    pub const SOURCE: GroupId = group_id(2);
    /// `title_override`.
    pub const TITLE: GroupId = group_id(3);
    /// `artist_override`.
    pub const ARTIST: GroupId = group_id(4);
    /// `album_override`.
    pub const ALBUM: GroupId = group_id(5);
    /// `artwork_override` and `artwork_override_byte_size`.
    pub const ARTWORK: GroupId = group_id(6);
    /// `review_state`, `review_changed_at_ms`, `review_deferred_until_ms`.
    pub const REVIEW: GroupId = group_id(7);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(8);
    /// `youtube_resolution_state`. Coupled: a resolved or terminal state wins over a
    /// concurrent `resolving`.
    pub const RESOLUTION: GroupId = group_id(9);
}

/// Signal field groups.
pub mod signal_group {
    use super::{GroupId, group_id};

    /// `item_id`, `signal`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(1);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(2);
}

/// Listening statistics field groups.
pub mod statistics_group {
    use super::{GroupId, group_id};

    /// `device_id`, `item_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `last_played_at_ms`.
    pub const PLAYED: GroupId = group_id(1);
    /// `play_count`.
    pub const PLAYS: GroupId = group_id(2);
    /// `completion_count`.
    pub const COMPLETIONS: GroupId = group_id(3);
    /// `skip_count`.
    pub const SKIPS: GroupId = group_id(4);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(5);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(6);
}

/// Location field groups.
pub mod location_group {
    use super::{GroupId, group_id};

    /// `root_id`, `relative_path`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `item_id`.
    pub const ITEM: GroupId = group_id(1);
    /// `first_seen_at_ms`.
    pub const DISCOVERED: GroupId = group_id(2);
    /// `lightweight_fingerprint`, `strong_fingerprint`.
    pub const FINGERPRINT: GroupId = group_id(3);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Source collection field groups.
pub mod collection_group {
    use super::{GroupId, group_id};

    /// `kind`, `identity_key`, `local_root_id`, `youtube_playlist_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `discovery_enabled`.
    pub const DISCOVERY: GroupId = group_id(3);
    /// `removed_at_ms`.
    pub const REMOVAL: GroupId = group_id(4);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(5);
}

/// Source collection item field groups.
pub mod collection_item_group {
    use super::{GroupId, group_id};

    /// `collection_id`, `item_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `first_discovered_at_ms`.
    pub const DISCOVERED: GroupId = group_id(1);
    /// `source_position`.
    pub const POSITION: GroupId = group_id(2);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(3);
}

/// Playlist field groups.
pub mod playlist_group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `icon`.
    pub const ICON: GroupId = group_id(2);
    /// `shuffle_enabled`, `repeat_mode`, `mix_enabled`.
    pub const PLAYBACK: GroupId = group_id(3);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(4);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(5);
}

/// Intended use field groups.
pub mod intended_use_group {
    use super::{GroupId, group_id};

    /// `playlist_id`, `intended_use`.
    pub const IDENTITY: GroupId = group_id(0);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(1);
}

/// Membership field groups.
pub mod membership_group {
    use super::{GroupId, group_id};

    /// `playlist_id`, `item_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(1);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(2);
    /// `weight`.
    pub const WEIGHT: GroupId = group_id(3);
    /// `enabled`.
    pub const ENABLED: GroupId = group_id(4);
    /// `start_ms`, `end_ms`, `volume_milli`, `rate_milli`.
    pub const RANGE: GroupId = group_id(5);
    /// `break_item_id`.
    pub const BREAK_ITEM: GroupId = group_id(6);
    /// The break copies of the range columns.
    pub const BREAK_RANGE: GroupId = group_id(7);
    /// Owned skip ranges.
    pub const SKIPS: GroupId = group_id(8);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(9);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(10);
}

/// Snooze field groups.
pub mod snooze_group {
    use super::{GroupId, group_id};

    /// `item_id`, `scope`, `playlist_id`.
    pub const TARGET: GroupId = group_id(0);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(1);
    /// `starts_at_ms`, `ends_at_ms`.
    pub const WINDOW: GroupId = group_id(2);
    /// `reason`.
    pub const REASON: GroupId = group_id(3);
}

/// Soundscape field groups.
pub mod soundscape_group {
    use super::{GroupId, group_id};

    /// `source_kind`, `generated_kind`, `bundled_identity`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(1);
    /// `name`.
    pub const NAME: GroupId = group_id(2);
    /// `icon`.
    pub const ICON: GroupId = group_id(3);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Preset field groups.
pub mod preset_group {
    use super::{GroupId, group_id};

    /// `created_at_ms`.
    pub const CREATED: GroupId = group_id(0);
    /// `name`.
    pub const NAME: GroupId = group_id(1);
    /// `icon`.
    pub const ICON: GroupId = group_id(2);
    /// `level_milli`.
    pub const LEVEL: GroupId = group_id(3);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(4);
}

/// Preset sound field groups.
pub mod preset_sound_group {
    use super::{GroupId, group_id};

    /// `preset_id`, `soundscape_id`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `order_key`.
    pub const ORDER: GroupId = group_id(1);
    /// `level_milli`.
    pub const LEVEL: GroupId = group_id(2);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(3);
}

/// Focus phase assignment field groups.
pub mod assignment_group {
    use super::{GroupId, group_id};

    /// `owner_kind`, `owner_id`, `phase`.
    pub const IDENTITY: GroupId = group_id(0);
    /// `behavior`.
    pub const BEHAVIOR: GroupId = group_id(1);
    /// `playlist_id`.
    pub const PLAYLIST: GroupId = group_id(2);
    /// `soundscape_behavior`.
    pub const SOUNDSCAPE_BEHAVIOR: GroupId = group_id(3);
    /// `soundscape_preset_id`.
    pub const PRESET: GroupId = group_id(4);
    /// `provenance_kind`, `provenance_id`.
    pub const PROVENANCE: GroupId = group_id(5);
    /// `updated_at_ms`.
    pub const UPDATED: GroupId = group_id(6);
    /// Existence register.
    pub const EXISTS: GroupId = group_id(7);
}

/// A hidden, non-recoverable group without references.
const fn group(
    id: GroupId,
    name: &'static str,
    kind: MergeKind,
    columns: &'static [ColumnSpec],
) -> GroupSpec {
    GroupSpec {
        id,
        name,
        kind,
        storage: GroupStorage::Columns(columns),
        references: &[],
        surfaced: false,
        recoverable: false,
        bumps_revision: false,
    }
}

/// `spec`, offered in recovery entries when its row is deleted while it changes.
const fn recoverable(spec: GroupSpec) -> GroupSpec {
    GroupSpec {
        recoverable: true,
        ..spec
    }
}

/// An `Immutable` group whose reference columns cascade from their rows.
const fn immutable(
    id: GroupId,
    name: &'static str,
    columns: &'static [ColumnSpec],
    references: &'static [ReferenceColumn],
) -> GroupSpec {
    GroupSpec {
        references,
        ..group(id, name, MergeKind::Immutable, columns)
    }
}

/// The existence register of a derived-key table.
const fn exists(id: GroupId) -> GroupSpec {
    group(id, "exists", MergeKind::Register, &[])
}

/// A one-column reference group.
const fn reference(
    id: GroupId,
    name: &'static str,
    column: &'static [ColumnSpec],
    table: TableId,
    on_delete: OnDelete,
) -> GroupSpec {
    group(id, name, MergeKind::Reference { table, on_delete }, column)
}

const fn cascade(column: &'static str, table: TableId) -> ReferenceColumn {
    ReferenceColumn {
        column,
        table,
        on_delete: OnDelete::Cascade,
    }
}

const fn text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Text { max_chars },
    }
}

const fn optional_text(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalText { max_chars },
    }
}

const fn trimmed(name: &'static str, max_chars: usize) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::TrimmedText {
            min_chars: 1,
            max_chars,
        },
    }
}

const fn integer(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Integer { min, max },
    }
}

const fn optional_integer(name: &'static str, min: i64, max: i64) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalInteger { min, max },
    }
}

/// A positive millisecond time, the form the schema checks with `> 0`.
const fn positive_ms(name: &'static str) -> ColumnSpec {
    integer(name, 1, i64::MAX)
}

/// A millisecond time the schema leaves unchecked.
const fn any_ms(name: &'static str) -> ColumnSpec {
    integer(name, i64::MIN, i64::MAX)
}

const fn flag(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::Flag,
    }
}

const fn order_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OrderKey,
    }
}

const fn row_key(name: &'static str) -> ColumnSpec {
    ColumnSpec {
        name,
        ty: ColumnType::OptionalRowKey,
    }
}

const fn local(name: &'static str, materialize: &'static str) -> LocalColumn {
    LocalColumn { name, materialize }
}

const fn applier_visible(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::ApplierVisible,
    }
}

const fn gated(name: &'static str) -> DomainTrigger {
    DomainTrigger {
        name,
        policy: TriggerPolicy::Gated,
    }
}

const fn unique(columns: &'static [&'static str]) -> Resolution {
    Resolution::DuplicateRepair {
        columns,
        collation: Collation::Binary,
    }
}

/// The local `version` counter every editable Music row carries.
const VERSION: LocalColumn = local("version", "1");

/// A table keyed by `id` with no derived columns, hooks, tree, or publication; each table
/// overrides what it declares.
const BASE: TableSpec = TableSpec {
    id: ROOTS_TABLE,
    name: "",
    key_column: "id",
    groups: &[],
    local_columns: &[],
    derived_columns: &[],
    hook_columns: &[],
    key_derived_columns: &[],
    revision_column: None,
    resolutions: &[],
    redirects: false,
    adapter: Some(ADAPTER),
    triggers: &[],
    derived_key: None,
    tree: None,
    acyclic: None,
    publish_when: None,
    published_with: None,
    key_parent: None,
    holds_deletes: false,
};

/// `music_local_roots`.
pub const ROOTS: TableSpec = TableSpec {
    id: ROOTS_TABLE,
    name: "music_local_roots",
    groups: &[
        group(
            root_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[positive_ms("created_at_ms")],
        ),
        group(
            root_group::NAME,
            "name",
            MergeKind::Register,
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        group(
            root_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[VERSION],
    ..BASE
};

/// `music_library_items`.
pub const ITEMS: TableSpec = TableSpec {
    id: ITEMS_TABLE,
    name: "music_library_items",
    groups: &[
        immutable(
            item_group::IDENTITY,
            "identity",
            &[
                text("source_kind", MAX_ENUM_CHARS),
                trimmed("identity_key", MAX_IDENTITY_KEY_CHARS),
                optional_text("youtube_video_id", MAX_ID_CHARS),
            ],
            &[],
        ),
        group(
            item_group::DISCOVERED,
            "discovered",
            MergeKind::Register,
            &[positive_ms("discovered_at_ms")],
        ),
        group(
            item_group::SOURCE,
            "source",
            MergeKind::Coupled,
            &[
                text("media_kind", MAX_ENUM_CHARS),
                text("original_title", MAX_DESCRIPTION_CHARS),
                text("original_artist", MAX_DESCRIPTION_CHARS),
                text("original_album", MAX_DESCRIPTION_CHARS),
                optional_integer("original_track_number", 1, i64::MAX),
                optional_integer("duration_ms", 0, i64::MAX),
                flag("metadata_resolved"),
            ],
        ),
        group(
            item_group::TITLE,
            "title",
            MergeKind::Register,
            &[optional_text("title_override", MAX_OVERRIDE_CHARS)],
        ),
        group(
            item_group::ARTIST,
            "artist",
            MergeKind::Register,
            &[optional_text("artist_override", MAX_OVERRIDE_CHARS)],
        ),
        group(
            item_group::ALBUM,
            "album",
            MergeKind::Register,
            &[optional_text("album_override", MAX_OVERRIDE_CHARS)],
        ),
        group(
            item_group::ARTWORK,
            "artwork",
            MergeKind::Register,
            &[
                optional_text("artwork_override", MAX_ARTWORK_REFERENCE_CHARS),
                optional_integer("artwork_override_byte_size", 1, MAX_ARTWORK_BYTES as i64),
            ],
        ),
        group(
            item_group::REVIEW,
            "review",
            MergeKind::Register,
            &[
                text("review_state", MAX_ENUM_CHARS),
                optional_integer("review_changed_at_ms", i64::MIN, i64::MAX),
                optional_integer("review_deferred_until_ms", 1, i64::MAX),
            ],
        ),
        group(
            item_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
        group(
            item_group::RESOLUTION,
            "resolution",
            MergeKind::Coupled,
            &[optional_text("youtube_resolution_state", MAX_ENUM_CHARS)],
        ),
    ],
    local_columns: &[
        local("availability", "'unknown'"),
        local("original_artwork_identity", "NULL"),
        VERSION,
    ],
    resolutions: &[
        unique(&["identity_key"]),
        unique(&["youtube_video_id"]),
        Resolution::WithinGroup {
            columns: &["source_kind", "youtube_video_id"],
            group: item_group::IDENTITY,
        },
        Resolution::WithinGroup {
            columns: &["artwork_override", "artwork_override_byte_size"],
            group: item_group::ARTWORK,
        },
    ],
    redirects: true,
    triggers: &[
        applier_visible("music_library_items_description_limits_insert"),
        applier_visible("music_library_items_description_limits_update"),
    ],
    ..BASE
};

/// `music_item_signals`.
pub const SIGNALS: TableSpec = TableSpec {
    id: SIGNALS_TABLE,
    name: "music_item_signals",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            signal_group::IDENTITY,
            "identity",
            &[row_key("item_id"), text("signal", MAX_ENUM_CHARS)],
            &[cascade("item_id", ITEMS_TABLE)],
        ),
        group(
            signal_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        exists(signal_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: signal_group::EXISTS,
        prefix: Some("music-signal"),
        key_inputs: &["item_id", "signal"],
    }),
    ..BASE
};

/// `music_listening_statistics`, one row per device and item. Each device writes only its
/// own row, so every count merges as a maximum.
pub const STATISTICS: TableSpec = TableSpec {
    id: STATISTICS_TABLE,
    name: "music_listening_statistics",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            statistics_group::IDENTITY,
            "identity",
            &[trimmed("device_id", MAX_ID_CHARS), row_key("item_id")],
            &[cascade("item_id", ITEMS_TABLE)],
        ),
        group(
            statistics_group::PLAYED,
            "played",
            MergeKind::Max,
            &[optional_integer("last_played_at_ms", i64::MIN, i64::MAX)],
        ),
        group(
            statistics_group::PLAYS,
            "plays",
            MergeKind::Max,
            &[integer("play_count", 0, i64::MAX)],
        ),
        group(
            statistics_group::COMPLETIONS,
            "completions",
            MergeKind::Max,
            &[integer("completion_count", 0, i64::MAX)],
        ),
        group(
            statistics_group::SKIPS,
            "skips",
            MergeKind::Max,
            &[integer("skip_count", 0, i64::MAX)],
        ),
        group(
            statistics_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
        exists(statistics_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: statistics_group::EXISTS,
        prefix: Some("music-statistics"),
        key_inputs: &["device_id", "item_id"],
    }),
    ..BASE
};

/// `music_local_locations`.
pub const LOCATIONS: TableSpec = TableSpec {
    id: LOCATIONS_TABLE,
    name: "music_local_locations",
    groups: &[
        immutable(
            location_group::IDENTITY,
            "identity",
            &[
                row_key("root_id"),
                trimmed("relative_path", MAX_RELATIVE_PATH_CHARS),
            ],
            &[cascade("root_id", ROOTS_TABLE)],
        ),
        reference(
            location_group::ITEM,
            "item",
            &[row_key("item_id")],
            ITEMS_TABLE,
            OnDelete::Cascade,
        ),
        group(
            location_group::DISCOVERED,
            "discovered",
            MergeKind::Register,
            &[positive_ms("first_seen_at_ms")],
        ),
        group(
            location_group::FINGERPRINT,
            "fingerprint",
            MergeKind::Register,
            &[
                optional_text("lightweight_fingerprint", MAX_FINGERPRINT_CHARS),
                optional_text("strong_fingerprint", MAX_FINGERPRINT_CHARS),
            ],
        ),
        group(
            location_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[
        local("file_size_bytes", "NULL"),
        local("modified_at_ms", "NULL"),
        local("availability", "'unknown'"),
        local("last_seen_generation", "NULL"),
    ],
    resolutions: &[
        unique(&["root_id", "relative_path"]),
        unique(&["root_id", "item_id", "relative_path"]),
    ],
    redirects: true,
    ..BASE
};

/// `music_source_collections`. The identity key names the root or YouTube playlist, so its
/// uniqueness also keeps one collection per root.
pub const COLLECTIONS: TableSpec = TableSpec {
    id: COLLECTIONS_TABLE,
    name: "music_source_collections",
    groups: &[
        immutable(
            collection_group::IDENTITY,
            "identity",
            &[
                text("kind", MAX_ENUM_CHARS),
                trimmed("identity_key", MAX_IDENTITY_KEY_CHARS),
                row_key("local_root_id"),
                optional_text("youtube_playlist_id", MAX_ID_CHARS),
            ],
            &[cascade("local_root_id", ROOTS_TABLE)],
        ),
        group(
            collection_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        group(
            collection_group::NAME,
            "name",
            MergeKind::Register,
            &[trimmed("name", MAX_COLLECTION_NAME_CHARS)],
        ),
        group(
            collection_group::DISCOVERY,
            "discovery",
            MergeKind::Register,
            &[flag("discovery_enabled")],
        ),
        group(
            collection_group::REMOVAL,
            "removal",
            MergeKind::Register,
            &[optional_integer("removed_at_ms", i64::MIN, i64::MAX)],
        ),
        group(
            collection_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[
        local("refresh_state", "'idle'"),
        local("last_successful_refresh_at_ms", "NULL"),
        local("previous_successful_refresh_at_ms", "NULL"),
        local("last_refresh_error_code", "NULL"),
        local("snapshot_generation", "0"),
        VERSION,
    ],
    resolutions: &[
        unique(&["identity_key"]),
        unique(&["youtube_playlist_id"]),
        Resolution::WithinGroup {
            columns: &[
                "kind",
                "identity_key",
                "local_root_id",
                "youtube_playlist_id",
            ],
            group: collection_group::IDENTITY,
        },
    ],
    redirects: true,
    ..BASE
};

/// `music_source_collection_items`.
pub const COLLECTION_ITEMS: TableSpec = TableSpec {
    id: COLLECTION_ITEMS_TABLE,
    name: "music_source_collection_items",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            collection_item_group::IDENTITY,
            "identity",
            &[row_key("collection_id"), row_key("item_id")],
            &[
                cascade("collection_id", COLLECTIONS_TABLE),
                cascade("item_id", ITEMS_TABLE),
            ],
        ),
        group(
            collection_item_group::DISCOVERED,
            "discovered",
            MergeKind::Register,
            &[positive_ms("first_discovered_at_ms")],
        ),
        group(
            collection_item_group::POSITION,
            "position",
            MergeKind::Register,
            &[optional_integer("source_position", 0, i64::MAX)],
        ),
        exists(collection_item_group::EXISTS),
    ],
    local_columns: &[
        local("last_seen_generation", "0"),
        local("missing_from_latest_snapshot", "0"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: collection_item_group::EXISTS,
        prefix: Some("music-collection-item"),
        key_inputs: &["collection_id", "item_id"],
    }),
    ..BASE
};

/// `music_playlists`.
pub const PLAYLISTS: TableSpec = TableSpec {
    id: PLAYLISTS_TABLE,
    name: "music_playlists",
    groups: &[
        group(
            playlist_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[any_ms("created_at_ms")],
        ),
        recoverable(group(
            playlist_group::NAME,
            "name",
            MergeKind::Register,
            &[trimmed("name", MAX_NAME_CHARS)],
        )),
        recoverable(group(
            playlist_group::ICON,
            "icon",
            MergeKind::Register,
            &[text("icon", MAX_ICON_CHARS)],
        )),
        recoverable(group(
            playlist_group::PLAYBACK,
            "playback",
            MergeKind::Register,
            &[
                flag("shuffle_enabled"),
                text("repeat_mode", MAX_ENUM_CHARS),
                flag("mix_enabled"),
            ],
        )),
        group(
            playlist_group::ORDER,
            "order",
            MergeKind::Position,
            &[order_key("order_key")],
        ),
        group(
            playlist_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[any_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[VERSION],
    triggers: &[
        applier_visible("music_playlists_order_key_insert"),
        applier_visible("music_playlists_order_key_update"),
    ],
    ..BASE
};

/// `music_playlist_intended_uses`. The identity is recoverable, so an intended use added while
/// another device deletes its playlist offers the playlist for recovery and returns with it.
pub const INTENDED_USES: TableSpec = TableSpec {
    id: INTENDED_USES_TABLE,
    name: "music_playlist_intended_uses",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        recoverable(immutable(
            intended_use_group::IDENTITY,
            "identity",
            &[row_key("playlist_id"), text("intended_use", MAX_ENUM_CHARS)],
            &[cascade("playlist_id", PLAYLISTS_TABLE)],
        )),
        exists(intended_use_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: intended_use_group::EXISTS,
        prefix: Some("music-intended-use"),
        key_inputs: &["playlist_id", "intended_use"],
    }),
    ..BASE
};

/// Membership range columns, `prefix` empty for the item and `break_` for the break item.
const RANGE_COLUMNS: [ColumnSpec; 4] = [
    optional_integer("start_ms", 0, i64::MAX),
    optional_integer("end_ms", 0, i64::MAX),
    optional_integer("volume_milli", 0, MAX_VOLUME_MILLI),
    optional_integer("rate_milli", MIN_RATE_MILLI, MAX_RATE_MILLI),
];

const BREAK_RANGE_COLUMNS: [ColumnSpec; 4] = [
    optional_integer("break_start_ms", 0, i64::MAX),
    optional_integer("break_end_ms", 0, i64::MAX),
    optional_integer("break_volume_milli", 0, MAX_VOLUME_MILLI),
    optional_integer("break_rate_milli", MIN_RATE_MILLI, MAX_RATE_MILLI),
];

/// The canonical skip list of a membership: sorted, non-overlapping ranges in ordinal order.
pub const SKIP_RANGES: OwnedTable = OwnedTable {
    table: SKIP_RANGES_OWNED,
    parent_column: "membership_id",
    order: OwnedOrder::Ordinal("ordinal"),
    columns: &[
        integer("start_ms", 0, i64::MAX),
        integer("end_ms", 0, i64::MAX),
    ],
    references: &[],
    unique: &[],
    max_rows: MAX_SKIP_RANGES,
    codec: OwnedCodec::Generic,
    nested: &[],
    bound: OwnedBound::Static,
};

/// `music_playlist_memberships`, keyed by the membership identity of its playlist and item,
/// so a removed membership can be added again under the same key. `UNIQUE (playlist_id,
/// item_id)` equals the key inputs and is implied by the key.
pub const MEMBERSHIPS: TableSpec = TableSpec {
    id: MEMBERSHIPS_TABLE,
    name: "music_playlist_memberships",
    groups: &[
        immutable(
            membership_group::IDENTITY,
            "identity",
            &[row_key("playlist_id"), row_key("item_id")],
            &[
                cascade("playlist_id", PLAYLISTS_TABLE),
                cascade("item_id", ITEMS_TABLE),
            ],
        ),
        group(
            membership_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        group(
            membership_group::ORDER,
            "order",
            MergeKind::Position,
            &[order_key("order_key")],
        ),
        recoverable(group(
            membership_group::WEIGHT,
            "weight",
            MergeKind::Register,
            &[text("weight", MAX_ENUM_CHARS)],
        )),
        recoverable(group(
            membership_group::ENABLED,
            "enabled",
            MergeKind::Register,
            &[flag("enabled")],
        )),
        recoverable(group(
            membership_group::RANGE,
            "range",
            MergeKind::Register,
            &RANGE_COLUMNS,
        )),
        recoverable(reference(
            membership_group::BREAK_ITEM,
            "break item",
            &[row_key("break_item_id")],
            ITEMS_TABLE,
            OnDelete::SetNull,
        )),
        recoverable(group(
            membership_group::BREAK_RANGE,
            "break range",
            MergeKind::Register,
            &BREAK_RANGE_COLUMNS,
        )),
        GroupSpec {
            id: membership_group::SKIPS,
            name: "skips",
            kind: MergeKind::Register,
            storage: GroupStorage::Owned {
                columns: &[],
                tables: &[SKIP_RANGES],
            },
            references: &[],
            surfaced: false,
            recoverable: true,
            bumps_revision: false,
        },
        group(
            membership_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
        exists(membership_group::EXISTS),
    ],
    local_columns: &[VERSION],
    resolutions: &[
        Resolution::WithinGroup {
            columns: &["start_ms", "end_ms"],
            group: membership_group::RANGE,
        },
        Resolution::WithinGroup {
            columns: &["break_start_ms", "break_end_ms"],
            group: membership_group::BREAK_RANGE,
        },
    ],
    triggers: &[
        applier_visible("music_playlist_memberships_order_key_insert"),
        applier_visible("music_playlist_memberships_order_key_update"),
    ],
    derived_key: Some(DerivedKey {
        exists_group: membership_group::EXISTS,
        prefix: Some("music-membership"),
        key_inputs: &["playlist_id", "item_id"],
    }),
    ..BASE
};

/// `music_snoozes`.
pub const SNOOZES: TableSpec = TableSpec {
    id: SNOOZES_TABLE,
    name: "music_snoozes",
    groups: &[
        immutable(
            snooze_group::TARGET,
            "target",
            &[
                row_key("item_id"),
                text("scope", MAX_ENUM_CHARS),
                row_key("playlist_id"),
            ],
            &[
                cascade("item_id", ITEMS_TABLE),
                cascade("playlist_id", PLAYLISTS_TABLE),
            ],
        ),
        group(
            snooze_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        group(
            snooze_group::WINDOW,
            "window",
            MergeKind::Register,
            &[
                positive_ms("starts_at_ms"),
                optional_integer("ends_at_ms", 1, i64::MAX),
            ],
        ),
        group(
            snooze_group::REASON,
            "reason",
            MergeKind::Register,
            &[text("reason", MAX_REASON_CHARS)],
        ),
    ],
    resolutions: &[
        Resolution::WithinGroup {
            columns: &["scope", "playlist_id"],
            group: snooze_group::TARGET,
        },
        Resolution::WithinGroup {
            columns: &["starts_at_ms", "ends_at_ms"],
            group: snooze_group::WINDOW,
        },
    ],
    ..BASE
};

/// `music_soundscapes`. Availability is observed per device: local loops are checked against
/// this device's file map, and generated and bundled sounds are always available. Generated
/// noise rows are seeds with one fixed key per kind, so the kind is unique without a repair;
/// a gated trigger refuses local writes that would give a kind another key.
pub const SOUNDSCAPES: TableSpec = TableSpec {
    id: SOUNDSCAPES_TABLE,
    name: "music_soundscapes",
    groups: &[
        immutable(
            soundscape_group::IDENTITY,
            "identity",
            &[
                text("source_kind", MAX_ENUM_CHARS),
                optional_text("generated_kind", MAX_ENUM_CHARS),
                optional_text("bundled_identity", MAX_IDENTITY_KEY_CHARS),
            ],
            &[],
        ),
        group(
            soundscape_group::CREATED,
            "created",
            MergeKind::Register,
            &[positive_ms("created_at_ms")],
        ),
        group(
            soundscape_group::NAME,
            "name",
            MergeKind::Register,
            &[trimmed("name", MAX_NAME_CHARS)],
        ),
        group(
            soundscape_group::ICON,
            "icon",
            MergeKind::Register,
            &[trimmed("icon", MAX_ICON_CHARS)],
        ),
        group(
            soundscape_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[local("availability", "'available'"), VERSION],
    resolutions: &[Resolution::WithinGroup {
        columns: &["source_kind", "generated_kind", "bundled_identity"],
        group: soundscape_group::IDENTITY,
    }],
    triggers: &[
        gated("music_soundscapes_generated_identity_insert"),
        gated("music_soundscapes_generated_identity_update"),
    ],
    ..BASE
};

/// `music_soundscape_presets`.
pub const PRESETS: TableSpec = TableSpec {
    id: PRESETS_TABLE,
    name: "music_soundscape_presets",
    groups: &[
        group(
            preset_group::CREATED,
            "created",
            MergeKind::Immutable,
            &[positive_ms("created_at_ms")],
        ),
        group(
            preset_group::NAME,
            "name",
            MergeKind::Register,
            &[trimmed("name", MAX_PRESET_NAME_CHARS)],
        ),
        group(
            preset_group::ICON,
            "icon",
            MergeKind::Register,
            &[trimmed("icon", MAX_ICON_CHARS)],
        ),
        group(
            preset_group::LEVEL,
            "level",
            MergeKind::Register,
            &[integer("level_milli", 0, MAX_PRESET_LEVEL_MILLI)],
        ),
        group(
            preset_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
    ],
    local_columns: &[VERSION],
    ..BASE
};

/// `music_soundscape_preset_sounds`.
pub const PRESET_SOUNDS: TableSpec = TableSpec {
    id: PRESET_SOUNDS_TABLE,
    name: "music_soundscape_preset_sounds",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            preset_sound_group::IDENTITY,
            "identity",
            &[row_key("preset_id"), row_key("soundscape_id")],
            &[
                cascade("preset_id", PRESETS_TABLE),
                cascade("soundscape_id", SOUNDSCAPES_TABLE),
            ],
        ),
        group(
            preset_sound_group::ORDER,
            "order",
            MergeKind::Position,
            &[order_key("order_key")],
        ),
        group(
            preset_sound_group::LEVEL,
            "level",
            MergeKind::Register,
            &[integer("level_milli", 0, MAX_SOUND_LEVEL_MILLI)],
        ),
        exists(preset_sound_group::EXISTS),
    ],
    derived_key: Some(DerivedKey {
        exists_group: preset_sound_group::EXISTS,
        prefix: Some("music-preset-sound"),
        key_inputs: &["preset_id", "soundscape_id"],
    }),
    ..BASE
};

/// `music_context_assignments`. Owners and the chosen playlist and preset stay opaque ids, so
/// a dangling one reads as unavailable rather than turning into none.
pub const ASSIGNMENTS: TableSpec = TableSpec {
    id: ASSIGNMENTS_TABLE,
    name: "music_context_assignments",
    key_column: SYNC_KEY_COLUMN,
    groups: &[
        immutable(
            assignment_group::IDENTITY,
            "identity",
            &[
                text("owner_kind", MAX_ENUM_CHARS),
                trimmed("owner_id", MAX_ID_CHARS),
                text("phase", MAX_ENUM_CHARS),
            ],
            &[],
        ),
        group(
            assignment_group::BEHAVIOR,
            "behavior",
            MergeKind::Register,
            &[text("behavior", MAX_ENUM_CHARS)],
        ),
        group(
            assignment_group::PLAYLIST,
            "playlist",
            MergeKind::Register,
            &[optional_text("playlist_id", MAX_ID_CHARS)],
        ),
        group(
            assignment_group::SOUNDSCAPE_BEHAVIOR,
            "soundscape behavior",
            MergeKind::Register,
            &[text("soundscape_behavior", MAX_ENUM_CHARS)],
        ),
        group(
            assignment_group::PRESET,
            "preset",
            MergeKind::Register,
            &[optional_text("soundscape_preset_id", MAX_ID_CHARS)],
        ),
        group(
            assignment_group::PROVENANCE,
            "provenance",
            MergeKind::Register,
            &[
                text("provenance_kind", MAX_ENUM_CHARS),
                optional_text("provenance_id", MAX_ID_CHARS),
            ],
        ),
        group(
            assignment_group::UPDATED,
            "updated",
            MergeKind::Max,
            &[positive_ms("updated_at_ms")],
        ),
        exists(assignment_group::EXISTS),
    ],
    local_columns: &[VERSION],
    derived_key: Some(DerivedKey {
        exists_group: assignment_group::EXISTS,
        prefix: Some("music-assignment"),
        key_inputs: &["owner_kind", "owner_id", "phase"],
    }),
    ..BASE
};

/// The soundscape state every device reads before it can play a soundscape.
const SOUNDSCAPE_STATE_SEED: &str = "INSERT OR IGNORE INTO music_soundscape_state \
     (singleton, updated_at_ms) VALUES (1, CAST(strftime('%s', 'now') AS INTEGER) * 1000)";

/// A device table without domain triggers.
const fn device(name: &'static str) -> DeviceTable {
    DeviceTable {
        name,
        seed: None,
        triggers: &[],
        assets: &[],
    }
}

/// Music domain. Its commands run on linked replicas.
pub const DOMAIN: DomainManifest = DomainManifest {
    name: DOMAIN_NAME,
    table_ids: TableIdRange {
        first: 48,
        last: 95,
    },
    tables: &[
        ROOTS,
        ITEMS,
        SIGNALS,
        STATISTICS,
        LOCATIONS,
        COLLECTIONS,
        COLLECTION_ITEMS,
        PLAYLISTS,
        INTENDED_USES,
        MEMBERSHIPS,
        SNOOZES,
        SOUNDSCAPES,
        PRESETS,
        PRESET_SOUNDS,
        ASSIGNMENTS,
    ],
    derived: &["music_search_fts", "music_search_index_state"],
    local: &[
        device("music_recent_selections"),
        device("music_refresh_job_entries"),
        device("music_refresh_job_issues"),
        device("music_refresh_jobs"),
        device("music_relink_plan_entries"),
        device("music_relink_plans"),
        device("music_session_checkpoints"),
        device("music_session_receipts"),
        DeviceTable {
            seed: Some(SOUNDSCAPE_STATE_SEED),
            ..device("music_soundscape_state")
        },
        device("music_transfer_receipts"),
    ],
    engine: &[],
    unconverted: &[],
    assets: &[ARTWORK_CLASS],
};
