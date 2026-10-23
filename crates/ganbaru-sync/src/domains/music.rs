//! Music value rules shared by the library and the engine: the managed artwork reference and the
//! adapter checking values the column types cannot express.
//!
//! An artwork override is stored as `music/artwork/{sha256}.{ext}`, the class name of
//! [`ARTWORK_CLASS`] followed by the engine's asset file name, so the reference names exactly
//! one content-addressed file and never an arbitrary path.

use std::fmt;
use std::path::{Path, PathBuf};

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::assets::{self, AssetRef};
use crate::codec;
use crate::manifest::vault::music::{
    ADAPTER, ARTWORK_CLASS, ASSIGNMENTS_TABLE, COLLECTION_ITEMS_TABLE, COLLECTIONS_TABLE,
    INTENDED_USES_TABLE, ITEMS_TABLE, LOCATIONS_TABLE, MEMBERSHIPS_TABLE, PLAYLISTS_TABLE,
    PRESET_SOUNDS_TABLE, SIGNALS_TABLE, SKIP_RANGES, SNOOZES_TABLE, SOUNDSCAPES_TABLE,
    STATISTICS_TABLE, assignment_group, collection_group, collection_item_group,
    intended_use_group, item_group, location_group, membership_group, playlist_group,
    preset_sound_group, signal_group, snooze_group, soundscape_group, statistics_group,
};

/// Bytes in one artwork file.
pub const MAX_ARTWORK_BYTES: u32 = 10 * 1024 * 1024;

/// Characters in a stored artwork reference, `music/artwork/{sha256}.{ext}` with room to spare.
pub const MAX_ARTWORK_REFERENCE_CHARS: usize = 128;

/// Characters in a schema enumeration value, above the longest one Music declares.
pub const MAX_ENUM_CHARS: usize = 32;

/// Characters in an opaque Music id, device id, or YouTube id; the library bounds ids by the same
/// number of bytes.
pub const MAX_ID_CHARS: usize = 128;

/// Characters in a root, playlist, or soundscape name.
pub const MAX_NAME_CHARS: usize = 200;

/// Characters in a source collection name, which a YouTube playlist title fills.
pub const MAX_COLLECTION_NAME_CHARS: usize = 500;

/// Characters in a soundscape preset name.
pub const MAX_PRESET_NAME_CHARS: usize = 80;

/// Characters in an icon value.
pub const MAX_ICON_CHARS: usize = 500;

/// Characters in a snooze reason.
pub const MAX_REASON_CHARS: usize = 500;

/// Characters in an original title, artist, or album.
pub const MAX_DESCRIPTION_CHARS: usize = 500;

/// Characters in a title, artist, or album override.
pub const MAX_OVERRIDE_CHARS: usize = 500;

/// Characters in a location's path below its root.
pub const MAX_RELATIVE_PATH_CHARS: usize = 4096;

/// Characters in an item or collection identity key, which may embed a relative path.
pub const MAX_IDENTITY_KEY_CHARS: usize = 8192;

/// Characters in a location fingerprint.
pub const MAX_FINGERPRINT_CHARS: usize = 1024;

/// Skip ranges one membership holds.
pub const MAX_SKIP_RANGES: usize = 64;

/// Membership volume in thousandths of full volume.
pub const MAX_VOLUME_MILLI: i64 = 1000;

/// Slowest membership rate in thousandths of normal speed.
pub const MIN_RATE_MILLI: i64 = 250;

/// Fastest membership rate in thousandths of normal speed.
pub const MAX_RATE_MILLI: i64 = 2000;

/// Level of one sound in a soundscape preset, in thousandths.
pub const MAX_SOUND_LEVEL_MILLI: i64 = 1000;

/// Overall level of a soundscape preset, in thousandths.
pub const MAX_PRESET_LEVEL_MILLI: i64 = 2000;

/// Content types artwork may have.
pub const ARTWORK_CONTENT_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// A value that is not a managed artwork reference, or a reference that cannot name an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidArtworkReference;

impl fmt::Display for InvalidArtworkReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "artwork must be a managed {} reference of a supported image",
            ARTWORK_CLASS.name
        )
    }
}

impl std::error::Error for InvalidArtworkReference {}

/// The file a managed artwork reference names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkFile {
    /// Lowercase hex SHA-256 of the file bytes.
    pub sha256: String,
    /// One of [`ARTWORK_CONTENT_TYPES`].
    pub content_type: String,
}

impl ArtworkFile {
    /// The asset this file is, given its exact size.
    pub fn asset(&self, byte_size: u32) -> Result<AssetRef, InvalidArtworkReference> {
        AssetRef::new(
            &ARTWORK_CLASS,
            self.sha256.clone(),
            self.content_type.clone(),
            byte_size,
        )
        .map_err(|_| InvalidArtworkReference)
    }

    /// The stored reference, `music/artwork/{sha256}.{ext}`.
    pub fn reference(&self) -> Result<String, InvalidArtworkReference> {
        artwork_reference(&self.sha256, &self.content_type)
    }

    /// The path of the file in a vault, the engine path of [`ARTWORK_CLASS`].
    pub fn path(&self, vault_root: &Path) -> Result<PathBuf, InvalidArtworkReference> {
        let name = assets::file_name(&self.sha256, &self.content_type)
            .map_err(|_| InvalidArtworkReference)?;
        Ok(ARTWORK_CLASS.directory(vault_root).join(name))
    }
}

/// The reference stored for an artwork file, after checking the digest and content type.
pub fn artwork_reference(
    sha256: &str,
    content_type: &str,
) -> Result<String, InvalidArtworkReference> {
    if !ARTWORK_CONTENT_TYPES.contains(&content_type) {
        return Err(InvalidArtworkReference);
    }
    let name = assets::file_name(sha256, content_type).map_err(|_| InvalidArtworkReference)?;
    Ok(format!("{}/{name}", ARTWORK_CLASS.name))
}

/// The file a stored reference names. Accepts only the exact form [`artwork_reference`]
/// produces, so no other spelling, directory, or extension can reach the filesystem.
pub fn parse_artwork_reference(reference: &str) -> Result<ArtworkFile, InvalidArtworkReference> {
    let name = reference
        .strip_prefix(ARTWORK_CLASS.name)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or(InvalidArtworkReference)?;
    let (sha256, extension) = name.split_once('.').ok_or(InvalidArtworkReference)?;
    let content_type = ARTWORK_CONTENT_TYPES
        .iter()
        .find(|content_type| assets::extension(content_type) == extension)
        .ok_or(InvalidArtworkReference)?;
    let file = ArtworkFile {
        sha256: sha256.to_string(),
        content_type: (*content_type).to_string(),
    };
    if file.reference()? != reference {
        return Err(InvalidArtworkReference);
    }
    Ok(file)
}

/// Values the schema allows for each enumerated Music column.
mod allowed {
    pub const ITEM_SOURCE_KINDS: &[&str] = &["local-file", "youtube-video"];
    pub const MEDIA_KINDS: &[&str] = &["audio", "video", "unknown"];
    pub const YOUTUBE_RESOLUTION_STATES: &[&str] = &[
        "resolving",
        "ready",
        "unavailable",
        "embedding-blocked",
        "timed-out",
    ];
    pub const REVIEW_STATES: &[&str] = &["unreviewed", "reviewed", "deferred", "ignored"];
    pub const SIGNALS: &[&str] = &[
        "lyrics",
        "sudden-changes",
        "high-intensity",
        "calm",
        "repetitive",
        "energizing",
    ];
    pub const COLLECTION_KINDS: &[&str] = &["local-root", "youtube-playlist"];
    pub const REPEAT_MODES: &[&str] = &["off", "all", "one"];
    pub const INTENDED_USES: &[&str] = &["general", "focus", "reading", "relaxation", "energizing"];
    pub const WEIGHTS: &[&str] = &[
        "rarely",
        "less-often",
        "normal",
        "more-often",
        "much-more-often",
    ];
    pub const SOUNDSCAPE_SOURCE_KINDS: &[&str] = &["generated-noise", "local-loop", "bundled-loop"];
    pub const GENERATED_KINDS: &[&str] = &["white", "pink", "brown"];
    pub const OWNER_KINDS: &[&str] = &[
        "project-default",
        "event-snapshot",
        "event-override",
        "work-environment",
    ];
    pub const PHASES: &[&str] = &["focus", "short-break", "long-break"];
    pub const BEHAVIORS: &[&str] = &[
        "inherit",
        "play-automatically",
        "prepare-silently",
        "pause-music",
        "keep-current-music",
    ];
    pub const SOUNDSCAPE_BEHAVIORS: &[&str] = &[
        "inherit",
        "play-selected",
        "pause-soundscape",
        "keep-current-soundscape",
    ];
    pub const PROVENANCE_KINDS: &[&str] = &["explicit", "copied-project", "work-environment"];
}

/// Identity key prefix of a source collection over a local root.
pub const LOCAL_ROOT_IDENTITY_PREFIX: &str = "local-root:";

/// Identity key prefix of a source collection over a YouTube playlist.
pub const YOUTUBE_PLAYLIST_IDENTITY_PREFIX: &str = "youtube-playlist:";

/// Key of the seeded generated noise soundscape of `kind`.
pub fn generated_soundscape_key(kind: &str) -> String {
    format!("generated-{kind}-noise")
}

/// Rules of Music values the column types cannot express: enumerations, the multi-column
/// CHECKs inside one group, required references, non-blank opaque ids, canonical skip lists,
/// and managed artwork references.
pub struct MusicAdapter;

fn one_of(field: &Field, values: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Text(text) if values.contains(&text.as_str()) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn optional_one_of(field: &Field, values: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => one_of(field, values, what),
    }
}

/// A row key the schema declares `NOT NULL`.
fn required(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Text(_) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

/// Null or text with a non-space character, the schema's `trim(column) <> ''`.
fn optional_non_blank(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        Field::Text(text) if !text.trim_matches(' ').is_empty() => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

/// A pair of optional bounds where a present end does not precede a present start.
fn ordered_bounds(start: &Field, end: &Field, what: &'static str) -> Result<(), AdapterError> {
    match (start, end) {
        (Field::Integer(start), Field::Integer(end)) if end < start => Err(AdapterError(what)),
        _ => Ok(()),
    }
}

fn fields<'value, const N: usize>(
    value: &'value Value,
    what: &'static str,
) -> Result<&'value [Field; N], AdapterError> {
    value.fields().try_into().map_err(|_| AdapterError(what))
}

fn validate_item(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == item_group::IDENTITY {
        let [kind, _, video] = fields(value, "music item identity")?;
        one_of(kind, allowed::ITEM_SOURCE_KINDS, "music item source")?;
        match (kind, video) {
            (Field::Text(kind), Field::Null) if kind == "local-file" => Ok(()),
            (Field::Text(kind), Field::Text(video))
                if kind == "youtube-video" && !video.trim_matches(' ').is_empty() =>
            {
                Ok(())
            }
            _ => Err(AdapterError("music item identity")),
        }
    } else if group == item_group::SOURCE {
        let [media, _, _, _, _, _, resolution] = fields(value, "music item source")?;
        one_of(media, allowed::MEDIA_KINDS, "music media kind")?;
        optional_one_of(
            resolution,
            allowed::YOUTUBE_RESOLUTION_STATES,
            "music YouTube resolution",
        )
    } else if group == item_group::ARTWORK {
        artwork_asset(value).map(|_| ())
    } else if group == item_group::REVIEW {
        let [state, _, _] = fields(value, "music review")?;
        one_of(state, allowed::REVIEW_STATES, "music review state")
    } else {
        Ok(())
    }
}

/// The artwork asset an artwork group value names, or `None` when it has no override.
fn artwork_asset(value: &Value) -> Result<Option<AssetRef>, AdapterError> {
    match fields(value, "music artwork")? {
        [Field::Null, Field::Null] => Ok(None),
        [Field::Text(reference), Field::Integer(size)] => {
            let size = u32::try_from(*size).map_err(|_| AdapterError("music artwork size"))?;
            parse_artwork_reference(reference)
                .and_then(|file| file.asset(size))
                .map(Some)
                .map_err(|_| AdapterError("music artwork"))
        }
        _ => Err(AdapterError("music artwork")),
    }
}

fn validate_collection(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group != collection_group::IDENTITY {
        return Ok(());
    }
    let [kind, identity, root, playlist] = fields(value, "music collection identity")?;
    one_of(kind, allowed::COLLECTION_KINDS, "music collection kind")?;
    let (Field::Text(kind), Field::Text(identity)) = (kind, identity) else {
        return Err(AdapterError("music collection identity"));
    };
    let expected = match (kind.as_str(), root, playlist) {
        ("local-root", Field::Text(root), Field::Null) => {
            format!("{LOCAL_ROOT_IDENTITY_PREFIX}{root}")
        }
        ("youtube-playlist", Field::Null, Field::Text(playlist))
            if !playlist.trim_matches(' ').is_empty() =>
        {
            format!("{YOUTUBE_PLAYLIST_IDENTITY_PREFIX}{playlist}")
        }
        _ => return Err(AdapterError("music collection identity")),
    };
    if *identity == expected {
        Ok(())
    } else {
        Err(AdapterError("music collection identity"))
    }
}

fn validate_membership(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == membership_group::IDENTITY {
        let [playlist, item] = fields(value, "music membership identity")?;
        required(playlist, "music membership playlist")?;
        required(item, "music membership item")
    } else if group == membership_group::WEIGHT {
        let [weight] = fields(value, "music membership weight")?;
        one_of(weight, allowed::WEIGHTS, "music membership weight")
    } else if group == membership_group::RANGE || group == membership_group::BREAK_RANGE {
        let [start, end, _, _] = fields(value, "music membership range")?;
        ordered_bounds(start, end, "music membership range")
    } else if group == membership_group::SKIPS {
        validate_skip_ranges(value)
    } else {
        Ok(())
    }
}

/// Skip ranges are ordered, non-overlapping, and each ends after it starts, so a range's
/// ordinal is its index.
fn validate_skip_ranges(value: &Value) -> Result<(), AdapterError> {
    let [Field::Blob(bytes)] = value.fields() else {
        return Err(AdapterError("music skip ranges"));
    };
    let rows = codec::decode(&SKIP_RANGES, bytes).map_err(|_| AdapterError("music skip ranges"))?;
    let mut previous_end = None;
    for row in &rows {
        let [Field::Integer(start), Field::Integer(end)] = row.fields.as_slice() else {
            return Err(AdapterError("music skip ranges"));
        };
        if end <= start || previous_end.is_some_and(|previous| start < previous) {
            return Err(AdapterError("music skip ranges"));
        }
        previous_end = Some(end);
    }
    Ok(())
}

fn validate_snooze(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == snooze_group::TARGET {
        let [item, scope, playlist] = fields(value, "music snooze target")?;
        required(item, "music snooze item")?;
        match (scope, playlist) {
            (Field::Text(scope), Field::Text(_)) if scope == "playlist" => Ok(()),
            (Field::Text(scope), Field::Null) if scope == "all-playlists" => Ok(()),
            _ => Err(AdapterError("music snooze target")),
        }
    } else if group == snooze_group::WINDOW {
        match fields(value, "music snooze window")? {
            [Field::Integer(start), Field::Integer(end)] if end <= start => {
                Err(AdapterError("music snooze window"))
            }
            _ => Ok(()),
        }
    } else {
        Ok(())
    }
}

fn validate_soundscape(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group != soundscape_group::IDENTITY {
        return Ok(());
    }
    let [kind, generated, bundled] = fields(value, "music soundscape identity")?;
    one_of(
        kind,
        allowed::SOUNDSCAPE_SOURCE_KINDS,
        "music soundscape source",
    )?;
    optional_one_of(generated, allowed::GENERATED_KINDS, "music generated noise")?;
    match (kind, generated, bundled) {
        (Field::Text(kind), Field::Text(_), Field::Null) if kind == "generated-noise" => Ok(()),
        (Field::Text(kind), Field::Null, Field::Null) if kind == "local-loop" => Ok(()),
        (Field::Text(kind), Field::Null, Field::Text(identity))
            if kind == "bundled-loop" && !identity.trim_matches(' ').is_empty() =>
        {
            Ok(())
        }
        _ => Err(AdapterError("music soundscape identity")),
    }
}

fn validate_assignment(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == assignment_group::IDENTITY {
        let [owner_kind, _, phase] = fields(value, "music assignment identity")?;
        one_of(owner_kind, allowed::OWNER_KINDS, "music assignment owner")?;
        one_of(phase, allowed::PHASES, "music assignment phase")
    } else if group == assignment_group::BEHAVIOR {
        let [behavior] = fields(value, "music assignment behavior")?;
        one_of(behavior, allowed::BEHAVIORS, "music assignment behavior")
    } else if group == assignment_group::PLAYLIST {
        let [playlist] = fields(value, "music assignment playlist")?;
        optional_non_blank(playlist, "music assignment playlist")
    } else if group == assignment_group::SOUNDSCAPE_BEHAVIOR {
        let [behavior] = fields(value, "music assignment soundscape behavior")?;
        one_of(
            behavior,
            allowed::SOUNDSCAPE_BEHAVIORS,
            "music assignment soundscape behavior",
        )
    } else if group == assignment_group::PRESET {
        let [preset] = fields(value, "music assignment preset")?;
        optional_non_blank(preset, "music assignment preset")
    } else if group == assignment_group::PROVENANCE {
        let [kind, id] = fields(value, "music assignment provenance")?;
        one_of(
            kind,
            allowed::PROVENANCE_KINDS,
            "music assignment provenance",
        )?;
        optional_non_blank(id, "music assignment provenance")
    } else {
        Ok(())
    }
}

/// Checks the row keys of an identity group the schema declares `NOT NULL`.
fn required_identity(value: &Value, what: &'static str) -> Result<(), AdapterError> {
    value.fields().iter().try_for_each(|field| match field {
        Field::Null => Err(AdapterError(what)),
        _ => Ok(()),
    })
}

impl DomainAdapter for MusicAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn asset_groups(&self, table: TableId) -> &'static [GroupId] {
        if table == ITEMS_TABLE {
            &[item_group::ARTWORK]
        } else {
            &[]
        }
    }

    fn asset_refs(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<AssetRef>, AdapterError> {
        if table != ITEMS_TABLE || group != item_group::ARTWORK {
            return Ok(Vec::new());
        }
        Ok(artwork_asset(value)?.into_iter().collect())
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        match table {
            ITEMS_TABLE => validate_item(group, value),
            SIGNALS_TABLE if group == signal_group::IDENTITY => {
                let [item, signal] = fields(value, "music signal")?;
                required(item, "music signal item")?;
                one_of(signal, allowed::SIGNALS, "music signal")
            }
            STATISTICS_TABLE if group == statistics_group::IDENTITY => {
                let [device, item] = fields(value, "music statistics identity")?;
                required(device, "music statistics device")?;
                required(item, "music statistics item")
            }
            LOCATIONS_TABLE if group == location_group::IDENTITY => {
                let [root, _] = fields(value, "music location identity")?;
                required(root, "music location root")
            }
            LOCATIONS_TABLE if group == location_group::ITEM => {
                let [item] = fields(value, "music location item")?;
                required(item, "music location item")
            }
            COLLECTIONS_TABLE => validate_collection(group, value),
            COLLECTION_ITEMS_TABLE if group == collection_item_group::IDENTITY => {
                required_identity(value, "music collection item identity")
            }
            PLAYLISTS_TABLE if group == playlist_group::PLAYBACK => {
                let [_, repeat, _] = fields(value, "music playlist playback")?;
                one_of(repeat, allowed::REPEAT_MODES, "music playlist repeat mode")
            }
            INTENDED_USES_TABLE if group == intended_use_group::IDENTITY => {
                let [playlist, intended_use] = fields(value, "music intended use")?;
                required(playlist, "music intended use playlist")?;
                one_of(intended_use, allowed::INTENDED_USES, "music intended use")
            }
            MEMBERSHIPS_TABLE => validate_membership(group, value),
            SNOOZES_TABLE => validate_snooze(group, value),
            SOUNDSCAPES_TABLE => validate_soundscape(group, value),
            PRESET_SOUNDS_TABLE if group == preset_sound_group::IDENTITY => {
                required_identity(value, "music preset sound identity")
            }
            ASSIGNMENTS_TABLE => validate_assignment(group, value),
            _ => Ok(()),
        }
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        if table != PLAYLISTS_TABLE {
            return presentation;
        }
        for (group, value) in values {
            if *group == playlist_group::NAME
                && let [Field::Text(name)] = value.fields()
            {
                presentation.title = name.trim_matches(' ').to_string();
            }
        }
        presentation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn artwork_references_round_trip_every_content_type() {
        for content_type in ARTWORK_CONTENT_TYPES {
            let reference = artwork_reference(DIGEST, content_type).unwrap();
            let file = parse_artwork_reference(&reference).unwrap();
            assert_eq!(file.sha256, DIGEST);
            assert_eq!(file.content_type, content_type);
        }
        assert_eq!(
            artwork_reference(DIGEST, "image/jpeg").unwrap(),
            format!("music/artwork/{DIGEST}.jpg")
        );
    }

    #[test]
    fn artwork_references_name_the_engine_asset_path() {
        let reference = format!("music/artwork/{DIGEST}.webp");
        let asset = parse_artwork_reference(&reference)
            .unwrap()
            .asset(1_024)
            .unwrap();
        assert_eq!(
            asset.path(Path::new("vault")),
            Path::new("vault")
                .join("assets")
                .join("music")
                .join("artwork")
                .join(format!("{DIGEST}.webp"))
        );
    }

    #[test]
    fn artwork_references_refuse_paths_and_other_spellings() {
        for value in [
            String::new(),
            "/custom/artwork.png".to_string(),
            "C:\\Music\\cover.jpg".to_string(),
            "content://media/external/images/1".to_string(),
            format!("music/artwork/{DIGEST}"),
            format!("music/artwork/{DIGEST}.jpeg"),
            format!("music/artwork/{DIGEST}.PNG"),
            format!("music/artwork/{DIGEST}.bmp"),
            format!("music/artwork/{DIGEST}.bin"),
            format!("music/artwork/{}.png", DIGEST.to_uppercase()),
            format!("music/artwork/{}.png", &DIGEST[1..]),
            format!("music/artwork/../{DIGEST}.png"),
            format!("music/artwork/sub/{DIGEST}.png"),
            format!("quick-notes/images/{DIGEST}.png"),
            format!("music/artwork{DIGEST}.png"),
            format!(" music/artwork/{DIGEST}.png"),
            format!("music/artwork/{DIGEST}.png.png"),
        ] {
            assert_eq!(
                parse_artwork_reference(&value),
                Err(InvalidArtworkReference),
                "{value}"
            );
        }
        assert_eq!(
            artwork_reference(DIGEST, "image/bmp"),
            Err(InvalidArtworkReference)
        );
    }

    #[test]
    fn artwork_assets_respect_the_class_bound() {
        let file = parse_artwork_reference(&format!("music/artwork/{DIGEST}.gif")).unwrap();
        assert!(file.asset(MAX_ARTWORK_BYTES).is_ok());
        assert_eq!(file.asset(0), Err(InvalidArtworkReference));
        assert_eq!(
            file.asset(MAX_ARTWORK_BYTES + 1),
            Err(InvalidArtworkReference)
        );
    }
}
